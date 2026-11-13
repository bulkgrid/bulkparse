use super::{
    fonts::{number, object, Font},
    malformed,
};
use crate::{Context, Error, Source};
use lopdf::{Dictionary, Document, Object, ObjectId};
use std::collections::{BTreeMap, HashSet};

#[derive(Clone, Copy, Debug)]
struct Matrix([f64; 6]);
impl Default for Matrix {
    fn default() -> Self {
        Self([1., 0., 0., 1., 0., 0.])
    }
}
impl Matrix {
    fn point(self, x: f64, y: f64) -> (f64, f64) {
        let [a, b, c, d, e, f] = self.0;
        (a * x + c * y + e, b * x + d * y + f)
    }
    // self applied after rhs (column-vector notation).
    fn compose(self, rhs: Self) -> Self {
        let [a, b, c, d, e, f] = self.0;
        let [g, h, i, j, k, l] = rhs.0;
        Self([
            a * g + c * h,
            b * g + d * h,
            a * i + c * j,
            b * i + d * j,
            a * k + c * l + e,
            b * k + d * l + f,
        ])
    }
    fn translate(self, x: f64, y: f64) -> Self {
        self.compose(Self([1., 0., 0., 1., x, y]))
    }
    fn read(values: &[Object]) -> Result<Self, Error> {
        if values.len() != 6 {
            return Err(malformed("invalid PDF matrix"));
        }
        let mut out = [0.; 6];
        for (i, v) in values.iter().enumerate() {
            out[i] = number(v)?;
        }
        Ok(Self(out))
    }
}
#[derive(Clone, Default)]
struct State {
    ctm: Matrix,
    text: Matrix,
    line: Matrix,
    font: Vec<u8>,
    size: f64,
    char_space: f64,
    word_space: f64,
    leading: f64,
    scale: f64,
    rise: f64,
    render_mode: u8,
    clip: Option<[f64; 4]>,
}
impl State {
    fn initial() -> Self {
        Self {
            size: 12.,
            scale: 1.,
            ..Self::default()
        }
    }
    fn next_line(&mut self) {
        self.line = self.line.translate(0., -self.leading);
        self.text = self.line;
    }
}
#[derive(Debug, Clone)]
pub(super) struct Glyph {
    pub text: String,
    pub x: f64,
    pub y: f64,
    pub end: f64,
    pub em: f64,
    pub seq: usize,
    pub rotation: u8,
    pub actual_text: bool,
    pub bold: bool,
}
#[derive(Default)]
pub(super) struct Page {
    pub glyphs: Vec<Glyph>,
    pub rules: Vec<super::rules::Rule>,
    pub image_count: usize,
    pub image_coverage: super::inspection::ImageCoverage,
    pub text_operations: usize,
    pub invisible_text_operations: usize,
    pub undecodable_text: bool,
    pub painted_paths: usize,
    pub outlines: Vec<[f64; 4]>,
    pub uncertain_graphics: bool,
    pub incomplete_content: bool,
    pub approximate: bool,
    pub unmapped: bool,
    pub rotated: bool,
    bytes: usize,
    expanded: usize,
}

