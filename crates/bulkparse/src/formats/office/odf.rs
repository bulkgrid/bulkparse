use super::style::{label, TextStyle};
use super::*;
#[derive(Clone, Default)]
struct Definition {
    parent: String,
    text: TextStyle,
    kind: Option<&'static str>,
    outline: Option<u8>,
}
#[derive(Default)]
struct Styles {
    equations: HashMap<String, Inline>,
    // Group by family so repeated inheritance lookups can borrow both keys.
    // Tuple keys required allocating two temporary Strings for every lookup.
    defs: HashMap<String, HashMap<String, Definition>>,
    defaults: HashMap<String, Definition>,
    lists: HashMap<(String, usize), (String, u64)>,
    ends: HashMap<String, u64>,
    last: HashMap<(String, usize), u64>,
}
impl Styles {
    fn load(&mut self, doc: &Document<'_>) -> Result<(), Error> {
        for n in doc
            .descendants()
            .filter(|n| named(*n, "style") || named(*n, "default-style"))
        {
            let name = attr(n, "name").unwrap_or("");
            let family = attr(n, "family").unwrap_or("");
            let mut d = Definition {
                parent: attr(n, "parent-style-name").unwrap_or("").into(),
                ..Default::default()
            };
            let display = attr(n, "display-name")
                .unwrap_or(name)
                .replace("_20_", " ")
                .to_ascii_lowercase();
            d.kind = match display.as_str() {
                "preformatted text" => Some("code"),
                "quotations" | "quotation" => Some("quote"),
                _ => None,
            };
            d.outline = attr(n, "default-outline-level")
                .and_then(|s| s.parse::<u8>().ok())
                .filter(|l| *l > 0)
                .map(|l| l.min(6));
            if let Some(p) = child(n, "text-properties") {
                d.text.bold = attr(p, "font-weight")
                    .map(|v| v == "bold" || v.parse::<u32>().is_ok_and(|n| n >= 600));
                d.text.italic = attr(p, "font-style").map(|v| matches!(v, "italic" | "oblique"));
                d.text.strike = attr(p, "text-line-through-style").map(|v| v != "none");
            }
            if named(n, "default-style") {
                self.defaults.insert(family.into(), d);
            } else {
                self.defs
                    .entry(family.into())
                    .or_default()
                    .insert(name.into(), d);
            }
        }
        for list in doc.descendants().filter(|n| named(*n, "list-style")) {
            let name = attr(list, "name").unwrap_or("");
            for level in list.children().filter(Node::is_element) {
                let l = count(attr(level, "level"), 1)?;
                let kind = if named(level, "list-level-style-number") {
                    attr(level, "num-format").unwrap_or("1")
                } else {
                    ""
                };
                let start = count(attr(level, "start-value"), 1)? as u64;
                self.lists.insert((name.into(), l), (kind.into(), start));
            }
        }
        Ok(())
    }
    fn resolve(&self, name: &str, family: &str, ctx: &Context<'_>) -> Result<Definition, Error> {
        let mut seen = HashSet::new();
        let mut chain = vec![];
        let mut current = name;
        while !current.is_empty() {
            if chain.len() >= ctx.options.limits.max_depth {
                return Err(Error::Limit("max_depth"));
            }
            if !seen.insert(current) {
                return Err(bad("cyclic ODF style inheritance"));
            }
            let Some(d) = self
                .defs
                .get(family)
                .and_then(|definitions| definitions.get(current))
                .or_else(|| {
                    self.defs
                        .get("")
                        .and_then(|definitions| definitions.get(current))
                })
            else {
                break;
            };
            chain.push(d);
            current = &d.parent;
        }
        let mut out = self.defaults.get(family).cloned().unwrap_or_default();
        for d in chain.into_iter().rev() {
            out.text = out.text.merge(d.text);
            out.kind = d.kind.or(out.kind);
            out.outline = d.outline.or(out.outline)
        }
        Ok(out)
    }
}

