use super::*;
#[derive(Clone)]
struct State {
    paragraph: super::style::ParagraphStyle,
    skip: bool,
    label: bool,
    bold: bool,
    italic: bool,
    strike: bool,
    uc: usize,
    encoding: &'static encoding_rs::Encoding,
}
#[derive(Default)]
struct Output {
    paragraph_style: super::style::ParagraphStyle,
    paragraph_started: bool,
    text: Vec<u8>,
    inlines: Vec<Inline>,
    blocks: Vec<Block>,
    row: Vec<Cell>,
    rows: Vec<super::edge_table::Row>,
    properties: Vec<super::edge_table::Properties>,
    pending: super::edge_table::Properties,
    row_left: i64,
    row_header: bool,
    table_mode: bool,
}
impl Output {
    fn flush(&mut self, s: &State, ctx: &mut Context<'_>) -> Result<(), Error> {
        if self.text.is_empty() {
            return Ok(());
        }
        let text = s
            .encoding
            .decode_without_bom_handling(&self.text)
            .0
            .into_owned();
        self.text.clear();
        self.add(text, s, ctx)
    }
    fn add(&mut self, text: String, s: &State, ctx: &mut Context<'_>) -> Result<(), Error> {
        if s.skip {
            return Ok(());
        }
        ctx.text(&text)?;
        // Bound retained inline allocations, independently of how many source
        // formatting controls the lightweight prepass skips.
        ctx.node()?;
        if !self.paragraph_started && !s.label {
            self.paragraph_style = s.paragraph;
            self.paragraph_started = true;
        }
        let mut i = Inline::text(text);
        if s.bold {
            i = Inline::Strong { children: vec![i] }
        }
        if s.italic {
            i = Inline::Emphasis { children: vec![i] }
        }
        if s.strike {
            i = Inline::Strike { children: vec![i] }
        }
        self.inlines.push(i);
        Ok(())
    }
    fn paragraph(&mut self, ctx: &mut Context<'_>) -> Result<(), Error> {
        self.paragraph_started = false;
        if self.inlines.is_empty() {
            return Ok(());
        }
        super::style::paragraph(
            std::mem::take(&mut self.inlines),
            self.paragraph_style,
            &mut self.blocks,
            &Default::default(),
            ctx,
        )?;
        Ok(())
    }
    fn cell(&mut self, ctx: &mut Context<'_>) -> Result<(), Error> {
        self.paragraph_started = false;
        ctx.cell()?;
        let blocks = if self.inlines.is_empty() {
            vec![]
        } else {
            vec![ctx.block(
                BlockContent::Paragraph {
                    inlines: std::mem::take(&mut self.inlines),
                },
                Default::default(),
            )?]
        };
        self.row.push(Cell {
            blocks,
            col_span: 1,
            row_span: 1,
            source: Default::default(),
        });
        Ok(())
    }
    fn end_row(&mut self, ctx: &mut Context<'_>) -> Result<(), Error> {
        let mut cells = vec![];
        let len = self.row.len().max(self.properties.len());
        let mut values = std::mem::take(&mut self.row).into_iter();
        for i in 0..len {
            let cell = values.next().unwrap_or(Cell {
                blocks: vec![],
                row_span: 1,
                col_span: 1,
                source: Default::default(),
            });
            cells.push((cell, self.properties.get(i).copied().unwrap_or_default()));
        }
        if !cells.is_empty() {
            ctx.node()?;
            self.rows.push(super::edge_table::Row {
                left: self.row_left,
                header: self.row_header,
                cells,
            });
        }
        self.table_mode = false;
        Ok(())
    }
    fn table(&mut self, ctx: &mut Context<'_>) -> Result<(), Error> {
        if !self.row.is_empty() {
            self.end_row(ctx)?;
        }
        if let Some(table) =
            super::edge_table::build(std::mem::take(&mut self.rows), &Default::default(), ctx)?
        {
            self.blocks.push(table);
        }
        self.table_mode = false;
        Ok(())
    }
}
pub fn parse(bytes: &[u8], ctx: &mut Context<'_>) -> Result<Vec<Block>, Error> {
    if !bytes.starts_with(b"{\\rtf") {
        return Err(bad("invalid RTF signature"));
    }
    let tree = super::rtf_tree::parse(bytes, ctx)?;
    let styles = super::rtf_tree::styles(&tree, ctx)?;
    let equations = super::rtf_tree::equations(&tree, ctx)?;
    let mut state = State {
        paragraph: Default::default(),
        skip: false,
        label: false,
        bold: false,
        italic: false,
        strike: false,
        uc: 1,
        encoding: encoding_rs::WINDOWS_1252,
    };
    let mut stack = vec![];
    let mut out = Output {
        paragraph_style: Default::default(),
        paragraph_started: false,
        text: vec![],
        inlines: vec![],
        blocks: vec![],
        row: vec![],
        rows: vec![],
        properties: vec![],
        pending: Default::default(),
        row_left: 0,
        row_header: false,
        table_mode: false,
    };
    let mut auxiliary_stack: Vec<(usize, String, Output, usize, Option<u16>)> = Vec::new();
    let mut auxiliary = Vec::new();
    let mut pos = 0;
    let mut fallback = 0usize;
    let mut star = false;
    let mut high = None;
    while pos < bytes.len() {
        ctx.check()?;
        let b = bytes[pos];
        pos += 1;
        if b == b'{' {
            if let Some((end, equation)) = equations.get(&(pos - 1)) {
                out.flush(&state, ctx)?;
                if !state.skip {
                    if out.inlines.is_empty() {
                        out.paragraph_style = state.paragraph;
                    }
                    if let Inline::Math { tex, .. } = equation {
                        ctx.text(tex)?;
                    }
                    ctx.node()?;
                    out.inlines.push(equation.clone());
                }
                pos = *end;
                continue;
            }
            out.flush(&state, ctx)?;
            ctx.depth(stack.len() + 1)?;
            ctx.node()?;
            stack.push(state.clone());
            star = false;
            continue;
        }
        if b == b'}' {
            out.flush(&state, ctx)?;
            if auxiliary_stack
                .last()
                .is_some_and(|(depth, ..)| *depth == stack.len())
            {
                out.table(ctx)?;
                out.paragraph(ctx)?;
                let (_, label, saved, saved_fallback, saved_high) = auxiliary_stack.pop().unwrap();
                let completed = std::mem::replace(&mut out, saved);
                if !completed.blocks.is_empty() {
                    auxiliary.push((label, completed.blocks));
                }
                fallback = saved_fallback;
                high = saved_high;
            }
            state = stack.pop().ok_or_else(|| bad("unbalanced RTF group"))?;
            star = false;
            continue;
        }
        if b != b'\\' {
            if b == b'\r' || b == b'\n' {
                continue;
            }
            if fallback > 0 {
                fallback -= 1;
                continue;
            }
            if !state.skip {
                out.text.push(b);
                if out.text.len() > ctx.options.limits.max_text_bytes {
                    return Err(Error::Limit("max_text_bytes"));
                }
            }
            continue;
        }
        let c = *bytes.get(pos).ok_or_else(|| bad("truncated RTF escape"))?;
        pos += 1;
        if c == b'\'' {
            let pair = bytes
                .get(pos..pos + 2)
                .ok_or_else(|| bad("truncated RTF hex escape"))?;
            let hex = std::str::from_utf8(pair).map_err(bad)?;
            let value = u8::from_str_radix(hex, 16).map_err(bad)?;
            pos += 2;
            if fallback > 0 {
                fallback -= 1
            } else if !state.skip {
                out.text.push(value)
            }
            continue;
        }
        if !c.is_ascii_alphabetic() {
            match c {
                b'\r' | b'\n' => {
                    if c == b'\r' && bytes.get(pos) == Some(&b'\n') {
                        pos += 1;
                    }
                    out.flush(&state, ctx)?;
                    if !state.skip {
                        if out.table_mode {
                            ctx.node()?;
                            out.inlines.push(Inline::Break);
                        } else {
                            out.table(ctx)?;
                            out.paragraph(ctx)?;
                        }
                    }
                }
                b'*' => star = true,
                b'\\' | b'{' | b'}' => {
                    if fallback > 0 {
                        fallback -= 1
                    } else if !state.skip {
                        out.text.push(c)
                    }
                }
                b'~' => {
                    out.flush(&state, ctx)?;
                    if fallback > 0 {
                        fallback -= 1
                    } else {
                        out.add("\u{a0}".into(), &state, ctx)?
                    }
                }
                b'_' => {
                    out.flush(&state, ctx)?;
                    out.add("‑".into(), &state, ctx)?
                }
                b'-' => {}
                _ => {}
            }
            continue;
        }
        let start = pos - 1;
        while bytes.get(pos).is_some_and(u8::is_ascii_alphabetic) {
            pos += 1
        }
        let key = std::str::from_utf8(&bytes[start..pos]).map_err(bad)?;
        let ns = pos;
        if bytes.get(pos) == Some(&b'-') {
            pos += 1
        }
        while bytes.get(pos).is_some_and(u8::is_ascii_digit) {
            pos += 1
        }
        let number = if pos > ns {
            Some(
                std::str::from_utf8(&bytes[ns..pos])
                    .map_err(bad)?
                    .parse::<i64>()
                    .map_err(bad)?,
            )
        } else {
            None
        };
        if bytes.get(pos) == Some(&b' ') {
            pos += 1
        }
        out.flush(&state, ctx)?;
        if key == "bin" {
            let size = number
                .filter(|n| *n >= 0)
                .ok_or_else(|| bad("invalid binary length"))? as usize;
            pos = pos
                .checked_add(size)
                .filter(|end| *end <= bytes.len())
                .ok_or_else(|| bad("truncated RTF binary data"))?;
            warn(
                ctx,
                "rtf_binary",
                "Embedded binary payload omitted.",
                &Default::default(),
            );
            continue;
        }
        if star && key != "ud" {
            state.skip = true
        }
        star = false;
        if !state.skip
            && matches!(
                key,
                "header"
                    | "footer"
                    | "headerl"
                    | "headerr"
                    | "headerf"
                    | "footerl"
                    | "footerr"
                    | "footerf"
            )
        {
            let label = if key.starts_with("header") {
                "Document header"
            } else {
                "Document footer"
            };
            auxiliary_stack.push((
                stack.len(),
                label.into(),
                std::mem::take(&mut out),
                fallback,
                high,
            ));
            fallback = 0;
            high = None;
            continue;
        }
        if matches!(
            key,
            "fonttbl"
                | "colortbl"
                | "stylesheet"
                | "info"
                | "pict"
                | "object"
                | "fldinst"
                | "listtable"
                | "listoverridetable"
                | "generator"
        ) {
            if !state.skip && key == "pict" {
                if let Some(image) = ctx.image_inline(&format!("RTF/pict/{}", start - 1), "") {
                    ctx.node()?;
                    out.inlines.push(image);
                }
            }
            if !state.skip && matches!(key, "pict" | "object") {
                warn(
                    ctx,
                    "rtf_object",
                    "Embedded image/object is not reconstructed in the text flow. Supported original image files are retained separately when enabled; other objects remain unsupported.",
                    &Default::default(),
                );
            }
            state.skip = true;
            continue;
        }
        if state.skip {
            continue;
        }
        match key {
            "listtext" | "pntext" => {
                // Label-local pard/plain controls do not describe the body paragraph.
                if !out.paragraph_started {
                    out.paragraph_style = state.paragraph;
                    out.paragraph_started = true;
                }
                state.label = true;
            }
            "s" => {
                state.paragraph = styles
                    .get(&number.unwrap_or(0))
                    .copied()
                    .unwrap_or_default();
                state.bold = state.paragraph.text.bold.unwrap_or(false);
                state.italic = state.paragraph.text.italic.unwrap_or(false);
                state.strike = state.paragraph.text.strike.unwrap_or(false);
            }
            "pard" => state.paragraph = Default::default(),
            "outlinelevel" => {
                state.paragraph.heading = Some(
                    number
                        .filter(|n| (0..=8).contains(n))
                        .map(|n| n as u8 + 1)
                        .unwrap_or(0),
                )
            }
            "ansicpg" => {
                let label = format!("windows-{}", number.unwrap_or(1252));
                state.encoding = encoding_rs::Encoding::for_label(label.as_bytes())
                    .or(match number {
                        Some(932) => Some(encoding_rs::SHIFT_JIS),
                        Some(936) => Some(encoding_rs::GBK),
                        Some(949) => Some(encoding_rs::EUC_KR),
                        Some(950) => Some(encoding_rs::BIG5),
                        Some(65001) => Some(encoding_rs::UTF_8),
                        _ => None,
                    })
                    .ok_or_else(|| Error::Unsupported("RTF code page".into()))?;
            }
            "uc" => {
                state.uc = number
                    .filter(|n| (0..=16).contains(n))
                    .ok_or_else(|| bad("invalid RTF Unicode fallback length"))?
                    as usize
            }
            "u" => {
                let unit = number
                    .filter(|n| (-32768..=65535).contains(n))
                    .ok_or_else(|| bad("invalid Unicode unit"))? as u16;
                if (0xD800..=0xDBFF).contains(&unit) {
                    high = Some(unit)
                } else if let Some(h) = high.take() {
                    out.add(String::from_utf16_lossy(&[h, unit]), &state, ctx)?
                } else {
                    out.add(String::from_utf16_lossy(&[unit]), &state, ctx)?
                }
                fallback = state.uc;
            }
            "b" => state.bold = number != Some(0),
            "i" => state.italic = number != Some(0),
            "strike" => state.strike = number != Some(0),
            "plain" => {
                state.bold = false;
                state.italic = false;
                state.strike = false;
            }
            "par" => {
                if out.table_mode {
                    ctx.node()?;
                    out.inlines.push(Inline::Break)
                } else {
                    out.table(ctx)?;
                    out.paragraph(ctx)?;
                }
            }
            "line" => {
                ctx.node()?;
                out.inlines.push(Inline::Break)
            }
            "tab" => out.add("\t".into(), &state, ctx)?,
            "trowd" => {
                if !out.inlines.is_empty() {
                    out.table(ctx)?;
                    out.paragraph(ctx)?;
                }
                if !out.row.is_empty() {
                    out.end_row(ctx)?;
                }
                out.properties.clear();
                out.pending = Default::default();
                out.row_left = 0;
                out.row_header = false;
                out.table_mode = true;
            }
            "trleft" => out.row_left = number.unwrap_or(0),
            "trhdr" => out.row_header = number != Some(0),
            "cellx" => {
                out.pending.right = number;
                ctx.node()?;
                out.properties.push(std::mem::take(&mut out.pending));
            }
            "clmgf" => out.pending.horizontal_start = true,
            "clmrg" => out.pending.horizontal_continue = true,
            "clvmgf" => out.pending.vertical_start = true,
            "clvmrg" => out.pending.vertical_continue = true,
            "itap" if number.is_some_and(|n| n > 1) => warn(
                ctx,
                "rtf_nested_table",
                "Nested RTF table hierarchy is not reconstructed.",
                &Default::default(),
            ),
            "intbl" => out.table_mode = true,
            "cell" => out.cell(ctx)?,
            "row" => {
                if !out.inlines.is_empty() {
                    out.cell(ctx)?;
                }
                out.end_row(ctx)?;
            }
            "emdash" => out.add("—".into(), &state, ctx)?,
            "endash" => out.add("–".into(), &state, ctx)?,
            "bullet" => out.add("•".into(), &state, ctx)?,
            "lquote" => out.add("‘".into(), &state, ctx)?,
            "rquote" => out.add("’".into(), &state, ctx)?,
            "ldblquote" => out.add("“".into(), &state, ctx)?,
            "rdblquote" => out.add("”".into(), &state, ctx)?,
            "ls" | "pn" => warn(
                ctx,
                "rtf_list",
                "List text retained; automatic numbering is not reconstructed.",
                &Default::default(),
            ),
            "mmath" | "omath" => warn(
                ctx,
                "rtf_math",
                "Equation source is not reconstructed.",
                &Default::default(),
            ),
            _ => {}
        }
    }
    out.flush(&state, ctx)?;
    if !stack.is_empty() {
        warn(
            ctx,
            "rtf_unbalanced",
            "Recovered text from unclosed RTF groups.",
            &Default::default(),
        );
    }
    if let Some(h) = high {
        out.add(String::from_utf16_lossy(&[h]), &state, ctx)?;
    }
    out.table(ctx)?;
    out.paragraph(ctx)?;
    // Recover unclosed auxiliary groups without losing the saved main story.
    while let Some((_, label, saved, _, _)) = auxiliary_stack.pop() {
        let completed = std::mem::replace(&mut out, saved);
        if !completed.blocks.is_empty() {
            auxiliary.push((label, completed.blocks));
        }
        out.table(ctx)?;
        out.paragraph(ctx)?;
    }
    for (label, blocks) in auxiliary {
        let source = Source {
            part: Some(
                if label == "Document header" {
                    "rtf/header"
                } else {
                    "rtf/footer"
                }
                .into(),
            ),
            ..Default::default()
        };
        ctx.text(&label)?;
        out.blocks.push(ctx.block(
            BlockContent::Heading {
                level: 2,
                inlines: vec![Inline::text(label)],
            },
            source.clone(),
        )?);
        out.blocks.extend(blocks);
        warn(
            ctx,
            "rtf_auxiliary_layout",
            "Header/footer content appended once; page-specific placement is not reconstructed.",
            &source,
        );
    }
    Ok(out.blocks)
}
