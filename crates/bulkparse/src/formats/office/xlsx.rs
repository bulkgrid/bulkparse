use super::style::TextStyle;
use super::*;
use std::borrow::Cow;
#[derive(Default)]
struct CellStyle {
    format: String,
    text: TextStyle,
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct Run {
    text: String,
    style: TextStyle,
}
mod shared_strings;

// Decode a row once while its XML is being validated. The on-disk representation
// retains extraction inputs, so replay does not reparse XML or namespace scopes.
#[derive(serde::Serialize, serde::Deserialize)]
struct RowInput<'a> {
    #[serde(rename = "r")]
    index: Option<Cow<'a, str>>,
    #[serde(rename = "h")]
    hidden: bool,
    #[serde(rename = "c")]
    cells: Vec<CellInput<'a>>,
}
#[derive(serde::Serialize, serde::Deserialize)]
struct CellInput<'a> {
    #[serde(rename = "r", default, skip_serializing_if = "Option::is_none")]
    address: Option<Cow<'a, str>>,
    #[serde(rename = "s", default, skip_serializing_if = "Option::is_none")]
    style: Option<Cow<'a, str>>,
    #[serde(rename = "t", default, skip_serializing_if = "str::is_empty")]
    kind: Cow<'a, str>,
    #[serde(rename = "v", default, skip_serializing_if = "str::is_empty")]
    value: Cow<'a, str>,
    #[serde(rename = "f", default, skip_serializing_if = "Option::is_none")]
    formula: Option<Cow<'a, str>>,
    #[serde(rename = "i", default, skip_serializing_if = "Option::is_none")]
    rich: Option<Vec<Run>>,
}
impl<'a> RowInput<'a> {
    fn from_node(row: Node<'a, '_>) -> Self {
        Self {
            index: row.attribute("r").map(Cow::Borrowed),
            hidden: matches!(row.attribute("hidden"), Some("1" | "true")),
            cells: row
                .children()
                .filter(|n| named(*n, "c"))
                .map(|c| CellInput {
                    address: c.attribute("r").map(Cow::Borrowed),
                    style: c.attribute("s").map(Cow::Borrowed),
                    kind: Cow::Borrowed(c.attribute("t").unwrap_or("")),
                    value: child(c, "v").map(input_text).unwrap_or_default(),
                    formula: child(c, "f").map(input_text),
                    rich: child(c, "is").map(runs),
                })
                .collect(),
        }
    }
}

// Borrow the common single text node while encoding a validated row. Mixed
// content retains the general descendant-text extraction behavior.
fn input_text<'a>(n: Node<'a, '_>) -> Cow<'a, str> {
    if let Some(text) = n
        .first_child()
        .filter(|t| t.is_text() && t.next_sibling().is_none())
    {
        Cow::Borrowed(text.text().unwrap_or(""))
    } else {
        Cow::Owned(content(n))
    }
}

fn font(n: Option<Node<'_, '_>>) -> TextStyle {
    let prop = |key| {
        n.and_then(|n| child(n, key))
            .map(|n| !matches!(n.attribute("val"), Some("0" | "false")))
    };
    TextStyle {
        bold: prop("b"),
        italic: prop("i"),
        strike: prop("strike"),
        ..Default::default()
    }
}
fn runs(n: Node<'_, '_>) -> Vec<Run> {
    n.children()
        .filter_map(|n| {
            if named(n, "t") {
                Some(Run {
                    text: content(n),
                    style: TextStyle::default(),
                })
            } else if named(n, "r") {
                child(n, "t").map(|t| Run {
                    text: content(t),
                    style: font(child(n, "rPr")),
                })
            } else {
                None
            }
        })
        .collect()
}

