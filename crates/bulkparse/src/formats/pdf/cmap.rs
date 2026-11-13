//! Bounded, read-only ToUnicode CMap handling. Ranges stay ranges: never expand
//! a four-byte codespace into a reverse map. Compact PDF delimiters are legal.
use super::malformed;
use crate::{Context, Error};

#[derive(Debug)]
enum Token {
    Hex(Vec<u8>),
    Word(String),
    Open,
    Close,
}

fn tokens(bytes: &[u8], ctx: &mut Context<'_>) -> Result<Vec<Token>, Error> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        ctx.check()?;
        match bytes[i] {
            b if b.is_ascii_whitespace() || b == 0 => i += 1,
            b'%' => {
                while i < bytes.len() && !matches!(bytes[i], b'\r' | b'\n') {
                    i += 1;
                }
            }
            b'(' => {
                i += 1;
                let mut depth = 1;
                while i < bytes.len() && depth > 0 {
                    match bytes[i] {
                        b'\\' => i += 1,
                        b'(' => {
                            depth += 1;
                            ctx.depth(depth)?;
                        }
                        b')' => depth -= 1,
                        _ => {}
                    }
                    i += 1;
                }
                if depth != 0 {
                    return Err(malformed("unterminated CMap literal"));
                }
            }
            b'<' if bytes.get(i + 1) == Some(&b'<') => i += 2,
            b'>' if bytes.get(i + 1) == Some(&b'>') => i += 2,
            b'<' => {
                i += 1;
                let mut hex = Vec::new();
                let mut high = None;
                while i < bytes.len() && bytes[i] != b'>' {
                    let b = bytes[i];
                    i += 1;
                    if b.is_ascii_whitespace() {
                        continue;
                    }
                    let digit = (b as char)
                        .to_digit(16)
                        .ok_or_else(|| malformed("invalid CMap hex"))?
                        as u8;
                    if let Some(h) = high.take() {
                        hex.push(h * 16 + digit);
                    } else {
                        high = Some(digit);
                    }
                }
                if i == bytes.len() {
                    return Err(malformed("unterminated CMap hex"));
                }
                i += 1;
                if let Some(h) = high {
                    hex.push(h * 16);
                }
                ctx.node()?;
                out.push(Token::Hex(hex));
            }
            b'[' => {
                i += 1;
                ctx.node()?;
                out.push(Token::Open);
            }
            b']' => {
                i += 1;
                ctx.node()?;
                out.push(Token::Close);
            }
            _ => {
                let start = i;
                i += 1;
                while i < bytes.len()
                    && !bytes[i].is_ascii_whitespace()
                    && !b"<>[]()%/".contains(&bytes[i])
                {
                    i += 1;
                }
                ctx.node()?;
                out.push(Token::Word(
                    String::from_utf8_lossy(&bytes[start..i]).into_owned(),
                ));
            }
        }
    }
    Ok(out)
}
fn code(bytes: &[u8]) -> Result<(u32, u8), Error> {
    if bytes.is_empty() || bytes.len() > 4 {
        return Err(malformed("invalid CMap source code length"));
    }
    Ok((
        bytes.iter().fold(0, |n, b| n * 256 + u32::from(*b)),
        bytes.len() as u8,
    ))
}
fn hex(tokens: &[Token], i: &mut usize) -> Result<Vec<u8>, Error> {
    let Some(Token::Hex(value)) = tokens.get(*i) else {
        return Err(malformed("expected CMap hex value"));
    };
    *i += 1;
    Ok(value.clone())
}
fn unicode(bytes: &[u8]) -> String {
    if !bytes.len().is_multiple_of(2) {
        return "\u{fffd}".into();
    }
    String::from_utf16_lossy(
        &bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| u16::from_be_bytes(*p))
            .collect::<Vec<_>>(),
    )
}
struct Range {
    first: u32,
    last: u32,
    len: u8,
    target: Target,
}
enum Target {
    Sequential(Vec<u8>),
    Array(Vec<String>),
}
#[derive(Default)]
pub(super) struct CMap {
    ranges: Vec<Range>,
    spaces: Vec<(u32, u32, u8)>,
}
impl CMap {
    pub fn parse(bytes: &[u8], ctx: &mut Context<'_>) -> Result<Self, Error> {
        let tokens = tokens(bytes, ctx)?;
        let mut out = Self::default();
        let mut i = 0;
        while i < tokens.len() {
            let Token::Word(word) = &tokens[i] else {
                i += 1;
                continue;
            };
            let (kind, end) = match word.as_str() {
                "begincodespacerange" => (0, "endcodespacerange"),
                "beginbfchar" => (1, "endbfchar"),
                "beginbfrange" => (2, "endbfrange"),
                "usecmap" => return Err(malformed("inherited CMaps are not supported")),
                _ => {
                    i += 1;
                    continue;
                }
            };
            i += 1;
            loop {
                if matches!(tokens.get(i), Some(Token::Word(word)) if word == end) {
                    i += 1;
                    break;
                }
                let (first, len) = code(&hex(&tokens, &mut i)?)?;
                let last = if kind == 1 {
                    first
                } else {
                    let (last, last_len) = code(&hex(&tokens, &mut i)?)?;
                    if last_len != len || last < first {
                        return Err(malformed("invalid CMap range"));
                    }
                    last
                };
                if kind == 0 {
                    out.spaces.push((first, last, len));
                    continue;
                }
                let target = if matches!(tokens.get(i), Some(Token::Open)) {
                    i += 1;
                    let mut values = Vec::new();
                    while !matches!(tokens.get(i), Some(Token::Close)) {
                        values.push(unicode(&hex(&tokens, &mut i)?));
                    }
                    i += 1;
                    if values.len() as u64 != u64::from(last) - u64::from(first) + 1 {
                        return Err(malformed("CMap array length does not match range"));
                    }
                    Target::Array(values)
                } else {
                    Target::Sequential(hex(&tokens, &mut i)?)
                };
                out.ranges.push(Range {
                    first,
                    last,
                    len,
                    target,
                });
            }
        }
        if out.ranges.is_empty() {
            return Err(malformed("empty ToUnicode CMap"));
        }
        // Sorted ranges allow bounded logarithmic lookup for normal disjoint maps.
        out.ranges.sort_by_key(|r| (r.len, r.first));
        for pair in out.ranges.windows(2) {
            if pair[0].len == pair[1].len && pair[0].last >= pair[1].first {
                return Err(malformed("overlapping ToUnicode ranges"));
            }
        }
        out.spaces.sort_by_key(|&(first, _, len)| (len, first));
        for pair in out.spaces.windows(2) {
            if pair[0].2 == pair[1].2 && pair[0].1 >= pair[1].0 {
                return Err(malformed("overlapping CMap codespaces"));
            }
        }
        Ok(out)
    }
    pub fn read(&self, bytes: &[u8], fixed_width: Option<u8>) -> (usize, u32, String) {
        let mut value = 0;
        for (index, b) in bytes.iter().take(4).enumerate() {
            value = value * 256 + u32::from(*b);
            let len = (index + 1) as u8;
            let space_index = self
                .spaces
                .partition_point(|&(a, _, l)| (l, a) <= (len, value));
            let in_space = space_index
                .checked_sub(1)
                .and_then(|i| self.spaces.get(i))
                .is_some_and(|&(_, b, l)| l == len && value <= b);
            let index_range = self
                .ranges
                .partition_point(|r| (r.len, r.first) <= (len, value));
            let range = index_range
                .checked_sub(1)
                .and_then(|i| self.ranges.get(i))
                .filter(|r| r.len == len && value <= r.last);
            if fixed_width.map_or(
                in_space || (self.spaces.is_empty() && range.is_some()),
                |width| width == len,
            ) {
                let text = range
                    .map(|r| match &r.target {
                        Target::Array(values) => values[(value - r.first) as usize].clone(),
                        Target::Sequential(first) => {
                            let mut target = first.clone();
                            let mut carry = value - r.first;
                            for b in target.iter_mut().rev() {
                                let sum = u32::from(*b) + (carry & 255);
                                *b = sum as u8;
                                carry = (carry >> 8) + (sum >> 8);
                            }
                            unicode(&target)
                        }
                    })
                    .unwrap_or_else(|| "\u{fffd}".into());
                return (usize::from(len), value, text);
            }
        }
        (1, u32::from(bytes[0]), "\u{fffd}".into())
    }
    pub fn mismatched_codespace(&self, width: u8) -> bool {
        !self.spaces.is_empty() && !self.spaces.iter().any(|&(_, _, len)| len == width)
    }
}