fn dictionary<'a>(doc: &'a Document, value: &'a Object) -> Result<&'a Dictionary, Error> {
    object(doc, value)?.as_dict().map_err(malformed)
}
fn resources(doc: &Document, id: ObjectId, ctx: &Context<'_>) -> Result<Dictionary, Error> {
    let mut current = id;
    let mut seen = HashSet::new();
    loop {
        ctx.depth(seen.len())?;
        if !seen.insert(current) {
            return Err(malformed("cyclic PDF page tree"));
        }
        let node = doc.get_dictionary(current).map_err(malformed)?;
        if let Ok(value) = node.get(b"Resources") {
            return Ok(dictionary(doc, value)?.clone());
        }
        match node.get(b"Parent").and_then(Object::as_reference) {
            Ok(parent) => current = parent,
            Err(_) => return Ok(Dictionary::new()),
        }
    }
}
struct Walker<'a, 'b, 'c> {
    doc: &'a Document,
    ctx: &'b mut Context<'c>,
    page: Page,
    active: Vec<ObjectId>,
    source: Source,
}
impl Walker<'_, '_, '_> {
    fn image(&mut self, matrix: Matrix, clip: Option<[f64; 4]>) -> Result<(), Error> {
        let points = [
            matrix.point(0., 0.),
            matrix.point(1., 0.),
            matrix.point(1., 1.),
            matrix.point(0., 1.),
        ];
        if points
            .iter()
            .any(|&(x, y)| !x.is_finite() || !y.is_finite() || x.abs() > 1e12 || y.abs() > 1e12)
        {
            return Err(malformed("excessive PDF image coordinate"));
        }
        let [a, b, c, d, _, _] = matrix.0;
        if !((b.abs() < 1e-9 && c.abs() < 1e-9) || (a.abs() < 1e-9 && d.abs() < 1e-9)) {
            self.page.uncertain_graphics = true;
        }
        let rect = [
            points.iter().map(|p| p.0).fold(f64::INFINITY, f64::min),
            points.iter().map(|p| p.1).fold(f64::INFINITY, f64::min),
            points.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max),
            points.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max),
        ];
        let rect = intersect(rect, clip);
        let rect = intersect(rect, self.page.image_coverage.page_box);
        if rect[2] > rect[0] && rect[3] > rect[1] {
            self.page.image_count += 1;
            self.page.image_coverage.add(rect);
        }
        Ok(())
    }

    fn walk(
        &mut self,
        bytes: &[u8],
        resources: &Dictionary,
        mut state: State,
        depth: usize,
    ) -> Result<(), Error> {
        self.ctx.depth(depth)?;
        self.page.expanded = self.page.expanded.saturating_add(bytes.len());
        if self.page.expanded > self.ctx.options.limits.max_expanded_bytes {
            return Err(Error::Limit("max_expanded_bytes"));
        }
        let mut fonts = BTreeMap::new();
        if let Ok(value) = resources.get(b"Font") {
            for (name, value) in dictionary(self.doc, value)?.iter() {
                let font = Font::new(self.doc, dictionary(self.doc, value)?, self.ctx);
                if let Err(error @ (Error::Limit(_) | Error::Cancelled)) = font {
                    return Err(error);
                }
                fonts.insert(name.clone(), font);
            }
        }
        let (content, repaired, incomplete) = super::content::decode(bytes);
        if repaired {
            self.ctx.diagnostic(
                "pdf_content_spacing_repaired",
                "Normalized PDF comments and whitespace for complete content decoding.",
                self.source.clone(),
                false,
            );
        }
        if incomplete {
            self.page.incomplete_content = true;
            self.page.uncertain_graphics = true;
            self.ctx.diagnostic("pdf_incomplete_content", "Content decoding stopped before the end of a stream. Available native content is retained; review the page.", self.source.clone(), true);
        }
        let mut stack = Vec::new();
        let mut marked: Vec<(usize, Option<String>)> = Vec::new();
        let mut path = super::rules::Path::default();
        let mut path_present = false;
        for op in &content.operations {
            self.ctx.node()?;
            path.operation(
                op,
                |x, y| state.ctm.point(x, y),
                &mut self.page.rules,
                &mut self.page.outlines,
                self.ctx,
            )?;
            let value = |i: usize| -> Result<f64, Error> {
                number(
                    op.operands
                        .get(i)
                        .ok_or_else(|| malformed("missing PDF operand"))?,
                )
            };
            match op.operator.as_str() {
                "S" | "s" | "f" | "F" | "f*" | "B" | "B*" | "b" | "b*" => {
                    self.page.painted_paths += usize::from(path_present);
                    path_present = false;
                }
                "m" | "l" | "c" | "v" | "y" | "re" => path_present = true,
                "n" => path_present = false,
                // Unhandled clipping, transparency, patterns and shadings make
                // image coverage uncertain; never call such a page blank.
                "W" | "W*" | "gs" | "sh" | "scn" | "SCN" => self.page.uncertain_graphics = true,
                "Tr" => {
                    let mode = value(0)?;
                    if !(0.0..=7.0).contains(&mode) || mode.fract() != 0.0 {
                        return Err(malformed("invalid PDF text rendering mode"));
                    }
                    state.render_mode = mode as u8;
                }
                "BMC" | "BDC" => {
                    self.ctx.depth(depth + marked.len() + 1)?;
                    let property = op.operands.get(1).and_then(|value| {
                        if let Object::Name(name) = value {
                            resources
                                .get(b"Properties")
                                .ok()
                                .and_then(|v| dictionary(self.doc, v).ok())
                                .and_then(|d| d.get(name).ok())
                        } else {
                            Some(value)
                        }
                    });
                    let actual = property
                        .and_then(|v| dictionary(self.doc, v).ok())
                        .and_then(|d| d.get(b"ActualText").ok())
                        .map(lopdf::decode_text_string)
                        .transpose()
                        .map_err(malformed)?;
                    if actual
                        .as_ref()
                        .is_some_and(|s| s.len() > self.ctx.options.limits.max_text_bytes)
                    {
                        return Err(Error::Limit("max_text_bytes"));
                    }
                    marked.push((self.page.glyphs.len(), actual));
                }
                "EMC" => {
                    let (start, actual) = marked
                        .pop()
                        .ok_or_else(|| malformed("unbalanced PDF marked content"))?;
                    if let Some(text) = actual {
                        if let Some(first) = self.page.glyphs.get(start).cloned() {
                            let end = self.page.glyphs[start..]
                                .iter()
                                .map(|g| g.end)
                                .fold(first.end, f64::max);
                            let x = self.page.glyphs[start..]
                                .iter()
                                .map(|g| g.x)
                                .fold(first.x, f64::min);
                            self.page.glyphs.truncate(start);
                            self.page.bytes = self.page.bytes.saturating_add(text.len());
                            if self.page.bytes > self.ctx.options.limits.max_text_bytes {
                                return Err(Error::Limit("max_text_bytes"));
                            }
                            self.page.glyphs.push(Glyph {
                                text,
                                actual_text: true,
                                x,
                                end,
                                ..first
                            });
                        }
                    }
                }
                "q" => {
                    self.ctx.depth(depth + stack.len() + 1)?;
                    stack.push(state.clone());
                }
                "Q" => {
                    let saved = stack
                        .pop()
                        .ok_or_else(|| malformed("unbalanced PDF graphics state"))?;
                    let (text, line) = (state.text, state.line);
                    state = saved;
                    state.text = text;
                    state.line = line;
                }
                "cm" => state.ctm = state.ctm.compose(Matrix::read(&op.operands)?),
                "BT" => {
                    state.text = Matrix::default();
                    state.line = Matrix::default();
                }
                "Tf" => {
                    state.font = op
                        .operands
                        .first()
                        .ok_or_else(|| malformed("missing PDF font"))?
                        .as_name()
                        .map_err(malformed)?
                        .to_vec();
                    state.size = value(1)?;
                }
                "Tm" => {
                    state.text = Matrix::read(&op.operands)?;
                    state.line = state.text;
                }
                "Td" | "TD" => {
                    let (tx, ty) = (value(0)?, value(1)?);
                    if op.operator == "TD" {
                        state.leading = -ty;
                    }
                    state.line = state.line.translate(tx, ty);
                    state.text = state.line;
                }
                "Tc" => state.char_space = value(0)?,
                "Tw" => state.word_space = value(0)?,
                "Tz" => state.scale = value(0)? / 100.,
                "TL" => state.leading = value(0)?,
                "Ts" => state.rise = value(0)?,
                "T*" => state.next_line(),
                "Tj" | "TJ" | "'" | "\"" => {
                    self.page.text_operations += 1;
                    if matches!(state.render_mode, 3 | 7) {
                        self.page.invisible_text_operations += 1;
                    }
                    if op.operator == "\"" {
                        state.word_space = value(0)?;
                        state.char_space = value(1)?;
                    }
                    if matches!(op.operator.as_str(), "'" | "\"") {
                        state.next_line();
                    }
                    let operand = op
                        .operands
                        .get(if op.operator == "\"" { 2 } else { 0 })
                        .ok_or_else(|| malformed("missing PDF text"))?;
                    let Some(Ok(font)) = fonts.get(&state.font) else {
                        self.page.undecodable_text = true;
                        self.ctx.diagnostic("pdf_font_error","A text run uses a missing or unsupported font; other page text is retained.",self.source.clone(),true);
                        continue;
                    };
                    if font.vertical {
                        self.ctx.diagnostic("pdf_vertical_text","Vertical font metrics are not supported; extracted order is uncertain.",self.source.clone(),true);
                    }
                    self.page.approximate |= font.approximate;
                    if font.recovered_codespace {
                        self.ctx.diagnostic("pdf_cmap_codespace_recovery", "ToUnicode codespace disagrees with a single-byte font; recovery uses the font's one-byte character codes.", self.source.clone(), true);
                    }
                    let items = if let Object::Array(items) = operand {
                        items.as_slice()
                    } else {
                        std::slice::from_ref(operand)
                    };
                    for item in items {
                        match item {
                            Object::String(bytes, _) => {
                                for (text, width, word_space) in font.decode(bytes, self.ctx)? {
                                    self.ctx.node()?;
                                    self.page.bytes = self.page.bytes.saturating_add(text.len());
                                    if self.page.bytes > self.ctx.options.limits.max_text_bytes {
                                        return Err(Error::Limit("max_text_bytes"));
                                    }
                                    self.page.unmapped |= text.contains('\u{fffd}');
                                    let advance = (width / 1000. * state.size
                                        + state.char_space
                                        + if word_space { state.word_space } else { 0. })
                                        * state.scale;
                                    let transform = state.ctm.compose(state.text);
                                    let (x, y) = transform.point(0., state.rise);
                                    let (end, end_y) = transform.point(
                                        width / 1000. * state.size * state.scale,
                                        state.rise,
                                    );
                                    let (hx, hy) = transform.point(0., state.rise + state.size);
                                    let em = (hx - x).hypot(hy - y).max(0.1);
                                    if [x, y, end, end_y, em, advance]
                                        .iter()
                                        .any(|n| !n.is_finite() || n.abs() > 1e12)
                                    {
                                        return Err(malformed(
                                            "excessive transformed PDF coordinate",
                                        ));
                                    }
                                    let (dx, dy) = transform.point(1., state.rise);
                                    let (dx, dy) = (dx - x, dy - y);
                                    let rotation = if dy.abs() < dx.abs() * 0.2 {
                                        if dx >= 0. {
                                            0
                                        } else {
                                            2
                                        }
                                    } else if dx.abs() < dy.abs() * 0.2 {
                                        if dy >= 0. {
                                            1
                                        } else {
                                            3
                                        }
                                    } else {
                                        4
                                    };
                                    let rotate = |x: f64, y: f64| match rotation {
                                        1 => (y, -x),
                                        2 => (-x, -y),
                                        3 => (-y, x),
                                        _ => (x, y),
                                    };
                                    let (x, y) = rotate(x, y);
                                    let (end, _) = rotate(end, end_y);
                                    self.page.rotated |= rotation != 0;
                                    self.page.glyphs.push(Glyph {
                                        text,
                                        x: x.min(end),
                                        y,
                                        end: x.max(end),
                                        em,
                                        seq: self.page.glyphs.len(),
                                        rotation,
                                        actual_text: false,
                                        bold: font.bold,
                                    });
                                    state.text = state.text.translate(advance, 0.);
                                }
                            }
                            Object::Integer(_) | Object::Real(_) => {
                                state.text = state.text.translate(
                                    -number(item)? / 1000. * state.size * state.scale,
                                    0.,
                                )
                            }
                            _ => return Err(malformed("invalid PDF text operand")),
                        }
                    }
                }
                "Do" => {
                    let name = op
                        .operands
                        .first()
                        .ok_or_else(|| malformed("missing XObject name"))?
                        .as_name()
                        .map_err(malformed)?;
                    let Some(value) = resources
                        .get(b"XObject")
                        .ok()
                        .and_then(|v| dictionary(self.doc, v).ok())
                        .and_then(|d| d.get(name).ok())
                    else {
                        self.page.uncertain_graphics = true;
                        continue;
                    };
                    let id = value.as_reference().ok();
                    if id.is_some_and(|id| self.active.contains(&id)) {
                        return Err(malformed("cyclic PDF Form XObject"));
                    }
                    let stream = object(self.doc, value)?.as_stream().map_err(malformed)?;
                    match stream.dict.get(b"Subtype").and_then(Object::as_name).ok() {
                        Some(b"Image") => self.image(state.ctm, state.clip)?,
                        Some(b"Form") => {
                            let mut form_state = state.clone();
                            if let Ok(matrix) =
                                stream.dict.get(b"Matrix").and_then(Object::as_array)
                            {
                                form_state.ctm = form_state.ctm.compose(Matrix::read(matrix)?);
                            }
                            if let Ok(bbox) = stream.dict.get(b"BBox") {
                                if let Some(b) = read_box(self.doc, bbox) {
                                    let [a, b1, c, d, _, _] = form_state.ctm.0;
                                    if !((b1.abs() < 1e-9 && c.abs() < 1e-9)
                                        || (a.abs() < 1e-9 && d.abs() < 1e-9))
                                    {
                                        self.page.uncertain_graphics = true;
                                    }
                                    let points = [
                                        form_state.ctm.point(b[0], b[1]),
                                        form_state.ctm.point(b[2], b[1]),
                                        form_state.ctm.point(b[2], b[3]),
                                        form_state.ctm.point(b[0], b[3]),
                                    ];
                                    if points.iter().any(|&(x, y)| {
                                        !x.is_finite()
                                            || !y.is_finite()
                                            || x.abs() > 1e12
                                            || y.abs() > 1e12
                                    }) {
                                        self.page.uncertain_graphics = true;
                                    } else {
                                        let rect = [
                                            points
                                                .iter()
                                                .map(|p| p.0)
                                                .fold(f64::INFINITY, f64::min),
                                            points
                                                .iter()
                                                .map(|p| p.1)
                                                .fold(f64::INFINITY, f64::min),
                                            points
                                                .iter()
                                                .map(|p| p.0)
                                                .fold(f64::NEG_INFINITY, f64::max),
                                            points
                                                .iter()
                                                .map(|p| p.1)
                                                .fold(f64::NEG_INFINITY, f64::max),
                                        ];
                                        form_state.clip = Some(intersect(rect, form_state.clip));
                                    }
                                } else {
                                    // Geometry uncertainty must not discard otherwise
                                    // readable native text from malformed producers.
                                    self.page.uncertain_graphics = true;
                                }
                            }
                            let form_resources = stream
                                .dict
                                .get(b"Resources")
                                .ok()
                                .map(|v| dictionary(self.doc, v))
                                .transpose()?
                                .unwrap_or(resources);
                            if let Some(id) = id {
                                self.active.push(id);
                            }
                            self.walk(
                                &stream.get_plain_content().map_err(malformed)?,
                                form_resources,
                                form_state,
                                depth + 1,
                            )?;
                            if id.is_some() {
                                self.active.pop();
                            }
                        }
                        _ => {}
                    }
                }
                "BI" => self.image(state.ctm, state.clip)?,
                _ => {}
            }
        }
        Ok(())
    }
}
pub(super) fn extract(
    doc: &Document,
    id: ObjectId,
    source: Source,
    ctx: &mut Context<'_>,
) -> Result<Page, Error> {
    let resources = resources(doc, id, ctx)?;
    let mut walker = Walker {
        doc,
        ctx,
        page: Page::default(),
        active: Vec::new(),
        source,
    };
    walker.page.image_coverage.page_box = page_box(doc, id, walker.ctx)?;
    // Annotation appearances and optional content require visual inspection.
    if doc
        .get_dictionary(id)
        .map_err(malformed)?
        .get(b"Annots")
        .ok()
        .is_some_and(|v| !matches!(object(doc, v), Ok(Object::Array(a)) if a.is_empty()))
    {
        walker.page.uncertain_graphics = true;
    }
    walker.walk(
        &doc.get_page_content(id).map_err(malformed)?,
        &resources,
        State::initial(),
        0,
    )?;
    Ok(walker.page)
}