fn inline(
    n: Node<'_, '_>,
    ctx: &mut Context<'_>,
    styles: &Styles,
    inherited: TextStyle,
) -> Result<Vec<Inline>, Error> {
    let inherited = inherited.merge(
        styles
            .resolve(
                attr(n, "style-name").unwrap_or(""),
                if named(n, "p") || named(n, "h") {
                    "paragraph"
                } else {
                    "text"
                },
                ctx,
            )?
            .text,
    );
    // A spreadsheet paragraph commonly contains one run. Starting at one
    // avoids Vec's four-slot first allocation being retained in every cell;
    // richer paragraphs still grow normally, without a second shrinking pass.
    let mut out = Vec::with_capacity(usize::from(n.first_child().is_some()));
    for c in n.children() {
        if c.is_text() {
            let t = c.text().unwrap_or("");
            ctx.text(t)?;
            out.push(inherited.text(t.to_owned()));
            continue;
        }
        if !c.is_element() {
            continue;
        }
        match c.tag_name().name() {
            "line-break" => out.push(Inline::Break),
            "tab" => {
                ctx.text("\t")?;
                out.push(Inline::text("\t"));
            }
            "s" => {
                let k = count(attr(c, "c"), 1)?;
                if k > ctx.options.limits.max_text_bytes {
                    return Err(Error::Limit("max_text_bytes"));
                }
                let t = " ".repeat(k);
                ctx.text(&t)?;
                out.push(inherited.text(t.to_owned()));
            }
            "a" => {
                let children = inline(c, ctx, styles, inherited)?;
                out.push(Inline::Link {
                    url: attr(c, "href").unwrap_or("").into(),
                    children,
                });
            }
            "span" => out.extend(inline(c, ctx, styles, inherited)?),
            "note" => {
                out.extend(inline(c, ctx, styles, inherited)?);
                ctx.diagnostic(
                    "odf_note",
                    "Note content retained inline.",
                    Default::default(),
                    true,
                );
            }
            "math" => out.push(super::math::inline(
                c,
                ctx,
                &Source {
                    part: Some("content.xml".into()),
                    ..Default::default()
                },
            )?),
            "object" if attr(c, "href").is_some_and(|h| styles.equations.contains_key(h)) => {
                let mut equation = styles.equations[attr(c, "href").unwrap()].clone();
                if let Inline::Math { display, .. } = &mut equation {
                    *display = false;
                }
                if let Inline::Math { tex, .. } = &equation {
                    ctx.text(tex)?;
                }
                out.push(equation);
            }
            "object" if c.descendants().any(|n| named(n, "math")) => {
                let math = c.descendants().find(|n| named(*n, "math")).unwrap();
                let mut equation = super::math::inline(math, ctx, &Default::default())?;
                if let Inline::Math { display, .. } = &mut equation {
                    *display = false;
                }
                out.push(equation);
            }
            "image"
                if c.parent().is_some_and(|p| {
                    p.children().filter(|n| named(*n, "object")).any(|n| {
                        attr(n, "href").is_some_and(|h| styles.equations.contains_key(h))
                            || n.descendants().any(|n| named(n, "math"))
                    })
                }) => {}
            "image" if attr(c, "href").and_then(|p| ctx.image_inline(p, "")).map(|image| out.push(image)).is_some() => {},
            "image" | "object" | "object-ole" => ctx.diagnostic(
                "odf_object",
                "Embedded image/object is not reconstructed in the text flow. Supported package image files are retained separately when enabled.",
                Default::default(),
                true,
            ),
            _ => out.extend(inline(c, ctx, styles, inherited)?),
        }
    }
    // Empty/unmapped content must not retain the speculative first slot.
    if out.is_empty() {
        return Ok(Vec::new());
    }
    Ok(out)
}
fn plain(n: Node<'_, '_>, limit: usize) -> Result<String, Error> {
    let mut out = String::new();
    for c in n.descendants() {
        if c.is_text() {
            out.push_str(c.text().unwrap_or(""))
        } else if named(c, "s") {
            let n = count(attr(c, "c"), 1)?;
            if n > limit.saturating_sub(out.len()) {
                return Err(Error::Limit("max_text_bytes"));
            }
            out.push_str(&" ".repeat(n))
        } else if named(c, "tab") {
            out.push('\t')
        } else if named(c, "line-break") {
            out.push('\n')
        }
        if out.len() > limit {
            return Err(Error::Limit("max_text_bytes"));
        }
    }
    Ok(out)
}
fn duration(s: &str) -> Option<String> {
    let negative = s.starts_with('-');
    let s = s.trim_start_matches('-').strip_prefix('P')?;
    let mut time = false;
    let mut number = String::new();
    let mut total = 0f64;
    let mut seen = HashSet::new();
    for c in s.chars() {
        if c == 'T' {
            if time || !number.is_empty() {
                return None;
            }
            time = true;
            continue;
        }
        if c.is_ascii_digit() || c == '.' {
            number.push(c);
            continue;
        }
        let n = number.parse::<f64>().ok()?;
        number.clear();
        if !seen.insert((time, c)) {
            return None;
        }
        let unit = match (time, c) {
            (false, 'W') => 604800.,
            (false, 'D') => 86400.,
            (true, 'H') => 3600.,
            (true, 'M') => 60.,
            (true, 'S') => 1.,
            _ => return None,
        };
        total += n * unit;
    }
    if !number.is_empty() || seen.is_empty() || !total.is_finite() || total > 9_000_000_000. {
        return None;
    }
    let ms = (total * 1000.).round() as u64;
    let seconds = ms / 1000;
    Some(format!(
        "{}{}:{:02}:{:02}{}",
        if negative && ms != 0 { "-" } else { "" },
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60,
        if ms.is_multiple_of(1000) {
            String::new()
        } else {
            format!(".{:03}", ms % 1000)
        }
    ))
}
fn value(n: Node<'_, '_>, limit: usize) -> Result<String, Error> {
    let mut paragraphs = n.children().filter(|n| named(*n, "p") || named(*n, "h"));
    if let Some(first) = paragraphs.next() {
        // Most cells contain one paragraph. Reuse its buffer instead of
        // collecting paragraphs and copying them into another joined string.
        let mut text = plain(first, limit)?;
        for paragraph in paragraphs {
            text.push('\n');
            text.push_str(&plain(paragraph, limit)?);
        }
        return Ok(text);
    }
    for key in [
        "string-value",
        "date-value",
        "time-value",
        "boolean-value",
        "value",
    ] {
        if let Some(v) = attr(n, key) {
            return Ok(if key == "time-value" {
                duration(v).unwrap_or_else(|| v.into())
            } else {
                v.into()
            });
        }
    }
    Ok(String::new())
}
// Planning data is recorded while validating a streamed row. This avoids
// parsing its XML again solely to calculate repeats, bounds and cached values.
#[derive(serde::Serialize, serde::Deserialize)]
struct RowPlan {
    #[serde(rename = "r", default, skip_serializing_if = "Option::is_none")]
    repeat: Option<String>,
    #[serde(rename = "h")]
    header: bool,
    #[serde(rename = "c")]
    cells: Vec<CellPlan>,
}
#[derive(serde::Serialize, serde::Deserialize)]
struct CellPlan {
    #[serde(rename = "n")]
    node: u32,
    #[serde(rename = "c", default, skip_serializing_if = "Option::is_none")]
    copies: Option<String>,
    #[serde(rename = "s", default, skip_serializing_if = "Option::is_none")]
    col_span: Option<String>,
    #[serde(rename = "r", default, skip_serializing_if = "Option::is_none")]
    row_span: Option<String>,
    #[serde(rename = "v")]
    value: PlannedValue,
    #[serde(rename = "f", default, skip_serializing_if = "Option::is_none")]
    formula: Option<String>,
    #[serde(rename = "x")]
    complex: bool,
    #[serde(rename = "p")]
    plain_paragraph: bool,
}
#[derive(serde::Serialize, serde::Deserialize)]
enum PlannedValue {
    Text(String),
    TooLong,
    Malformed(String),
}
impl RowPlan {
    fn from_node(row: Node<'_, '_>, limit: usize) -> Self {
        Self {
            repeat: attr(row, "number-rows-repeated").map(str::to_owned),
            header: row
                .ancestors()
                .take_while(|p| !named(*p, "table"))
                .any(|p| named(p, "table-header-rows")),
            cells: row
                .children()
                .filter(|n| named(*n, "table-cell") || named(*n, "covered-table-cell"))
                .map(|c| CellPlan {
                    node: c.id().get(),
                    copies: attr(c, "number-columns-repeated").map(str::to_owned),
                    col_span: attr(c, "number-columns-spanned").map(str::to_owned),
                    row_span: attr(c, "number-rows-spanned").map(str::to_owned),
                    value: match value(c, limit) {
                        Ok(text) => PlannedValue::Text(text),
                        Err(Error::Limit(_)) => PlannedValue::TooLong,
                        Err(Error::Malformed(message)) => PlannedValue::Malformed(message),
                        Err(_) => {
                            unreachable!("plain cell values only fail on syntax or text limits")
                        }
                    },
                    formula: attr(c, "formula").map(str::to_owned),
                    complex: c.children().any(|n| {
                        n.is_element()
                            && matches!(n.tag_name().name(), "list" | "table" | "frame" | "section")
                    }),
                    plain_paragraph: {
                        let mut children = c.children().filter(Node::is_element);
                        children.next().is_some_and(|p| {
                            named(p, "p")
                                && p.attributes().len() == 0
                                && p.first_child()
                                    .is_some_and(|t| t.is_text() && t.next_sibling().is_none())
                        }) && children.next().is_none()
                    },
                })
                .collect(),
        }
    }
}
fn odf_table(
    n: Node<'_, '_>,
    src: &Source,
    ctx: &mut Context<'_>,
    depth: usize,
    styles: &mut Styles,
) -> Result<Option<Block>, Error> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        odf_table_rows(n, src, ctx, depth, styles, None, None)
    }
    #[cfg(target_arch = "wasm32")]
    {
        odf_table_rows(n, src, ctx, depth, styles, None)
    }
}
fn odf_table_rows(
    n: Node<'_, '_>,
    src: &Source,
    ctx: &mut Context<'_>,
    depth: usize,
    styles: &mut Styles,
    #[cfg(not(target_arch = "wasm32"))] mut parts: Option<&mut super::xml_parts::Parts>,
    output: Option<&mut crate::render::Writer<'_>>,
) -> Result<Option<Block>, Error> {
    ctx.depth(depth)?;
    let mut templates = BTreeMap::new();
    let mut header_rows = 0usize;
    let mut grid = BTreeMap::new();
    let mut merges = vec![];
    let mut row = 1u32;
    let mut text_bytes = 0usize;
    {
        let text_limit = ctx.options.limits.max_text_bytes;
        let mut plan_row = |r: RowPlan, fragment: Option<usize>| -> Result<(), Error> {
            let repeat = count(r.repeat.as_deref(), 1)?;
            if repeat == 0 || repeat > 1_048_576 || u64::from(row) + repeat as u64 > 1_048_577 {
                return Err(Error::Limit("max_table_cells"));
            }
            if header_rows == row as usize - 1 && r.header {
                header_rows += repeat;
            }
            let mut columns = vec![];
            let mut planned_cells = grid.len();
            let mut column = 1u32;
            for c in r.cells {
                let copies = count(c.copies.as_deref(), 1)?;
                if copies == 0 || copies > 16384 || u64::from(column) + copies as u64 > 16385 {
                    return Err(Error::Limit("max_table_cells"));
                }
                let mut text = match c.value {
                    PlannedValue::Text(text) => text,
                    PlannedValue::TooLong => return Err(Error::Limit("max_text_bytes")),
                    PlannedValue::Malformed(message) => return Err(bad(message)),
                };
                let cs = count(c.col_span.as_deref(), 1)?;
                let rs = count(c.row_span.as_deref(), 1)?;
                if cs == 0 || rs == 0 || cs.saturating_mul(rs) > ctx.options.limits.max_table_cells
                {
                    return Err(Error::Limit("max_table_cells"));
                }
                if let Some(formula) = c.formula {
                    if text.is_empty() {
                        warn(
                            ctx,
                            "ods_formula_missing_cache",
                            "Formula without cached text/value; expression retained.",
                            src,
                        );
                        text = formula.to_owned();
                    } else {
                        ctx.diagnostic(
                            "ods_formula_cache",
                            "Cached formula value used; no evaluation.",
                            src.clone(),
                            false,
                        );
                    }
                }
                if !text.is_empty() || cs > 1 || rs > 1 || c.complex {
                    ctx.repeat_cells(copies.saturating_mul(repeat).saturating_sub(1))?;
                    planned_cells = planned_cells.saturating_add(copies.saturating_mul(repeat));
                    if planned_cells > ctx.options.limits.max_table_cells {
                        return Err(Error::Limit("max_table_cells"));
                    }
                    text_bytes = text_bytes
                        .saturating_add(text.len().saturating_mul(copies).saturating_mul(repeat));
                    if text_bytes > ctx.options.limits.max_text_bytes {
                        return Err(Error::Limit("max_text_bytes"));
                    }
                    columns.push((
                        column,
                        copies,
                        text,
                        rs as u32,
                        cs as u32,
                        roxmltree::NodeId::new(c.node),
                        c.plain_paragraph,
                    ));
                }
                column += copies as u32;
            }
            for i in 0..if columns.is_empty() { 0 } else { repeat } {
                ctx.check()?;
                for (col, copies, text, rs, cs, node, plain) in &columns {
                    for j in 0..*copies {
                        let (r, c) = (row + i as u32, col + j as u32);
                        grid.insert((r, c), text.clone());
                        templates.insert((r, c), (fragment, *node, *plain));
                        if *rs > 1 || *cs > 1 {
                            merges.push((r, c, r + rs - 1, c + cs - 1));
                        }
                    }
                }
            }
            row += repeat as u32;
            Ok(())
        };
        #[allow(unused_mut)]
        let mut streamed = false;
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(parts) = parts.as_deref_mut() {
            let indices: Vec<_> = parts
                .fragments
                .iter()
                .enumerate()
                .filter(|(_, f)| f.table_start == Some(n.range().start))
                .map(|(i, _)| i)
                .collect();
            for i in indices {
                let plan: RowPlan = serde_json::from_slice(&parts.record(i)?).map_err(bad)?;
                plan_row(plan, Some(i))?;
            }
            streamed = true;
        }
        if !streamed {
            for row in n.descendants().filter(|r| {
                named(*r, "table-row")
                    && r.ancestors()
                        .find(|a| named(*a, "table"))
                        .is_some_and(|a| a == n)
            }) {
                plan_row(RowPlan::from_node(row, text_limit), None)?;
            }
        }
    }
    let mut rich = BTreeMap::new();
    let mut compact = BTreeMap::new();
    {
        let mut build =
            |position: (u32, u32), text: String, node: Option<Node<'_, '_>>| -> Result<(), Error> {
                let source = Source {
                    row: Some(position.0),
                    cell: Some(position.1),
                    ..src.clone()
                };
                let contents = if let Some(node) = node {
                    let mut contents = blocks(node, &source, ctx, depth + 1, styles)?;
                    if contents.is_empty() && !text.is_empty() {
                        contents.push(para(text, &source, ctx)?);
                    }
                    contents
                } else {
                    // The validated plan already contains this paragraph's exact
                    // text. Avoid reparsing its row XML just to copy that text.
                    // Default paragraph styles and normal budgets still apply.
                    ctx.depth(depth + 1)?;
                    let resolved = styles.resolve("", "paragraph", ctx)?;
                    ctx.text(&text)?;
                    let content = if resolved.kind == Some("code") {
                        BlockContent::Code {
                            language: String::new(),
                            text,
                        }
                    } else if let Some(level) = resolved.outline {
                        BlockContent::Heading {
                            level,
                            inlines: vec![resolved.text.text(text)],
                        }
                    } else if resolved.kind == Some("quote") {
                        let block = ctx.block(
                            BlockContent::Paragraph {
                                inlines: vec![resolved.text.text(text)],
                            },
                            source.clone(),
                        )?;
                        BlockContent::Quote {
                            blocks: vec![block],
                        }
                    } else {
                        BlockContent::Paragraph {
                            inlines: vec![resolved.text.text(text)],
                        }
                    };
                    vec![ctx.block(content, source.clone())?]
                };
                if output.is_some() {
                    compact.insert(position, super::PendingBlocks::new(contents, position, src));
                } else {
                    rich.insert(position, contents);
                }
                Ok(())
            };
        let mut entries = grid.into_iter().peekable();
        while let Some((position, _)) = entries.peek() {
            let (fragment, node_id, plain) = templates[position];
            if plain {
                let (position, text) = entries.next().unwrap();
                build(position, text, None)?;
                continue;
            }
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(index) = fragment {
                let source = parts.as_deref_mut().unwrap().get(index)?;
                let doc = Document::parse(&source).map_err(bad)?;
                while entries
                    .peek()
                    .is_some_and(|(position, _)| templates[position].0 == Some(index))
                {
                    let (position, text) = entries.next().unwrap();
                    let (_, node_id, plain) = templates[&position];
                    let node = if plain {
                        None
                    } else {
                        Some(
                            doc.get_node(node_id)
                                .ok_or_else(|| bad("missing row fragment cell"))?,
                        )
                    };
                    build(position, text, node)?;
                }
                continue;
            }
            let _ = fragment;
            let (position, text) = entries.next().unwrap();
            build(
                position,
                text,
                Some(
                    n.document()
                        .get_node(node_id)
                        .ok_or_else(|| bad("missing table cell"))?,
                ),
            )?;
        }
    }
    drop(templates);
    if output.is_some() {
        super::structured_table_output_with(
            compact,
            merges,
            src,
            ctx,
            header_rows,
            TableOutput {
                output,
                prefix: None,
            },
            |_, _| Ok(()),
        )
    } else {
        structured_table_output(rich, merges, src, ctx, header_rows, output, None)
    }
}
fn shape_bounds(n: Node<'_, '_>) -> Option<super::layout::Rect> {
    if attr(n, "transform").is_some() {
        return None;
    }
    fn measure(s: &str) -> Option<f64> {
        let end = s
            .find(|c: char| !c.is_ascii_digit() && !matches!(c, '.' | '-' | '+'))
            .unwrap_or(s.len());
        let value = s[..end].parse::<f64>().ok()?;
        let scale = match &s[end..] {
            "cm" => 360000.,
            "mm" => 36000.,
            "in" => 914400.,
            "pt" => 12700.,
            "pc" => 152400.,
            "px" => 9525.,
            "" => 1.,
            _ => return None,
        };
        Some(value * scale)
    }
    super::layout::Rect::new(
        measure(attr(n, "x")?)?,
        measure(attr(n, "y")?)?,
        measure(attr(n, "width")?)?,
        measure(attr(n, "height")?)?,
    )
}
fn blocks(
    n: Node<'_, '_>,
    src: &Source,
    ctx: &mut Context<'_>,
    depth: usize,
    styles: &mut Styles,
) -> Result<Vec<Block>, Error> {
    ctx.depth(depth)?;
    let children: Vec<_> = n.children().filter(Node::is_element).collect();
    // Most cell bodies contain one paragraph. Preserve room for additional
    // blocks through ordinary growth instead of reserving four in every cell.
    let mut out = Vec::with_capacity(children.len().min(1));
    let order = if src.page.is_some()
        && matches!(n.tag_name().name(), "page" | "g")
        && attr(n, "transform").is_none()
    {
        super::layout::order(
            &children
                .iter()
                .map(|n| shape_bounds(*n))
                .collect::<Vec<_>>(),
        )
    } else {
        (0..children.len()).collect()
    };
    for i in order {
        let c = children[i];
        match c.tag_name().name() {
            "p" | "h" => {
                let resolved =
                    styles.resolve(attr(c, "style-name").unwrap_or(""), "paragraph", ctx)?;
                let spans = inline(c, ctx, styles, TextStyle::default())?;
                if !spans.is_empty() {
                    let b = if resolved.kind == Some("code") {
                        let text = plain(c, ctx.options.limits.max_text_bytes)?;
                        BlockContent::Code {
                            language: String::new(),
                            text,
                        }
                    } else if named(c, "h") || resolved.outline.is_some() {
                        BlockContent::Heading {
                            level: if named(c, "h") {
                                count(attr(c, "outline-level"), 1)?.clamp(1, 6) as u8
                            } else {
                                resolved.outline.unwrap()
                            },
                            inlines: spans,
                        }
                    } else if resolved.kind == Some("quote") {
                        let p =
                            ctx.block(BlockContent::Paragraph { inlines: spans }, src.clone())?;
                        BlockContent::Quote { blocks: vec![p] }
                    } else {
                        BlockContent::Paragraph { inlines: spans }
                    };
                    out.push(ctx.block(b, src.clone())?);
                }
            }
            "list" => {
                let style = c
                    .ancestors()
                    .filter(|n| named(*n, "list"))
                    .find_map(|n| attr(n, "style-name"))
                    .unwrap_or("")
                    .to_owned();
                let level = c.ancestors().filter(|n| named(*n, "list")).count();
                let spec = styles.lists.get(&(style.clone(), level)).cloned();
                let (kind, initial) = spec.clone().unwrap_or((String::new(), 1));
                if spec.is_none() {
                    warn(
                        ctx,
                        "odf_list_numbering",
                        "List style is missing; items retained as bullets.",
                        src,
                    );
                }
                let mut number = attr(c, "continue-list")
                    .and_then(|id| styles.ends.get(id).copied())
                    .or_else(|| {
                        if attr(c, "continue-numbering") == Some("true") {
                            styles.last.get(&(style.clone(), level)).copied()
                        } else {
                            None
                        }
                    })
                    .unwrap_or(initial);
                let mut items = vec![];
                let mut first = number;
                for item in c.children().filter(Node::is_element) {
                    if named(item, "list-header") {
                        if !items.is_empty() {
                            out.push(ctx.block(
                                BlockContent::List {
                                    start: if kind.is_empty() { None } else { Some(first) },
                                    items: std::mem::take(&mut items),
                                },
                                src.clone(),
                            )?);
                        }
                        out.extend(blocks(item, src, ctx, depth + 1, styles)?);
                        continue;
                    }
                    if !named(item, "list-item") {
                        continue;
                    }
                    let restart = attr(item, "start-value")
                        .map(|s| s.parse::<u64>().map_err(bad))
                        .transpose()?;
                    if let Some(n) = restart {
                        if !items.is_empty() {
                            out.push(ctx.block(
                                BlockContent::List {
                                    start: if kind.is_empty() { None } else { Some(first) },
                                    items: std::mem::take(&mut items),
                                },
                                src.clone(),
                            )?);
                        }
                        number = n;
                    }
                    if items.is_empty() {
                        first = number;
                    }
                    let mut contents = blocks(item, src, ctx, depth + 1, styles)?;
                    if !kind.is_empty() && kind != "1" {
                        let marker =
                            format!("{}. ", label(number.min(i64::MAX as u64) as i64, &kind));
                        ctx.text(&marker)?;
                        if let Some(Block {
                            content: BlockContent::Paragraph { inlines },
                            ..
                        }) = contents.first_mut()
                        {
                            inlines.insert(0, Inline::text(marker))
                        } else {
                            contents.insert(0, para(marker, src, ctx)?)
                        }
                        out.extend(contents);
                    } else {
                        items.push(contents)
                    }
                    number = number.saturating_add(1);
                }
                if !items.is_empty() {
                    out.push(ctx.block(
                        BlockContent::List {
                            start: if kind.is_empty() { None } else { Some(first) },
                            items,
                        },
                        src.clone(),
                    )?);
                }
                if let Some(id) = attr(c, "id") {
                    styles.ends.insert(id.into(), number);
                }
                styles.last.insert((style, level), number);
            }
            "table" => {
                if let Some(t) = odf_table(c, src, ctx, depth + 1, styles)? {
                    out.push(t)
                }
            }
            "math" => {
                let equation = super::math::inline(c, ctx, src)?;
                out.push(ctx.block(
                    BlockContent::Paragraph {
                        inlines: vec![equation],
                    },
                    src.clone(),
                )?);
            }
            "object" if attr(c, "href").is_some_and(|h| styles.equations.contains_key(h)) => {
                let equation = styles.equations[attr(c, "href").unwrap()].clone();
                if let Inline::Math { tex, .. } = &equation {
                    ctx.text(tex)?;
                }
                out.push(ctx.block(
                    BlockContent::Paragraph {
                        inlines: vec![equation],
                    },
                    src.clone(),
                )?);
            }
            "image" if attr(c, "href").and_then(|p| ctx.image_inline(p, "")).is_some() => {
                let image = ctx.image_inline(attr(c, "href").unwrap(), "").unwrap();
                out.push(ctx.block(BlockContent::Paragraph { inlines: vec![image] }, src.clone())?);
            }
            "image" | "object" | "object-ole" => {
                warn(ctx, "odf_object", "Embedded image/object placement is not reconstructed; supported package image files are retained separately when enabled.", src)
            }
            "tracked-changes" => warn(
                ctx,
                "odf_revisions",
                "Tracked-change history omitted; current body retained.",
                src,
            ),
            "section" | "text-box" | "frame" | "g" | "custom-shape" | "notes" => {
                out.extend(blocks(c, src, ctx, depth + 1, styles)?)
            }
            "soft-page-break" | "sequence-decls" | "variable-decls" | "forms" => {}
            _ => {
                if c.children().any(|n| n.is_element()) {
                    out.extend(blocks(c, src, ctx, depth + 1, styles)?);
                } else if !content(c).trim().is_empty() {
                    out.push(para(content(c), src, ctx)?);
                    warn(
                        ctx,
                        "odf_unmapped",
                        "Text from unsupported element retained without its semantics.",
                        src,
                    );
                }
            }
        }
    }
    if out.is_empty() {
        return Ok(Vec::new());
    }
    Ok(out)
}
pub fn parse(format: Format, bytes: &[u8], ctx: &mut Context<'_>) -> Result<Vec<Block>, Error> {
    parse_output(format, bytes, ctx, None)
}
pub(super) fn parse_output(
    format: Format,
    bytes: &[u8],
    ctx: &mut Context<'_>,
    mut output: Option<&mut crate::render::Writer<'_>>,
) -> Result<Vec<Block>, Error> {
    let mut pkg = Package::new(bytes, ctx)?;
    if let Some(s) = pkg.read_xml("META-INF/manifest.xml", ctx)? {
        let d = xml(&s, ctx)?;
        if d.descendants().any(|n| named(n, "encryption-data")) {
            return Err(Error::Encrypted);
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    let mut row_parts = if format == Format::Ods {
        super::xml_parts::partition_mapped(
            &mut pkg,
            "content.xml",
            super::xml_parts::Kind::OdfRows,
            ctx,
            Some(&mut |row| {
                serde_json::to_vec(&RowPlan::from_node(row, ctx.options.limits.max_text_bytes))
                    .map_err(bad)
            }),
        )?
    } else {
        None
    };
    #[cfg(not(target_arch = "wasm32"))]
    let s = if let Some(parts) = &mut row_parts {
        parts.account(ctx)?;
        std::mem::take(&mut parts.projection)
    } else {
        pkg.required_xml("content.xml", ctx)?
    };
    #[cfg(target_arch = "wasm32")]
    let s = pkg.required_xml("content.xml", ctx)?;
    let d = xml(&s, ctx)?;
    let mut styles = Styles::default();
    if let Some(s) = pkg.read_xml("styles.xml", ctx)? {
        styles.load(&xml(&s, ctx)?)?;
    }
    styles.load(&d)?;
    for node in d.descendants().filter(|n| named(*n, "object")) {
        if let Some(href) = attr(node, "href").filter(|h| !h.contains(':') && !h.starts_with('/')) {
            if styles.equations.contains_key(href) {
                continue;
            }
            let part = format!(
                "{}/content.xml",
                resolve("content.xml", href)?.trim_end_matches('/')
            );
            if let Some(s) = pkg.read_xml(&part, ctx)? {
                let object = xml(&s, ctx)?;
                if let Some(math) = object.descendants().find(|n| named(*n, "math")) {
                    let src = Source {
                        part: Some(part.into()),
                        ..Default::default()
                    };
                    styles
                        .equations
                        .insert(href.into(), super::math::inline(math, ctx, &src)?);
                }
            }
        }
    }
    let src = Source {
        part: Some("content.xml".into()),
        ..Default::default()
    };
    let kind = match format {
        Format::Odt => "text",
        Format::Ods => "spreadsheet",
        _ => "presentation",
    };
    let body = d
        .descendants()
        .find(|n| named(*n, "body"))
        .and_then(|n| child(n, kind))
        .ok_or_else(|| bad("OpenDocument body does not match format"))?;
    let mut out = vec![];
    if format == Format::Ods {
        for (i, sheet) in body.children().filter(|n| named(*n, "table")).enumerate() {
            if i >= ctx.options.limits.max_pages {
                return Err(Error::Limit("max_pages"));
            }
            super::emit(
                &mut out,
                heading(attr(sheet, "name").unwrap_or("Sheet").into(), 2, &src, ctx)?,
                &mut output,
            )?;
            #[cfg(not(target_arch = "wasm32"))]
            let table = odf_table_rows(
                sheet,
                &src,
                ctx,
                0,
                &mut styles,
                row_parts.as_mut(),
                output.as_deref_mut(),
            )?;
            #[cfg(target_arch = "wasm32")]
            let table = odf_table(sheet, &src, ctx, 0, &mut styles)?;
            if let Some(t) = table {
                super::emit(&mut out, t, &mut output)?;
            }
        }
    } else if format == Format::Odp {
        for (i, page) in body.children().filter(|n| named(*n, "page")).enumerate() {
            if i >= ctx.options.limits.max_pages {
                return Err(Error::Limit("max_pages"));
            }
            let src = Source {
                page: Some(i as u32 + 1),
                ..src.clone()
            };
            out.push(heading(
                attr(page, "name")
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("Slide {}", i + 1)),
                1,
                &src,
                ctx,
            )?);
            out.extend(blocks(page, &src, ctx, 0, &mut styles)?);
        }
    } else {
        out = blocks(body, &src, ctx, 0, &mut styles)?;
    }
    #[allow(unused_mut)]
    let mut fragment_styles = false;
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(parts) = &row_parts {
        fragment_styles = parts.styled_odf;
    }
    if fragment_styles
        || d.descendants()
            .any(|n| named(n, "span") || attr(n, "style-name").is_some())
    {
        warn(
            ctx,
            "odf_styles",
            "Inherited emphasis, code/quote styles and numbering retained; visual ODF styles remain incomplete.",
            &src,
        )
    }
    Ok(out)
}
