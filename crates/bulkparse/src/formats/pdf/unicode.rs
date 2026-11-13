//! Selective expansion of glyph presentation forms, not blanket NFKC rewriting.
use super::positioned::Glyph;
use crate::Error;
use unicode_normalization::UnicodeNormalization;

pub(super) fn normalize(glyph: &mut Glyph, limit: usize) -> Result<bool, Error> {
    if glyph.actual_text || !glyph.text.chars().any(presentation) {
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