pub(super) fn address(s: &str) -> Result<(u32, u32), Error> {
    let s = if s.contains('$') {
        Cow::Owned(s.replace('$', ""))
    } else {
        Cow::Borrowed(s)
    };
    let mut col = 0u32;
    let mut split = 0;
    for b in s.bytes() {
        if b.is_ascii_alphabetic() {
            col = col
                .checked_mul(26)
                .and_then(|v| v.checked_add(u32::from(b.to_ascii_uppercase() - b'A' + 1)))
                .ok_or_else(|| bad("cell address overflow"))?;
            split += 1
        } else {
            break;
        }
    }
    let row: u32 = s
        .get(split..)
        .ok_or_else(|| bad("cell address"))?
        .parse()
        .map_err(|_| bad("cell row"))?;
    if !(1..=1_048_576).contains(&row) || !(1..=16_384).contains(&col) {
        return Err(bad("cell outside workbook bounds"));
    }
    Ok((row, col))
}
pub(super) fn range(s: &str) -> Result<(u32, u32, u32, u32), Error> {
    let (a, b) = s.split_once(':').unwrap_or((s, s));
    let (r, c) = address(a)?;
    let (er, ec) = address(b)?;
    if er < r || ec < c {
        return Err(bad("reversed cell range"));
    }
    Ok((r, c, er, ec))
}
fn rich(n: Node<'_, '_>) -> String {
    n.children()
        .filter(Node::is_element)
        .filter_map(|n| {
            if named(n, "t") {
                Some(content(n))
            } else if named(n, "r") {
                child(n, "t").map(content)
            } else {
                None
            }
        })
        .collect()
}
pub(super) fn builtin(id: u32) -> &'static str {
    match id {
        1 => "0",
        2 => "0.00",
        3 => "#,##0",
        4 => "#,##0.00",
        9 => "0%",
        10 => "0.00%",
        11 => "0.00E+00",
        14 => "mm-dd-yy",
        15 => "d-mmm-yy",
        16 => "d-mmm",
        17 => "mmm-yy",
        18 => "h:mm AM/PM",
        19 => "h:mm:ss AM/PM",
        20 => "h:mm",
        21 => "h:mm:ss",
        22 => "m/d/yy h:mm",
        45 => "mm:ss",
        46 => "[h]:mm:ss",
        47 => "mmss.0",
        49 => "@",
        0 => "General",
        _ => "Unsupported",
    }
}
fn styles(
    pkg: &mut Package<'_>,
    part: &str,
    rels: &HashMap<String, Rel>,
    ctx: &mut Context<'_>,
) -> Result<Vec<CellStyle>, Error> {
    let p = rels
        .values()
        .find(|r| r.kind.ends_with("/styles") && !r.external)
        .map(|r| resolve(part, &r.target))
        .transpose()?
        .unwrap_or(resolve(part, "styles.xml")?);
    let Some(s) = pkg.read_xml(&p, ctx)? else {
        return Ok(vec![]);
    };
    let d = xml(&s, ctx)?;
    let custom: HashMap<_, _> = d
        .descendants()
        .filter(|n| named(*n, "numFmt"))
        .filter_map(|n| {
            Some((
                n.attribute("numFmtId")?.parse::<u32>().ok()?,
                n.attribute("formatCode")?.to_owned(),
            ))
        })
        .collect();
    let fonts: Vec<_> = d
        .descendants()
        .find(|n| named(*n, "fonts"))
        .into_iter()
        .flat_map(|n| n.children())
        .filter(|n| named(*n, "font"))
        .map(|n| font(Some(n)))
        .collect();
    Ok(d.descendants()
        .find(|n| named(*n, "cellXfs"))
        .into_iter()
        .flat_map(|n| n.children())
        .filter(|n| named(*n, "xf"))
        .map(|n| {
            let id = n
                .attribute("numFmtId")
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(0);
            CellStyle {
                format: custom
                    .get(&id)
                    .cloned()
                    .unwrap_or_else(|| builtin(id).into()),
                text: fonts
                    .get(
                        n.attribute("fontId")
                            .and_then(|s| s.parse::<usize>().ok())
                            .unwrap_or(0),
                    )
                    .copied()
                    .unwrap_or_default(),
            }
        })
        .collect())
}
// Display common numeric/date formats; preserve raw values with a diagnostic for
// unsupported patterns instead of pretending to execute Excel's format language.
pub(super) fn display(raw: &str, fmt: &str, epoch: bool) -> Option<String> {
    if uses_raw_value(fmt) {
        return Some(raw.into());
    }
    let value: f64 = raw.parse().ok()?;
    if !value.is_finite() {
        return None;
    }
    let mut sections = vec![String::new()];
    let mut quoted = false;
    let mut escaped = false;
    for c in fmt.chars() {
        if c == ';' && !quoted && !escaped {
            sections.push(String::new());
            continue;
        }
        sections.last_mut()?.push(c);
        if c == '"' && !escaped {
            quoted = !quoted
        }
        escaped = c == '\\' && !escaped;
    }
    if quoted || escaped {
        return None;
    }
    let selected = if value < 0.0 && sections.len() > 1 {
        1
    } else if value == 0.0 && sections.len() > 2 {
        2
    } else {
        0
    };
    let s = sections.get(selected)?;
    let mut tokens = vec![];
    let mut chars = s.chars().peekable();
    let mut quote = false;
    while let Some(c) = chars.next() {
        match c {
            '"' => quote = !quote,
            '\\' if !quote => tokens.push((chars.next()?, true)),
            '_' | '*' if !quote => {
                chars.next()?;
            }
            '[' if !quote => {
                let mut tag = String::new();
                let mut closed = false;
                for c in chars.by_ref() {
                    if c == ']' {
                        closed = true;
                        break;
                    }
                    tag.push(c)
                }
                if !closed {
                    return None;
                }
                let lower = tag.to_ascii_lowercase();
                if matches!(lower.as_str(), "h" | "hh" | "m" | "mm" | "s" | "ss") {
                    for c in format!("[{lower}]").chars() {
                        tokens.push((c, false));
                    }
                } else if let Some(currency) = tag.strip_prefix('$') {
                    for c in currency.split('-').next()?.chars() {
                        tokens.push((c, true));
                    }
                } else if !matches!(
                    lower.as_str(),
                    "black" | "blue" | "cyan" | "green" | "magenta" | "red" | "white" | "yellow"
                ) {
                    return None;
                }
            }
            c => tokens.push((c, quote)),
        }
    }
    if quote {
        return None;
    }
    let date = tokens
        .iter()
        .any(|(c, l)| !*l && "ydh".contains(c.to_ascii_lowercase()));
    if date {
        if tokens.iter().any(|(c, l)| *l && !" -/:.,".contains(*c)) {
            return None;
        }
        let pattern: String = tokens.iter().map(|x| x.0).collect();
        return display_simple(raw, &pattern.replace("[hh]", "[h]"), epoch);
    }
    let mut pattern = String::new();
    let mut prefix = String::new();
    let mut suffix = String::new();
    let mut seen = false;
    for (c, literal) in tokens {
        if !literal && matches!(c, '0' | '#' | '?' | '@') {
            seen = true
        }
        if literal || matches!(c, '(' | ')') {
            if seen {
                suffix.push(c)
            } else {
                prefix.push(c)
            }
        } else {
            pattern.push(c)
        }
    }
    if !seen {
        return Some(prefix + &suffix + &pattern);
    }
    let raw = if selected == 1 {
        value.abs().to_string()
    } else {
        raw.into()
    };
    let rendered = display_simple(&raw, &pattern, epoch)?;
    Some(format!("{prefix}{rendered}{suffix}"))
}
fn uses_raw_value(fmt: &str) -> bool {
    matches!(fmt, "General" | "@" | "")
}
fn display_simple(raw: &str, fmt: &str, date1904: bool) -> Option<String> {
    if uses_raw_value(fmt) {
        return Some(raw.into());
    }
    let v: f64 = raw.parse().ok()?;
    if !v.is_finite() {
        return None;
    }
    let f = fmt.to_ascii_lowercase();
    if f.contains(['"', '\\', ';', '_', '*']) {
        return None;
    }
    if f.chars()
        .any(|c| c.is_ascii_alphabetic() && !"ymdhsap".contains(c))
    {
        return None;
    }
    if f.contains('y') || f.contains('d') || f.contains("h:") || f.contains("[h]") || f == "mm:ss" {
        if !(0.0..=2_958_465.0).contains(&v) {
            return None;
        }
        let days = v.floor() as i64;
        let (mut year, mut left) = if date1904 {
            (1904, days)
        } else {
            (1900, days - 1 - if days > 60 { 1 } else { 0 })
        };
        if days == 60 && !date1904 {
            return Some("1900-02-29".into());
        }
        if left < 0 {
            left = 0
        }
        loop {
            let n = if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
                366
            } else {
                365
            };
            if left < n {
                break;
            }
            left -= n;
            year += 1
        }
        let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        let months = [
            31,
            if leap { 29 } else { 28 },
            31,
            30,
            31,
            30,
            31,
            31,
            30,
            31,
            30,
            31,
        ];
        let mut month = 1;
        for n in months {
            if left < n {
                break;
            }
            left -= n;
            month += 1
        }
        let seconds = ((v.fract() * 86400.0).round() as u64) % 86400;
        let hour = seconds / 3600;
        let minute = seconds / 60 % 60;
        let second = seconds % 60;
        if f.contains("[h]") {
            return Some(format!(
                "{}:{minute:02}:{second:02}",
                (v * 24.0).floor() as u64
            ));
        }
        let date = if days == 0 && !date1904 {
            "1899-12-31".into()
        } else {
            format!("{year:04}-{month:02}-{:02}", left + 1)
        };
        let time = if f.contains('s') {
            format!("{hour:02}:{minute:02}:{second:02}")
        } else {
            format!("{hour:02}:{minute:02}")
        };
        return Some(if f.contains('y') || f.contains('d') {
            if f.contains('h') {
                format!("{date} {time}")
            } else {
                date
            }
        } else {
            time
        });
    }
    if f.contains(['?', 'e', '[']) || f.chars().any(|c| !"0#.,%$€£¥ ".contains(c)) {
        return None;
    }
    let decimals = f
        .split_once('.')
        .map(|(_, s)| s.chars().take_while(|c| matches!(c, '0' | '#')).count())
        .unwrap_or(0);
    if decimals > 12 {
        return None;
    }
    let val = if f.contains('%') { v * 100.0 } else { v };
    let mut s = format!("{val:.decimals$}");
    if f.contains(",") {
        let neg = s.starts_with('-');
        let (integer, fraction) = s.split_once('.').unwrap_or((&s, ""));
        let digits = integer.trim_start_matches('-');
        let mut grouped = String::new();
        for (i, c) in digits.chars().enumerate() {
            if i > 0 && (digits.len() - i) % 3 == 0 {
                grouped.push(',')
            }
            grouped.push(c)
        }
        s = format!(
            "{}{grouped}{}",
            if neg { "-" } else { "" },
            if fraction.is_empty() {
                String::new()
            } else {
                format!(".{fraction}")
            }
        );
    }
    for currency in ['$', '€', '£', '¥'] {
        if f.contains(currency) {
            s = format!("{currency}{s}");
            break;
        }
    }
    if f.contains('%') {
        s.push('%')
    }
    Some(s)
}
pub fn parse(bytes: &[u8], ctx: &mut Context<'_>) -> Result<Vec<Block>, Error> {
    parse_output(bytes, ctx, None)
}
pub(super) fn parse_output(
    bytes: &[u8],
    ctx: &mut Context<'_>,
    mut output: Option<&mut crate::render::Writer<'_>>,
) -> Result<Vec<Block>, Error> {
    let mut pkg = Package::new(bytes, ctx)?;
    let part = pkg.main("xl/workbook.xml", ctx)?;
    let s = pkg.required_xml(&part, ctx)?;
    let d = xml(&s, ctx)?;
    if !named(d.root_element(), "workbook") {
        return Err(bad("main part is not a workbook"));
    }
    let rels = pkg.rels(&part, ctx)?;
    let formats = styles(&mut pkg, &part, &rels, ctx)?;
    let sp = rels
        .values()
        .find(|r| r.kind.ends_with("/sharedStrings") && !r.external)
        .map(|r| resolve(&part, &r.target))
        .transpose()?
        .unwrap_or(resolve(&part, "sharedStrings.xml")?);
    let mut strings = shared_strings::SharedStrings::load(&mut pkg, &sp, ctx)?;
    let epoch = d
        .descendants()
        .find(|n| named(*n, "workbookPr"))
        .and_then(|n| n.attribute("date1904"))
        .is_some_and(|v| v == "1" || v == "true");
    let mut out = vec![];
    let mut total_text = 0usize;
    for (index, sheet) in d.descendants().filter(|n| named(*n, "sheet")).enumerate() {
        ctx.check()?;
        if index >= ctx.options.limits.max_pages {
            return Err(Error::Limit("max_pages"));
        }
        let path = linked(
            &rels,
            attr(sheet, "id").ok_or_else(|| bad("sheet has no relationship"))?,
            &part,
        )?;
        let src = Source {
            part: Some(path.clone().into()),
            ..Default::default()
        };
        if matches!(sheet.attribute("state"), Some("hidden" | "veryHidden")) {
            ctx.diagnostic(
                "office_hidden_content",
                "Hidden sheet omitted by visible-content policy.",
                src,
                false,
            );
            continue;
        }
        #[cfg(not(target_arch = "wasm32"))]
        let mut row_parts = super::xml_parts::partition_mapped(
            &mut pkg,
            &path,
            super::xml_parts::Kind::Rows,
            ctx,
            Some(&mut |node| {
                let rows: Vec<_> = node
                    .descendants()
                    .filter(|n| named(*n, "row"))
                    .map(RowInput::from_node)
                    .collect();
                serde_json::to_vec(&rows).map_err(bad)
            }),
        )?;
        #[cfg(not(target_arch = "wasm32"))]
        let source = if let Some(parts) = &mut row_parts {
            parts.account(ctx)?;
            std::mem::take(&mut parts.projection)
        } else {
            pkg.required_xml(&path, ctx)?
        };
        #[cfg(target_arch = "wasm32")]
        let source = pkg.required_xml(&path, ctx)?;
        let doc = xml(&source, ctx)?;
        if named(doc.root_element(), "chartsheet") {
            super::emit(
                &mut out,
                heading(
                    sheet.attribute("name").unwrap_or("Chart").into(),
                    2,
                    &src,
                    ctx,
                )?,
                &mut output,
            )?;
            for block in extras(&mut pkg, &path, &src, ctx)? {
                super::emit(&mut out, block, &mut output)?;
            }
            continue;
        }
        if !named(doc.root_element(), "worksheet") {
            return Err(bad("sheet part is not a worksheet"));
        }
        super::emit(
            &mut out,
            heading(
                sheet.attribute("name").unwrap_or("Sheet").into(),
                2,
                &src,
                ctx,
            )?,
            &mut output,
        )?;
        let mut grid = BTreeMap::new();
        let mut decorated = HashMap::new();
        let mut row_index = 0u32;
        let mut hidden_cols = HashSet::new();
        for n in doc
            .descendants()
            .filter(|n| named(*n, "col") && matches!(n.attribute("hidden"), Some("1" | "true")))
        {
            let lo = count(n.attribute("min"), 1)?;
            let hi = count(n.attribute("max"), lo)?;
            if hi > 16384 || lo > hi {
                return Err(bad("invalid column bounds"));
            }
            for c in lo..=hi {
                hidden_cols.insert(c as u32);
            }
        }
        let mut hidden_rows = HashSet::new();
        {
            let mut process_row = |row: RowInput| -> Result<(), Error> {
                ctx.check()?;
                row_index = count(row.index.as_deref(), row_index as usize + 1)?
                    .try_into()
                    .map_err(|_| bad("row overflow"))?;
                if row_index == 0 || row_index > 1_048_576 {
                    return Err(bad("row outside bounds"));
                }
                if row.hidden {
                    hidden_rows.insert(row_index);
                    return Ok(());
                }
                let mut column = 0u32;
                for c in row.cells {
                    let (r, col) = c
                        .address
                        .as_deref()
                        .map(address)
                        .transpose()?
                        .unwrap_or((row_index, column + 1));
                    if r == 0 || r > 1_048_576 || col == 0 || col > 16_384 {
                        return Err(bad("cell outside worksheet bounds"));
                    }
                    column = col;
                    if hidden_cols.contains(&col) {
                        continue;
                    }
                    if grid.len() >= ctx.options.limits.max_table_cells {
                        return Err(Error::Limit("max_table_cells"));
                    }
                    let style = count(c.style.as_deref(), 0)?;
                    let cellstyle = formats.get(style).map(|s| s.text).unwrap_or_default();
                    let value = c.value.into_owned();
                    let kind = c.kind.as_ref();
                    let shared = if kind == "s" {
                        Some(
                            strings.get(
                                value
                                    .parse::<usize>()
                                    .map_err(|_| bad("shared string index"))?,
                            )?,
                        )
                    } else {
                        None
                    };
                    let text = match kind {
                        "s" => shared
                            .as_ref()
                            .unwrap()
                            .iter()
                            .map(|r| r.text.as_str())
                            .collect::<String>(),
                        "inlineStr" => c
                            .rich
                            .as_ref()
                            .map(|runs| runs.iter().map(|r| r.text.as_str()).collect::<String>())
                            .unwrap_or_default(),
                        "b" => {
                            if value == "1" {
                                "TRUE".into()
                            } else if value == "0" {
                                "FALSE".into()
                            } else {
                                return Err(bad("invalid boolean cell"));
                            }
                        }
                        "e" => {
                            warn(
                                ctx,
                                "xlsx_error_cell",
                                "Workbook contains an error-valued cell.",
                                &src,
                            );
                            value
                        }
                        "str" | "d" => value,
                        _ => {
                            let fmt = formats
                                .get(style)
                                .map(|s| s.format.as_str())
                                .unwrap_or("General");
                            // These formats preserve the raw value exactly;
                            // move its existing buffer instead of copying it.
                            if value.is_empty() || uses_raw_value(fmt) {
                                value
                            } else if let Some(text) = display(&value, fmt, epoch) {
                                if fmt.contains(['y', 'd', 'h']) {
                                    ctx.diagnostic(
                                        "xlsx_date_normalized",
                                        "Date/time displayed in normalized ISO form.",
                                        src.clone(),
                                        false,
                                    );
                                }
                                text
                            } else {
                                warn(
                                    ctx,
                                    "xlsx_number_format",
                                    "Unsupported number format; numeric value retained.",
                                    &src,
                                );
                                value
                            }
                        }
                    };
                    let text = if let Some(f) = c.formula {
                        ctx.diagnostic(
                        "xlsx_formula_cache",
                        "Formula results are cached source values; formulas are never evaluated.",
                        src.clone(),
                        false,
                    );
                        if text.is_empty() {
                            warn(
                                ctx,
                                "xlsx_formula_missing_cache",
                                "Formula has no cached result; expression retained.",
                                &src,
                            );
                            format!("={f}")
                        } else {
                            text
                        }
                    } else {
                        text
                    };
                    total_text = total_text.saturating_add(text.len());
                    if total_text > ctx.options.limits.max_text_bytes {
                        return Err(Error::Limit("max_text_bytes"));
                    }
                    if !text.is_empty() {
                        let rich = match kind {
                            "s" => shared,
                            "inlineStr" => c.rich.map(std::borrow::Cow::Owned),
                            _ => None,
                        };
                        // The base table already creates this exact plain inline.
                        // Keep decorations only when they change the structured
                        // result, rather than retaining a second copy per cell.
                        let plain = match rich.as_deref() {
                            None => cellstyle.is_plain_text(),
                            Some([run]) => {
                                run.text == text && cellstyle.merge(run.style).is_plain_text()
                            }
                            _ => false,
                        };
                        let inlines = if plain {
                            None
                        } else if let Some(rich) = rich {
                            Some(
                                rich.into_owned()
                                    .into_iter()
                                    .map(|r| cellstyle.merge(r.style).text(r.text))
                                    .collect(),
                            )
                        } else {
                            Some(vec![cellstyle.text(text.clone())])
                        };
                        if let Some(inlines) = inlines {
                            decorated.insert((r, col), inlines);
                        }
                    }
                    if !text.is_empty() && grid.insert((r, col), text).is_some() {
                        return Err(bad("duplicate worksheet cell"));
                    }
                }
                Ok(())
            };
            #[allow(unused_mut)]
            let mut streamed = false;
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(parts) = &mut row_parts {
                for i in 0..parts.record_count() {
                    let rows: Vec<RowInput> =
                        serde_json::from_slice(&parts.record(i)?).map_err(bad)?;
                    for row in rows {
                        process_row(row)?;
                    }
                }
                streamed = true;
            }
            if !streamed {
                for row in doc.descendants().filter(|n| named(*n, "row")) {
                    process_row(RowInput::from_node(row))?;
                }
            }
        }
        let mut merges = vec![];
        for n in doc.descendants().filter(|n| named(*n, "mergeCell")) {
            if let Some(s) = n.attribute("ref") {
                merges.push(range(s)?);
            }
        }
        // Preserve original cell coordinates. Hidden cells are not extracted;
        // blank placeholders keep the grid and source references aligned.
        if !hidden_cols.is_empty() || !hidden_rows.is_empty() {
            ctx.diagnostic(
                "office_hidden_content",
                "Hidden worksheet values omitted; original cell coordinates retained.",
                src.clone(),
                false,
            );
        }
        let sheet_rels = pkg.rels(&path, ctx)?;
        let mut hyperlinks = vec![];
        for link in doc.descendants().filter(|n| named(*n, "hyperlink")) {
            let Some(reference) = link.attribute("ref") else {
                continue;
            };
            let rect = range(reference)?;
            let url = if let Some(id) = attr(link, "id") {
                sheet_rels.get(id).map(|r| r.target.clone())
            } else {
                link.attribute("location").map(|s| format!("#{s}"))
            };
            if let Some(url) = url {
                hyperlinks.push((rect, url))
            } else {
                warn(
                    ctx,
                    "xlsx_hyperlink_missing",
                    "Hyperlink target is missing.",
                    &src,
                );
            }
        }
        let table = table_output_with(
            grid,
            merges,
            &src,
            ctx,
            output.as_deref_mut(),
            None,
            |row, ctx| {
                for cell in row {
                    for block in &mut cell.blocks {
                        let (r, c) = (block.source.row.unwrap(), block.source.cell.unwrap());
                        if let Block {
                            content: BlockContent::Paragraph { inlines },
                            ..
                        } = block
                        {
                            if let Some(runs) = decorated.remove(&(r, c)) {
                                *inlines = runs;
                            }
                            for ((r1, c1, r2, c2), url) in &hyperlinks {
                                if r >= *r1 && r <= *r2 && c >= *c1 && c <= *c2 {
                                    ctx.text(url)?;
                                    *inlines = vec![Inline::Link {
                                        url: url.clone(),
                                        children: std::mem::take(inlines),
                                    }];
                                    break;
                                }
                            }
                        }
                    }
                }
                Ok(())
            },
        )?;
        if let Some(b) = table {
            super::emit(&mut out, b, &mut output)?;
        }
        for block in extras(&mut pkg, &path, &src, ctx)? {
            super::emit(&mut out, block, &mut output)?;
        }
        #[allow(unused_mut)]
        let mut fragment_formatting = false;
        #[allow(unused_mut)]
        let mut fragment_visual = false;
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(parts) = &row_parts {
            fragment_formatting = parts.formatting;
            fragment_visual = parts.visual;
        }
        if doc.descendants().any(|n| {
            matches!(n.tag_name().name(), "hyperlinks" | "conditionalFormatting") || named(n, "r")
        }) || !formats.is_empty()
            || fragment_formatting
        {
            warn(ctx,"xlsx_formatting","Cell values, emphasis and hyperlinks retained; visual styles and conditional display rules remain incomplete.",&src);
        }
        if doc.descendants().any(|n| {
            matches!(
                n.tag_name().name(),
                "drawing" | "legacyDrawing" | "oleObjects" | "extLst"
            )
        }) || fragment_visual
        {
            warn(
                ctx,
                "xlsx_visual_content",
                "Cached chart values, drawing descriptions and comments extracted where present; image pixels, drawing geometry and extensions remain incomplete.",
                &src,
            );
        }
    }
    Ok(out)
}

