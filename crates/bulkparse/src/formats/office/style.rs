//! Shared semantic text styling and literal numbering; no layout/model engine.
use super::*;
#[derive(Clone, Copy, Default, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(super) struct TextStyle {
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub strike: Option<bool>,
    pub code: Option<bool>,
    pub hidden: Option<bool>,
    pub pre: Option<bool>,
}
impl TextStyle {
    pub fn is_plain_text(self) -> bool {
        [self.bold, self.italic, self.strike, self.code]
            .iter()
            .all(|v| *v != Some(true))
    }
    pub fn merge(self, r: Self) -> Self {
        Self {
            bold: r.bold.or(self.bold),
            italic: r.italic.or(self.italic),
            strike: r.strike.or(self.strike),
            code: r.code.or(self.code),
            hidden: r.hidden.or(self.hidden),
            pre: r.pre.or(self.pre),
        }
    }
    pub fn text(self, s: String) -> Inline {
        let mut i = if self.code == Some(true) {
            Inline::Code { text: s }
        } else {
            Inline::text(s)
        };
        if self.bold == Some(true) {
            i = Inline::Strong { children: vec![i] }
        }
        if self.italic == Some(true) {
            i = Inline::Emphasis { children: vec![i] }
        }
        if self.strike == Some(true) {
            i = Inline::Strike { children: vec![i] }
        }
        i
    }
}
pub(super) fn label(value: i64, kind: &str) -> String {
    if value <= 0 {
        return value.to_string();
    }
    match kind {
        "a" | "A" => {
            let mut n = value as u64;
            let mut s = vec![];
            while n > 0 {
                n -= 1;
                s.push((b'a' + (n % 26) as u8) as char);
                n /= 26;
            }
            let s: String = s.into_iter().rev().collect();
            if kind == "A" {
                s.to_uppercase()
            } else {
                s
            }
        }
        "i" | "I" if value <= 3999 => {
            let mut n = value;
            let mut s = String::new();
            for (v, l) in [
                (1000, "M"),
                (900, "CM"),
                (500, "D"),
                (400, "CD"),
                (100, "C"),
                (90, "XC"),
                (50, "L"),
                (40, "XL"),
                (10, "X"),
                (9, "IX"),
                (5, "V"),
                (4, "IV"),
                (1, "I"),
            ] {
                while n >= v {
                    s.push_str(l);
                    n -= v
                }
            }
            if kind == "i" {
                s.to_lowercase()
            } else {
                s
            }
        }
        _ => value.to_string(),
    }
}

#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub(super) struct ParagraphStyle {
    pub text: TextStyle,
    pub heading: Option<u8>,
    pub kind: Option<&'static str>,
}
impl ParagraphStyle {
    pub fn merge(self, r: Self) -> Self {
        Self {
            text: self.text.merge(r.text),
            heading: r.heading.or(self.heading),
            kind: r.kind.or(self.kind),
        }
    }
    pub fn named(name: &str) -> Self {
        let n = name.trim().replace("_20_", " ").to_ascii_lowercase();
        let kind = match n.as_str() {
            "preformatted text" | "html preformatted" | "source code" => Some("code"),
            "quote" | "quotation" | "quotations" | "intense quote" | "block text" => Some("quote"),
            _ => None,
        };
        let heading = n
            .strip_prefix("heading ")
            .and_then(|n| n.parse::<u8>().ok())
            .filter(|n| (1..=9).contains(n));
        Self {
            kind,
            heading,
            ..Default::default()
        }
    }
}
pub(super) fn plain(items: &[Inline]) -> String {
    let mut s = String::new();
    for i in items {
        match i {
            Inline::Text { text } | Inline::Code { text } => s.push_str(text),
            Inline::Math { tex, .. } => s.push_str(tex),
            Inline::Break => s.push('\n'),
            Inline::Image { alt, .. } => s.push_str(alt),
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Strike { children }
            | Inline::Link { children, .. } => s.push_str(&plain(children)),
            Inline::Anchor { .. } => {}
        }
    }
    s
}
pub(super) fn paragraph(
    inlines: Vec<Inline>,
    style: ParagraphStyle,
    out: &mut Vec<Block>,
    src: &Source,
    ctx: &mut Context<'_>,
) -> Result<(), Error> {
    let b = if style.kind == Some("code") {
        let text = plain(&inlines);
        if let Some(Block {
            content: BlockContent::Code { text: previous, .. },
            ..
        }) = out.last_mut()
        {
            previous.push('\n');
            previous.push_str(&text);
            return Ok(());
        }
        BlockContent::Code {
            language: String::new(),
            text,
        }
    } else if let Some(level) = style.heading.filter(|l| *l > 0 && *l <= 9) {
        BlockContent::Heading {
            level: level.min(6),
            inlines,
        }
    } else if style.kind == Some("quote") {
        let p = ctx.block(BlockContent::Paragraph { inlines }, src.clone())?;
        if let Some(Block {
            content: BlockContent::Quote { blocks },
            ..
        }) = out.last_mut()
        {
            blocks.push(p);
            return Ok(());
        }
        BlockContent::Quote { blocks: vec![p] }
    } else {
        BlockContent::Paragraph { inlines }
    };
    out.push(ctx.block(b, src.clone())?);
    Ok(())
}
