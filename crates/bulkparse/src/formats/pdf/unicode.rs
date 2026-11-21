//! Selective expansion of glyph presentation forms, not blanket NFKC rewriting.
use super::positioned::Glyph;
use crate::Error;
use unicode_normalization::UnicodeNormalization;

pub(super) fn normalize(glyph: &mut Glyph, limit: usize) -> Result<bool, Error> {
    let layout_only = glyph
        .text
        .chars()
        .all(|c| c.is_whitespace() || c == '\u{200b}');
    if (glyph.actual_text && !layout_only)
        || !glyph
            .text
            .chars()
            .any(|c| presentation(c) || c == '\u{200b}')
    {
        return Ok(false);
    }
    let mut out = String::new();
    for c in glyph.text.chars() {
        if presentation(c) {
            for expanded in c.to_string().nfkc() {
                if out.len() + expanded.len_utf8() > limit {
                    return Err(Error::Limit("max_text_bytes"));
                }
                out.push(expanded);
            }
        } else {
            // U+200B is a word-break opportunity, not visible ink. Preserve its
            // boundary as a space so it cannot bridge column gutters. ZWJ and
            // ZWNJ remain untouched because they affect script shaping.
            let c = if c == '\u{200b}' { ' ' } else { c };
            if out.len() + c.len_utf8() > limit {
                return Err(Error::Limit("max_text_bytes"));
            }
            out.push(c);
        }
    }
    if out == glyph.text {
        return Ok(false);
    }
    glyph.text = out;
    Ok(true)
}
fn presentation(c: char) -> bool {
    matches!(c, '\u{fb00}'..='\u{fb06}' | '\u{fb50}'..='\u{fdff}' | '\u{fe70}'..='\u{fefc}')
}
