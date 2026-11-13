//! Streaming, reduced-fidelity recovery. Never raises archive or output caps.
use super::*;
use crate::formats::bounded::Cells;
use quick_xml::{events::Event, Reader};
use std::io::BufReader;

enum Token {
    Start(String, HashMap<String, String>),
    End(String),
    Text(String),
}
// Return false when a resource or caller's output budget stops the stream.
fn scan(
    pkg: &mut Package<'_>,
    name: &str,
    ctx: &mut Context<'_>,
    mut visit: impl FnMut(Token, usize, &mut Context<'_>) -> Result<bool, Error>,
) -> Result<bool, Error> {
    let cap = pkg.remaining;
    let file = pkg.zip.by_name(name).map_err(bad)?;
    let capped = file.size() > cap as u64;
    let mut reader = Reader::from_reader(BufReader::new(file.take(cap as u64)));
    reader.config_mut().expand_empty_elements = true;
    reader.config_mut().check_comments = true;
    let mut buf = Vec::new();
    let mut depth = 0usize;
    let result = (|| loop {
        ctx.check()?;
        let event = match reader.read_event_into(&mut buf) {
            Ok(e) => e,
            Err(_) if capped && reader.buffer_position() >= cap as u64 => return Ok(false),
            Err(e) => return Err(bad(e)),
        };
        let token = match event {
            Event::Start(e) => {
                depth += 1;
                ctx.depth(depth)?;
                if !ctx.fits(1, 0, 0) {
                    return Ok(false);
                }
                ctx.node()?;
                let name = std::str::from_utf8(e.local_name().as_ref())
                    .map_err(bad)?
                    .to_owned();
                let mut attrs = HashMap::new();
                for a in e.attributes() {
                    let a = a.map_err(bad)?;
                    if a.key.as_ref() == b"xmlns" || a.key.as_ref().starts_with(b"xmlns:") {
                        continue;
                    }
                    let k = std::str::from_utf8(a.key.local_name().as_ref())
                        .map_err(bad)?
                        .to_owned();
                    let v = a
                        .decoded_and_normalized_value(
                            quick_xml::XmlVersion::Implicit1_0,
                            reader.decoder(),
                        )
                        .map_err(bad)?
                        .into_owned();
                    if attrs.insert(k, v).is_some() {
                        return Err(bad("ambiguous attribute in preview"));
                    }
                }
                Some(Token::Start(name, attrs))
            }
            Event::End(e) => Some(Token::End(
                std::str::from_utf8(e.local_name().as_ref())
                    .map_err(bad)?
                    .into(),
            )),
            Event::Text(e) => Some(Token::Text(e.xml10_content().map_err(bad)?.into_owned())),
            Event::CData(e) => Some(Token::Text(e.xml10_content().map_err(bad)?.into_owned())),
            Event::GeneralRef(e) => {
                let entity = format!("&{};", e.decode().map_err(bad)?);
                Some(Token::Text(
                    quick_xml::escape::unescape(&entity)
                        .map_err(bad)?
                        .into_owned(),
                ))
            }
            Event::DocType(_) => return Err(bad("DTD is not allowed in spreadsheet preview")),
            Event::Eof => {
                if depth != 0 {
                    if capped && reader.buffer_position() >= cap as u64 {
                        return Ok(false);
                    }
                    return Err(bad("unclosed XML element in preview"));
                }
                return Ok(true);
            }
            _ => None,
        };
        if let Some(token) = token {
            let end = matches!(token, Token::End(_));
            if !visit(token, depth, ctx)? {
                return Ok(false);
            }
            if end {
                depth = depth.checked_sub(1).ok_or_else(|| bad("unbalanced XML"))?;
            }
        }
        buf.clear();
    })();
    let remaining = reader.into_inner().into_inner().limit() as usize;
    pkg.remaining = pkg.remaining.saturating_sub(cap - remaining);
    result
}
fn append(target: &mut String, text: &str, ctx: &Context<'_>) -> Result<(), Error> {
    if target.len().saturating_add(text.len()) > ctx.options.limits.max_text_bytes {
        return Err(Error::Limit("max_text_bytes"));
    }
    target.push_str(text);
    Ok(())
}
fn xlsx(
    bytes: &[u8],
    ctx: &mut Context<'_>,
    output: Option<&mut crate::render::Writer<'_>>,
) -> Result<Vec<Block>, Error> {
    let mut pkg = Package::new(bytes, ctx)?;
    let part = pkg.main("xl/workbook.xml", ctx)?;
    let source = pkg.required(&part)?;
    let doc = xml(&source, ctx)?;
    let rels = pkg.rels(&part, ctx)?;
    let mut strings = Vec::new();
    let strings_path = rels
        .values()
        .find(|r| r.kind.ends_with("/sharedStrings") && !r.external)
        .map(|r| resolve(&part, &r.target))
        .transpose()?
        .unwrap_or(resolve(&part, "sharedStrings.xml")?);
    if pkg.has(&strings_path) {
        let path = strings_path;
        let mut current = None;
        let mut in_text = false;
        // Reserve most of the node budget for useful cell output. No giant SST DOM.
        let reserve = ctx.options.limits.max_nodes / 2;
        let mut phonetic = false;
        scan(&mut pkg, &path, ctx, |t, _, ctx| {
            match t {
                Token::Start(n, _) if n == "si" => {
                    if !ctx.fits(reserve, 0, 0) {
                        return Ok(false);
                    }
                    current = Some(String::new());
                }
                Token::Start(n, _) if n == "rPh" => phonetic = true,
                Token::End(n) if n == "rPh" => phonetic = false,
                Token::Start(n, _) if n == "t" => in_text = !phonetic,
                Token::End(n) if n == "t" => in_text = false,
                Token::End(n) if n == "si" => {
                    if let Some(s) = current.take() {
                        ctx.text(&s)?;
                        strings.push(s);
                    }
                }
                Token::Text(s) if in_text => {
                    if let Some(c) = &mut current {
                        append(c, &s, ctx)?;
                    }
                }
                _ => {}
            };
            Ok(true)
        })?;
    }
    let mut out = Cells::new(output);
    for (i, sheet) in doc.descendants().filter(|n| named(*n, "sheet")).enumerate() {
        if i >= ctx.options.limits.max_pages {
            break;
        }
        if matches!(sheet.attribute("state"), Some("hidden" | "veryHidden")) {
            continue;
        }
        let path = linked(
            &rels,
            attr(sheet, "id").ok_or_else(|| bad("sheet relationship"))?,
            &part,
        )?;
        let mut row = 0u32;
        let mut col = 0u32;
        let mut cell = None;
        let mut text = String::new();
        let mut kind = String::new();
        let mut capture = false;
        let mut formula = String::new();
        let mut in_formula = false;
        let mut hidden = false;
        let mut hidden_cols = HashSet::new();
        let mut stack = Vec::new();
        let complete = scan(&mut pkg, &path, ctx, |t, depth, ctx| {
            match t {
                Token::Start(n, a) => {
                    if depth == 1 && n != "worksheet" && n != "chartsheet" {
                        return Err(bad("invalid worksheet root"));
                    }
                    if n == "col"
                        && a.get("hidden")
                            .is_some_and(|v| matches!(v.as_str(), "1" | "true"))
                    {
                        let lo = count(a.get("min").map(String::as_str), 1)?;
                        let hi = count(a.get("max").map(String::as_str), lo)?;
                        if hi > 16384 || lo > hi {
                            return Err(bad("invalid hidden column range"));
                        }
                        hidden_cols.extend((lo..=hi).map(|n| n as u32));
                    }
                    if n == "row" && stack.last().is_some_and(|s| s == "sheetData") {
                        row = count(a.get("r").map(String::as_str), row as usize + 1)?
                            .try_into()
                            .map_err(bad)?;
                        if row == 0 || row > 1_048_576 {
                            return Err(bad("row outside worksheet"));
                        }
                        col = 0;
                        hidden = a
                            .get("hidden")
                            .is_some_and(|v| matches!(v.as_str(), "1" | "true"));
                    }
                    if n == "c" && stack.last().is_some_and(|s| s == "row") {
                        let key = a
                            .get("r")
                            .map(|s| xlsx::address(s))
                            .transpose()?
                            .unwrap_or((row, col + 1));
                        if key.0 == 0 || key.0 > 1_048_576 || key.1 == 0 || key.1 > 16384 {
                            return Err(bad("cell outside worksheet"));
                        }
                        col = key.1;
                        cell = Some(key);
                        text.clear();
                        formula.clear();
                        kind = a.get("t").cloned().unwrap_or_default();
                    }
                    if cell.is_some()
                        && (n == "v"
                            || (n == "t"
                                && kind == "inlineStr"
                                && !stack.iter().any(|s| s == "rPh")))
                    {
                        capture = true;
                    }
                    if cell.is_some() && n == "f" {
                        in_formula = true;
                    }
                    stack.push(n);
                }
                Token::Text(s) => {
                    if capture {
                        append(&mut text, &s, ctx)?;
                    }
                    if in_formula {
                        append(&mut formula, &s, ctx)?;
                    }
                }
                Token::End(n) => {
                    if n == "v" || n == "t" {
                        capture = false;
                    }
                    if n == "f" {
                        in_formula = false;
                    }
                    if n == "c" {
                        if let Some((r, c)) = cell.take() {
                            if !hidden && !hidden_cols.contains(&c) {
                                let value = match kind.as_str() {
                                    "s" => {
                                        let j = text.parse::<usize>().map_err(bad)?;
                                        let Some(s) = strings.get(j) else {
                                            warn(ctx,"preview_shared_string_limit","Stopped before a cell whose shared string is outside the retained string prefix.",&Source {part:Some(path.clone().into()),row:Some(r),cell:Some(c),..Default::default()});
                                            return Ok(false);
                                        };
                                        s.clone()
                                    }
                                    "b" => match text.as_str() {
                                        "0" => "FALSE".into(),
                                        "1" => "TRUE".into(),
                                        _ => return Err(bad("invalid boolean")),
                                    },
                                    _ => {
                                        if text.is_empty() && !formula.is_empty() {
                                            format!("={formula}")
                                        } else {
                                            std::mem::take(&mut text)
                                        }
                                    }
                                };
                                if !out.add(&path, r, c, value, ctx)? {
                                    return Ok(false);
                                }
                            }
                        }
                    }
                    stack.pop();
                }
            };
            Ok(true)
        });
        match complete {
            Ok(true) => {}
            Ok(false)
            | Err(Error::Limit(
                "max_nodes" | "max_text_bytes" | "max_table_cells" | "max_expanded_bytes",
            )) => break,
            Err(e) => return Err(e),
        }
    }
    ctx.diagnostic("xlsx_preview_values","Preview retains raw cached cell values; numeric/date formatting is not applied and formulas are not calculated.",Source::default(),true);
    out.finish(ctx)
}

fn ods(
    bytes: &[u8],
    ctx: &mut Context<'_>,
    output: Option<&mut crate::render::Writer<'_>>,
) -> Result<Vec<Block>, Error> {
    let mut pkg = Package::new(bytes, ctx)?;
    if let Some(s) = pkg.read("META-INF/manifest.xml")? {
        if xml(&s, ctx)?
            .descendants()
            .any(|n| named(n, "encryption-data"))
        {
            return Err(Error::Encrypted);
        }
    }
    let mut out = Cells::new(output);
    let mut sheet_depth = None;
    let mut sheet = String::new();
    let mut sheets = 0usize;
    let mut row = 1u32;
    let mut col = 1u32;
    let mut repeat_rows = 1usize;
    let mut repeat_cols = 1usize;
    let mut row_depth = None;
    let mut cell_depth = None;
    let mut text = String::new();
    let mut fallback = String::new();
    let mut in_paragraph = 0usize;
    let mut values: Vec<(u32, usize, String)> = vec![];
    let mut pending_text = 0usize;
    let mut stack: Vec<String> = vec![];
    let result = scan(&mut pkg, "content.xml", ctx, |t, depth, ctx| {
        match t {
            Token::Start(n, a) => {
                if n == "table"
                    && sheet_depth.is_none()
                    && stack.last().is_some_and(|s| s == "spreadsheet")
                {
                    sheets += 1;
                    if sheets > ctx.options.limits.max_pages {
                        return Ok(false);
                    }
                    sheet_depth = Some(depth);
                    sheet = format!(
                        "content.xml/{}",
                        a.get("name").map(String::as_str).unwrap_or("Sheet")
                    );
                    row = 1;
                }
                if n == "table-row" && sheet_depth.is_some() && row_depth.is_none() {
                    row_depth = Some(depth);
                    repeat_rows = count(a.get("number-rows-repeated").map(String::as_str), 1)?;
                    if repeat_rows == 0 || u64::from(row) + repeat_rows as u64 > 1_048_577 {
                        return Err(bad("row repetition outside worksheet"));
                    }
                    col = 1;
                    values.clear();
                    pending_text = 0;
                }
                if matches!(n.as_str(), "table-cell" | "covered-table-cell")
                    && row_depth.is_some()
                    && cell_depth.is_none()
                {
                    cell_depth = Some(depth);
                    repeat_cols = count(a.get("number-columns-repeated").map(String::as_str), 1)?;
                    if repeat_cols == 0 || u64::from(col) + repeat_cols as u64 > 16385 {
                        return Err(bad("column repetition outside worksheet"));
                    }
                    text.clear();
                    fallback = String::new();
                    for key in [
                        "string-value",
                        "date-value",
                        "time-value",
                        "boolean-value",
                        "value",
                        "formula",
                    ] {
                        if let Some(v) = a.get(key) {
                            fallback = v.clone();
                            break;
                        }
                    }
                }
                if cell_depth.is_some() {
                    if n == "p" {
                        in_paragraph += 1;
                    }
                    if in_paragraph > 0 {
                        if n == "s" {
                            let n = count(a.get("c").map(String::as_str), 1)?;
                            if n > ctx.options.limits.max_text_bytes.saturating_sub(text.len()) {
                                return Err(Error::Limit("max_text_bytes"));
                            }
                            text.extend(std::iter::repeat_n(' ', n));
                        }
                        if n == "tab" {
                            append(&mut text, "\t", ctx)?;
                        }
                        if n == "line-break" {
                            append(&mut text, "\n", ctx)?;
                        }
                    }
                }
                stack.push(n);
            }
            Token::Text(s) => {
                if in_paragraph > 0 {
                    append(&mut text, &s, ctx)?;
                }
            }
            Token::End(n) => {
                if n == "p" && in_paragraph > 0 {
                    in_paragraph -= 1;
                    append(&mut text, "\n", ctx)?;
                }
                if cell_depth == Some(depth) {
                    let value = if text.is_empty() {
                        std::mem::take(&mut fallback)
                    } else {
                        text.trim_end_matches('\n').to_owned()
                    };
                    pending_text = pending_text.saturating_add(value.len());
                    if pending_text > ctx.options.limits.max_text_bytes
                        || values.len() >= ctx.options.limits.max_table_cells
                    {
                        return Ok(false);
                    }
                    if !value.is_empty() {
                        values.push((col, repeat_cols, value));
                    }
                    col += repeat_cols as u32;
                    cell_depth = None;
                    in_paragraph = 0;
                }
                if row_depth == Some(depth) {
                    // Empty repeated rows advance provenance without allocation.
                    if !values.is_empty() {
                        for j in 0..repeat_rows {
                            for (c, copies, value) in &values {
                                for k in 0..*copies {
                                    if !out.add(
                                        &sheet,
                                        row + j as u32,
                                        c + k as u32,
                                        value.clone(),
                                        ctx,
                                    )? {
                                        return Ok(false);
                                    }
                                }
                            }
                        }
                    }
                    row += repeat_rows as u32;
                    row_depth = None;
                    values.clear();
                }
                if n == "table" && sheet_depth == Some(depth) {
                    sheet_depth = None;
                }
                stack.pop();
            }
        };
        Ok(true)
    });
    match result {
        Ok(_)
        | Err(Error::Limit(
            "max_nodes" | "max_text_bytes" | "max_table_cells" | "max_expanded_bytes",
        )) => {}
        Err(e) => return Err(e),
    }
    out.finish(ctx)
}
pub(crate) fn parse(
    format: Format,
    bytes: &[u8],
    ctx: &mut Context<'_>,
) -> Result<Vec<Block>, Error> {
    parse_output(format, bytes, ctx, None)
}
pub(crate) fn parse_output(
    format: Format,
    bytes: &[u8],
    ctx: &mut Context<'_>,
    output: Option<&mut crate::render::Writer<'_>>,
) -> Result<Vec<Block>, Error> {
    match format {
        Format::Xlsx => xlsx(bytes, ctx, output),
        Format::Ods => ods(bytes, ctx, output),
        _ => unreachable!(),
    }
}