fn extras(
    pkg: &mut Package<'_>,
    part: &str,
    src: &Source,
    ctx: &mut Context<'_>,
) -> Result<Vec<Block>, Error> {
    let rels = pkg.rels(part, ctx)?;
    let mut out = vec![];
    let mut related: Vec<_> = rels
        .values()
        .filter(|r| !r.external && (r.kind.ends_with("/comments") || r.kind.ends_with("/drawing")))
        .collect();
    related.sort_by_key(|r| &r.target);
    for rel in related {
        let path = resolve(part, &rel.target)?;
        let s = pkg.required_xml(&path, ctx)?;
        let d = xml(&s, ctx)?;
        if rel.kind.ends_with("/comments") {
            let authors: Vec<_> = d
                .descendants()
                .filter(|n| named(*n, "author"))
                .map(content)
                .collect();
            for comment in d.descendants().filter(|n| named(*n, "comment")) {
                let reference = comment.attribute("ref").unwrap_or("");
                let (row, col) = address(reference)?;
                let src = Source {
                    part: Some(path.clone().into()),
                    row: Some(row),
                    cell: Some(col),
                    ..src.clone()
                };
                let author = count(comment.attribute("authorId"), 0)?;
                let title = format!(
                    "Comment {reference}{}",
                    authors
                        .get(author)
                        .map(|a| format!(" — {a}"))
                        .unwrap_or_default()
                );
                let text = child(comment, "text").map(rich).unwrap_or_default();
                let blocks = vec![heading(title, 4, &src, ctx)?, para(text, &src, ctx)?];
                out.push(ctx.block(BlockContent::Quote { blocks }, src)?);
            }
        } else {
            let rels = pkg.rels(&path, ctx)?;
            for n in d.descendants().filter(Node::is_element) {
                if named(n, "chart") {
                    let id =
                        attr(n, "id").ok_or_else(|| bad("drawing chart without relationship"))?;
                    let chart = linked(&rels, id, &path)?;
                    out.extend(super::chart::read(pkg, &chart, src, ctx)?);
                }
                if named(n, "blip") {
                    if let Some(id) = attr(n, "embed") {
                        if let Ok(part) = linked(&rels, id, &path) {
                            if let Some(image) = ctx.image_inline(&part, "") {
                                let anchor = n.ancestors().find(|a| {
                                    matches!(a.tag_name().name(), "oneCellAnchor" | "twoCellAnchor")
                                });
                                let from = anchor.and_then(|a| child(a, "from"));
                                let coordinate = |key| {
                                    from.and_then(|f| child(f, key))
                                        .and_then(|n| n.text())
                                        .and_then(|s| s.parse::<u32>().ok())
                                        .and_then(|v| v.checked_add(1))
                                };
                                let origin = Source {
                                    row: coordinate("row"),
                                    cell: coordinate("col"),
                                    ..src.clone()
                                };
                                out.push(ctx.block(
                                    BlockContent::Paragraph {
                                        inlines: vec![image],
                                    },
                                    origin,
                                )?);
                            }
                        }
                    }
                }
                if named(n, "cNvPr") {
                    if let Some(t) = n.attribute("descr").filter(|t| !t.is_empty()) {
                        out.push(para(t.into(), src, ctx)?);
                    }
                }
                if named(n, "p") {
                    let text: String = n
                        .descendants()
                        .filter(|n| named(*n, "t"))
                        .map(content)
                        .collect();
                    if !text.is_empty() {
                        out.push(para(text, src, ctx)?);
                    }
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod input_tests {
    use super::*;

    #[test]
    fn row_spool_preserves_entities_mixed_text_formulas_and_rich_runs() {
        let xml = r#"<row r="2"><c r="$A$2" s="3" t="str"><v> Å &amp; 東京 </v><f>A1&amp;B1</f></c><c r="B2"><v>one<x>two</x>three</v></c><c r="C2" t="inlineStr"><is><r><rPr><b/></rPr><t>rich</t></r><t> tail</t></is></c><c/></row>"#;
        let doc = Document::parse(xml).unwrap();
        let row = RowInput::from_node(doc.root_element());
        assert_eq!(row.cells[0].value, " Å & 東京 ");
        assert_eq!(row.cells[1].value, "onetwothree");
        let record = serde_json::to_vec(&row).unwrap();
        let replay: RowInput<'static> = serde_json::from_slice(&record).unwrap();
        drop(doc);
        drop(record);
        assert_eq!(replay.cells[0].formula.as_deref(), Some("A1&B1"));
        assert_eq!(
            replay.cells[2].rich.as_ref().unwrap()[0].style.bold,
            Some(true)
        );
        assert_eq!(replay.cells[2].rich.as_ref().unwrap()[1].text, " tail");
        assert!(replay.cells[3].value.is_empty());
        assert_eq!(address("$A$2").unwrap(), address("A2").unwrap());
        assert_eq!(address("XFD1048576").unwrap(), (1_048_576, 16_384));
        assert!(address("東京1").is_err());
    }
}
