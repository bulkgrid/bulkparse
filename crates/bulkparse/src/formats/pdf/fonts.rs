use super::{cmap::CMap, malformed};
use crate::{Context, Error};
use lopdf::{Dictionary, Document, Object};

pub(super) fn object<'a>(doc: &'a Document, value: &'a Object) -> Result<&'a Object, Error> {
    doc.dereference(value).map(|(_, o)| o).map_err(malformed)
}
pub(super) fn number(value: &Object) -> Result<f64, Error> {
    let n = match value {
        Object::Integer(n) => *n as f64,
        Object::Real(n) => f64::from(*n),
        _ => return Err(malformed("expected PDF number")),
    };
    if !n.is_finite() || n.abs() > 1e12 {
        return Err(malformed("non-finite or excessive PDF coordinate"));
    }
    Ok(n)
}
struct Width {
    first: u32,
    last: u32,
    values: Vec<f64>,
}
pub(super) struct Font {
    fallback: Vec<String>,
    cmap: Option<CMap>,
    widths: Vec<Width>,
    default_width: f64,
    standard_widths: Vec<Option<f64>>,
    pub approximate: bool,
    pub vertical: bool,
    pub recovered_codespace: bool,
    pub bold: bool,
    composite: bool,
}
impl Font {
    pub fn new(doc: &Document, dict: &Dictionary, ctx: &mut Context<'_>) -> Result<Self, Error> {
        ctx.node()?;
        let composite = dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Type0");
        let encoding = dict.get(b"Encoding").and_then(Object::as_name).ok();
        let vertical = encoding.is_some_and(|s| s.ends_with(b"-V"));
        let cmap = if let Ok(value) = dict.get(b"ToUnicode") {
            let stream = object(doc, value)?.as_stream().map_err(malformed)?;
            let bytes = stream.get_plain_content().map_err(malformed)?;
            if bytes.len() > ctx.options.limits.max_expanded_bytes {
                return Err(Error::Limit("max_expanded_bytes"));
            }
            Some(CMap::parse(&bytes, ctx)?)
        } else {
            None
        };
        if composite && cmap.is_none() {
            return Err(malformed("Composite font has no ToUnicode map; character identity cannot be inferred safely."));
        }
        let fallback = if !composite {
            let mut base = dict.clone();
            base.remove(b"ToUnicode");
            let encoding = base.get_font_encoding(doc).map_err(malformed)?;
            (0..=255u8)
                .map(|code| {
                    encoding
                        .bytes_to_string(&[code])
                        .unwrap_or_else(|_| "\u{fffd}".into())
                })
                .collect()
        } else {
            Vec::new()
        };
        let metrics = if composite {
            let descendants = object(doc, dict.get(b"DescendantFonts").map_err(malformed)?)?
                .as_array()
                .map_err(malformed)?;
            object(
                doc,
                descendants
                    .first()
                    .ok_or_else(|| malformed("empty descendant font"))?,
            )?
            .as_dict()
            .map_err(malformed)?
        } else {
            dict
        };
        let descriptor = metrics
            .get(b"FontDescriptor")
            .ok()
            .and_then(|v| object(doc, v).ok())
            .and_then(|v| v.as_dict().ok());
        let named_bold = |value: &Object| value.as_name().ok().is_some_and(bold_font_name);
        let bold = dict.get(b"BaseFont").ok().is_some_and(named_bold)
            || metrics.get(b"BaseFont").ok().is_some_and(named_bold)
            || descriptor.is_some_and(|d| {
                d.get(b"FontName").ok().is_some_and(named_bold)
                    || d.get(b"FontWeight")
                        .ok()
                        .and_then(|v| number(v).ok())
                        .is_some_and(|w| w >= 600.)
                    || d.get(b"Flags")
                        .ok()
                        .and_then(|v| v.as_i64().ok())
                        .is_some_and(|v| v & (1 << 18) != 0)
            });
        let mut widths = Vec::new();
        let mut default_width = if composite { 1000.0 } else { 500.0 };
        let mut approximate = false;
        if composite {
            if let Ok(v) = metrics.get(b"DW") {
                default_width = number(v)?;
            }
            if let Ok(w) = metrics.get(b"W") {
                let values = object(doc, w)?.as_array().map_err(malformed)?;
                let mut i = 0;
                while i < values.len() {
                    ctx.node()?;
                    let first =
                        u32::try_from(values[i].as_i64().map_err(malformed)?).map_err(malformed)?;
                    i += 1;
                    let next = values
                        .get(i)
                        .ok_or_else(|| malformed("truncated PDF font widths"))?;
                    i += 1;
                    if let Object::Array(array) = next {
                        let numbers = array.iter().map(number).collect::<Result<Vec<_>, _>>()?;
                        let last = first
                            .checked_add(numbers.len().saturating_sub(1) as u32)
                            .ok_or_else(|| malformed("font width overflow"))?;
                        if !numbers.is_empty() {
                            widths.push(Width {
                                first,
                                last,
                                values: numbers,
                            });
                        }
                    } else {
                        let last =
                            u32::try_from(next.as_i64().map_err(malformed)?).map_err(malformed)?;
                        let width = number(
                            values
                                .get(i)
                                .ok_or_else(|| malformed("truncated PDF font widths"))?,
                        )?;
                        i += 1;
                        if last < first {
                            return Err(malformed("invalid PDF width range"));
                        }
                        widths.push(Width {
                            first,
                            last,
                            values: vec![width],
                        });
                    }
                }
            }
        } else if let Ok(w) = dict.get(b"Widths") {
            let first = dict
                .get(b"FirstChar")
                .and_then(Object::as_i64)
                .unwrap_or(0)
                .max(0) as u32;
            let numbers = object(doc, w)?
                .as_array()
                .map_err(malformed)?
                .iter()
                .map(number)
                .collect::<Result<Vec<_>, _>>()?;
            let last = first
                .checked_add(numbers.len().saturating_sub(1) as u32)
                .ok_or_else(|| malformed("font width overflow"))?;
            if !numbers.is_empty() {
                widths.push(Width {
                    first,
                    last,
                    values: numbers,
                });
            }
        } else {
            approximate = true;
            if dict
                .get(b"BaseFont")
                .and_then(Object::as_name)
                .is_ok_and(|n| n.starts_with(b"Courier"))
            {
                default_width = 600.0;
                approximate = false;
            }
        }
        // Standard fonts may legally omit widths. Resolve metrics through the
        // glyph encoding, not ToUnicode (which can substitute accessibility text).
        // Embedded/custom fonts and explicit widths always retain their own metrics.
        let standard_widths = if !composite
            && dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Type1")
            && widths.is_empty()
            && !descriptor.is_some_and(|d| {
                [b"FontFile".as_slice(), b"FontFile2", b"FontFile3"]
                    .iter()
                    .any(|k| d.has(k))
            }) {
            dict.get(b"BaseFont")
                .and_then(Object::as_name)
                .ok()
                .map(|name| {
                    fallback
                        .iter()
                        .map(|text| super::standard_fonts::width(name, text))
                        .collect()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        widths.sort_by_key(|w| w.first);
        let recovered_codespace =
            !composite && cmap.as_ref().is_some_and(|c| c.mismatched_codespace(1));
        Ok(Self {
            fallback,
            cmap,
            widths,
            default_width,
            standard_widths,
            approximate,
            vertical,
            recovered_codespace,
            bold,
            composite,
        })
    }
    pub fn decode(
        &self,
        bytes: &[u8],
        ctx: &Context<'_>,
    ) -> Result<Vec<(String, f64, bool)>, Error> {
        let mut out = Vec::new();
        let mut i = 0;
        let mut decoded_bytes = 0usize;
        while i < bytes.len() {
            ctx.check()?;
            if out.len() >= ctx.options.limits.max_nodes {
                return Err(Error::Limit("max_nodes"));
            }
            let (len, code, mut text) = if let Some(cmap) = &self.cmap {
                cmap.read(&bytes[i..], (!self.composite).then_some(1))
            } else {
                (
                    1,
                    u32::from(bytes[i]),
                    self.fallback[usize::from(bytes[i])].clone(),
                )
            };
            if text == "\u{fffd}" && len == 1 && !self.composite {
                text = self.fallback[usize::from(bytes[i])].clone();
            }
            let index = self.widths.partition_point(|w| w.first <= code);
            let width = index
                .checked_sub(1)
                .and_then(|i| self.widths.get(i))
                .filter(|w| code <= w.last)
                .map(|w| {
                    w.values[if w.values.len() == 1 {
                        0
                    } else {
                        (code - w.first) as usize
                    }]
                })
                .or_else(|| self.standard_widths.get(code as usize).copied().flatten())
                .unwrap_or(self.default_width);
            decoded_bytes = decoded_bytes.saturating_add(text.len());
            if decoded_bytes > ctx.options.limits.max_text_bytes {
                return Err(Error::Limit("max_text_bytes"));
            }
            out.push((text, width, len == 1 && code == 32));
            i += len;
        }
        Ok(out)
    }
}

// Whole style tokens and separated abbreviations avoid treating family names
// such as Blackadder as bold.
fn bold_font_name(name: &[u8]) -> bool {
    let name = String::from_utf8_lossy(name);
    let name = name
        .split_once('+')
        .filter(|(prefix, _)| prefix.len() == 6 && prefix.bytes().all(|b| b.is_ascii_uppercase()))
        .map_or(name.as_ref(), |(_, rest)| rest);
    for part in name.split(['-', '_', ',', '+', ' ']) {
        let lower = part.to_ascii_lowercase();
        if ["b", "bd", "bi", "bdit", "blk", "sb"].contains(&lower.as_str()) {
            return true;
        }
        // Traditional TeX faces encode the style before a numeric design size.
        let tex = lower.trim_end_matches(|c: char| c.is_ascii_digit());
        if ["cmbx", "cmssbx", "lmbx"].contains(&tex) {
            return true;
        }
        let bytes = part.as_bytes();
        let mut start = 0;
        for i in 1..=bytes.len() {
            let boundary = i == bytes.len()
                || (bytes[i].is_ascii_uppercase()
                    && (bytes[i - 1].is_ascii_lowercase()
                        || (i + 1 < bytes.len() && bytes[i + 1].is_ascii_lowercase())))
                || bytes[i].is_ascii_digit();
            if boundary {
                if [
                    "bold",
                    "black",
                    "heavy",
                    "demi",
                    "semibold",
                    "demibold",
                    "extrabold",
                    "ultrabold",
                ]
                .contains(&part[start..i].to_ascii_lowercase().as_str())
                {
                    return true;
                }
                start = i;
            }
        }
    }
    name.to_ascii_lowercase().starts_with("nimbusromno9l-medi")
}

#[cfg(test)]
mod tests {
    use super::bold_font_name;
    #[test]
    fn font_style_uses_tokens_instead_of_family_substrings() {
        for name in [
            "Helvetica-Bold",
            "ABCDEF+Example-SemiBold",
            "Example-Bd",
            "Example-BI",
            "CMBX12",
            "NimbusRomNo9L-Medi",
        ] {
            assert!(bold_font_name(name.as_bytes()), "{name}");
        }
        for name in [
            "BlackadderITC",
            "Bookman-Light",
            "LinLibertineTB",
            "ABCDEF+Example-Regular",
            "Myboldfamily",
            "HelveticaNeueLTPro",
        ] {
            assert!(!bold_font_name(name.as_bytes()), "{name}");
        }
    }
}
