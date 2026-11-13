//! Bounded RTF prelude/equation reader; binary payloads are skipped by declared length.
use super::style::ParagraphStyle;
use super::*;
#[derive(Debug)]
pub(super) enum Token {
    Control(String, Option<i64>),
    Text(Vec<u8>),
    Group(Group),
}
#[derive(Debug)]
pub(super) struct Group {
    pub start: usize,
    pub end: usize,
    pub tokens: Vec<Token>,
}
impl Group {
    pub fn kind(&self) -> &str {
        self.tokens
            .iter()
            .find_map(|t| {
                if let Token::Control(k, _) = t {
                    Some(k.as_str())
                } else {
                    None
                }
            })
            .unwrap_or("")
    }
    pub fn control(&self, key: &str) -> Option<i64> {
        self.tokens.iter().find_map(|t| match t {
            Token::Control(k, n) if k == key => Some(n.unwrap_or(1)),
            _ => None,
        })
    }
    pub fn groups(&self) -> impl Iterator<Item = &Group> {
        self.tokens.iter().filter_map(|t| {
            if let Token::Group(g) = t {
                Some(g)
            } else {
                None
            }
        })
    }
}
fn group(
    b: &[u8],
    pos: &mut usize,
    depth: usize,
    mut retain: bool,
    ctx: &mut Context<'_>,
) -> Result<Group, Error> {
    ctx.depth(depth)?;
    let start = *pos;
    *pos += 1;
    let mut tokens = vec![];
    let mut text = vec![];
    let mut first_control = true;
    while *pos < b.len() {
        ctx.check()?;
        let c = b[*pos];
        if (c == b'{' || c == b'}' || c == b'\\') && !text.is_empty() {
            ctx.node()?;
            tokens.push(Token::Text(std::mem::take(&mut text)));
        }
        match c {
            b'{' => {
                let child = group(b, pos, depth + 1, retain, ctx)?;
                if !child.tokens.is_empty() {
                    tokens.push(Token::Group(child));
                }
            }
            b'}' => {
                *pos += 1;
                if !tokens.is_empty() {
                    ctx.node()?;
                }
                return Ok(Group {
                    start,
                    end: *pos,
                    tokens,
                });
            }
            b'\\' => {
                *pos += 1;
                let c = *b.get(*pos).ok_or_else(|| bad("truncated RTF control"))?;
                *pos += 1;
                if c == b'\'' {
                    let raw = b
                        .get(*pos..*pos + 2)
                        .ok_or_else(|| bad("truncated RTF hex"))?;
                    let byte = u8::from_str_radix(std::str::from_utf8(raw).map_err(bad)?, 16)
                        .map_err(bad)?;
                    if retain {
                        text.push(byte);
                    }
                    *pos += 2;
                } else if c.is_ascii_alphabetic() {
                    let a = *pos - 1;
                    while b.get(*pos).is_some_and(u8::is_ascii_alphabetic) {
                        *pos += 1;
                    }
                    let key = std::str::from_utf8(&b[a..*pos]).map_err(bad)?.to_owned();
                    // The main reader handles body text and formatting. This
                    // prepass only needs style definitions and math trees;
                    // retaining every body control duplicates memory and node
                    // accounting on ordinary long RTF documents.
                    if first_control && matches!(key.as_str(), "stylesheet" | "mmath") {
                        retain = true;
                    }
                    first_control = false;
                    let a = *pos;
                    if b.get(*pos) == Some(&b'-') {
                        *pos += 1;
                    }
                    while b.get(*pos).is_some_and(u8::is_ascii_digit) {
                        *pos += 1;
                    }
                    let n = if *pos > a {
                        Some(
                            std::str::from_utf8(&b[a..*pos])
                                .map_err(bad)?
                                .parse::<i64>()
                                .map_err(bad)?,
                        )
                    } else {
                        None
                    };
                    if b.get(*pos) == Some(&b' ') {
                        *pos += 1;
                    }
                    if key == "bin" {
                        let n = n
                            .filter(|n| *n >= 0)
                            .ok_or_else(|| bad("invalid binary length"))?
                            as usize;
                        *pos = pos
                            .checked_add(n)
                            .filter(|p| *p <= b.len())
                            .ok_or_else(|| bad("truncated RTF binary"))?;
                    } else if retain || (depth == 0 && key == "ansicpg") {
                        ctx.node()?;
                        tokens.push(Token::Control(key, n));
                    }
                } else if matches!(c, b'\\' | b'{' | b'}') {
                    if retain {
                        text.push(c)
                    }
                } else if c == b'~' && retain {
                    text.push(0xa0)
                }
            }
            b'\r' | b'\n' => *pos += 1,
            _ => {
                if retain {
                    text.push(c);
                }
                *pos += 1;
            }
        }
    }
    if !text.is_empty() {
        ctx.node()?;
        tokens.push(Token::Text(text));
    }
    if !tokens.is_empty() {
        ctx.node()?;
    }
    Ok(Group {
        start,
        end: *pos,
        tokens,
    }) // main reader reports the unbalanced group.
}
pub(super) fn parse(b: &[u8], ctx: &mut Context<'_>) -> Result<Group, Error> {
    group(b, &mut 0, 0, false, ctx)
}
fn text(g: &Group, enc: &'static encoding_rs::Encoding) -> String {
    let mut out = String::new();
    let mut uc = 1usize;
    let mut skip = 0usize;
    let mut high = None;
    for t in &g.tokens {
        match t {
            Token::Text(b) => {
                let k = skip.min(b.len());
                skip -= k;
                out.push_str(&enc.decode_without_bom_handling(&b[k..]).0);
            }
            Token::Control(k, Some(n)) if k == "uc" => uc = (*n).clamp(0, 16) as usize,
            Token::Control(k, Some(n)) if k == "u" => {
                let u = *n as u16;
                if (0xd800..=0xdbff).contains(&u) {
                    high = Some(u)
                } else if let Some(h) = high.take() {
                    out.push_str(&String::from_utf16_lossy(&[h, u]))
                } else {
                    out.push_str(&String::from_utf16_lossy(&[u]));
                }
                skip = uc;
            }
            Token::Group(g) => out.push_str(&text(g, enc)),
            _ => {}
        }
    }
    out
}
pub(super) fn styles(
    root: &Group,
    ctx: &Context<'_>,
) -> Result<HashMap<i64, ParagraphStyle>, Error> {
    let enc = encoding_rs::Encoding::for_label(
        format!("windows-{}", root.control("ansicpg").unwrap_or(1252)).as_bytes(),
    )
    .unwrap_or(encoding_rs::WINDOWS_1252);
    let mut defs = HashMap::new();
    for sheet in root.groups().filter(|g| g.kind() == "stylesheet") {
        for g in sheet.groups() {
            if let Some(id) = g.control("s") {
                let mut p = ParagraphStyle::named(text(g, enc).trim_end_matches(';'));
                p.text.bold = g.control("b").map(|n| n != 0);
                p.text.italic = g.control("i").map(|n| n != 0);
                p.text.strike = g.control("strike").map(|n| n != 0);
                p.heading = g
                    .control("outlinelevel")
                    .map(|n| if (0..=8).contains(&n) { n as u8 + 1 } else { 0 })
                    .or(p.heading);
                defs.insert(id, (g.control("sbasedon"), p));
            }
        }
    }
    let mut result = HashMap::new();
    for &id in defs.keys() {
        let mut chain = vec![];
        let mut seen = HashSet::new();
        let mut cur = Some(id);
        while let Some(n) = cur {
            ctx.depth(chain.len())?;
            if !seen.insert(n) {
                return Err(bad("cyclic RTF style inheritance"));
            }
            let Some((parent, p)) = defs.get(&n) else {
                break;
            };
            chain.push(*p);
            cur = parent.filter(|p| *p != 222 && *p != n);
        }
        let mut p = ParagraphStyle::default();
        for next in chain.into_iter().rev() {
            p = p.merge(next);
        }
        result.insert(id, p);
    }
    Ok(result)
}
fn escaped(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
fn math_xml(g: &Group, enc: &'static encoding_rs::Encoding) -> String {
    let key = g.kind();
    let name = match key {
        "mmath" => "oMath",
        "moMath" => "oMath",
        "moMathPara" => "oMathPara",
        "mr" => "r",
        _ => key.strip_prefix('m').unwrap_or(""),
    };
    if key == "mmathPict" || name.ends_with("ctrlPr") {
        return String::new();
    }
    if name.is_empty() {
        return g.groups().map(|g| math_xml(g, enc)).collect();
    }
    if key == "mr" {
        return format!(
            "<r>{}<t>{}</t></r>",
            if g.control("mnor").is_some_and(|n| n != 0) {
                "<rPr><nor val=\"1\"/></rPr>"
            } else {
                ""
            },
            escaped(&text(g, enc))
        );
    }
    if matches!(name, "chr" | "begChr" | "endChr" | "sepChr" | "degHide") {
        return format!("<{name} val=\"{}\"/>", escaped(text(g, enc).trim()));
    }
    let children: String = g.groups().map(|g| math_xml(g, enc)).collect();
    format!("<{name}>{children}</{name}>")
}
pub(super) fn equations(
    root: &Group,
    ctx: &mut Context<'_>,
) -> Result<HashMap<usize, (usize, Inline)>, Error> {
    fn scan(
        g: &Group,
        out: &mut HashMap<usize, (usize, Inline)>,
        ctx: &mut Context<'_>,
    ) -> Result<(), Error> {
        if g.kind() == "mmath" {
            let s = math_xml(g, encoding_rs::WINDOWS_1252);
            let d = xml(&s, ctx)?;
            let i = super::math::inline(
                d.root_element(),
                ctx,
                &Source {
                    part: Some("rtf/equation".into()),
                    ..Default::default()
                },
            )?;
            out.insert(g.start, (g.end, i));
        } else {
            for c in g.groups() {
                scan(c, out, ctx)?;
            }
        }
        Ok(())
    }
    let mut out = HashMap::new();
    scan(root, &mut out, ctx)?;
    Ok(out)
}