// Resolve inherited boxes independently: CropBox may be on a different ancestor
// from MediaBox. Invalid/missing geometry is unknown, not a guessed Letter page.
fn page_box(doc: &Document, id: ObjectId, ctx: &Context<'_>) -> Result<Option<[f64; 4]>, Error> {
    let mut current = id;
    let mut seen = HashSet::new();
    let (mut media, mut crop) = (None, None);
    loop {
        ctx.depth(seen.len())?;
        if !seen.insert(current) {
            return Err(malformed("cyclic PDF page tree"));
        }
        let node = doc.get_dictionary(current).map_err(malformed)?;
        for (name, slot) in [
            (b"MediaBox".as_slice(), &mut media),
            (b"CropBox".as_slice(), &mut crop),
        ] {
            if slot.is_none() {
                if let Ok(value) = node.get(name) {
                    let Some(bounds) = read_box(doc, value) else {
                        return Ok(None);
                    };
                    if bounds[2] <= bounds[0] || bounds[3] <= bounds[1] {
                        return Ok(None);
                    }
                    *slot = Some(bounds);
                }
            }
        }
        match node.get(b"Parent").and_then(Object::as_reference) {
            Ok(parent) => current = parent,
            Err(_) => break,
        }
    }
    Ok(match (media, crop) {
        (Some(m), Some(c)) => {
            let r = [
                m[0].max(c[0]),
                m[1].max(c[1]),
                m[2].min(c[2]),
                m[3].min(c[3]),
            ];
            (r[2] > r[0] && r[3] > r[1]).then_some(r)
        }
        (m, _) => m,
    })
}

fn intersect(rect: [f64; 4], clip: Option<[f64; 4]>) -> [f64; 4] {
    match clip {
        Some(c) => [
            rect[0].max(c[0]),
            rect[1].max(c[1]),
            rect[2].min(c[2]),
            rect[3].min(c[3]),
        ],
        None => rect,
    }
}

fn read_box(doc: &Document, value: &Object) -> Option<[f64; 4]> {
    let values = object(doc, value).ok()?.as_array().ok()?;
    if values.len() != 4 {
        return None;
    }
    let mut bounds = [0.; 4];
    for (i, v) in values.iter().enumerate() {
        bounds[i] = number(object(doc, v).ok()?).ok()?;
    }
    if bounds.iter().any(|v| !v.is_finite() || v.abs() > 1e12) {
        return None;
    }
    Some([
        bounds[0].min(bounds[2]),
        bounds[1].min(bounds[3]),
        bounds[0].max(bounds[2]),
        bounds[1].max(bounds[3]),
    ])
}
