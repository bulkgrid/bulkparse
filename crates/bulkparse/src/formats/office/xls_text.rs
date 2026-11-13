//! Bounded BIFF8 drawing/comment text. Never decode embedded image payloads.
use super::{bad, legacy::u16le, Context, Error};

pub(super) fn object(data: &[u8]) -> Option<(u16, u16)> {
    let mut p = 0usize;
    while p.checked_add(4)? <= data.len() {
        let kind = u16le(data, p).ok()?;
        let len = usize::from(u16le(data, p + 2).ok()?);
        let body = data.get(p + 4..p.checked_add(4)?.checked_add(len)?)?;
        if kind == 0x15 {
            return Some((u16le(body, 2).ok()?, u16le(body, 0).ok()?));
        }
        p += 4 + len;
    }
    None
}

pub(super) fn text(
    header: &[u8],
    records: &[(usize, u16, &[u8])],
    ctx: &Context<'_>,
) -> Result<String, Error> {
    let count = usize::from(u16le(header, 10)?);
    if count > ctx.options.limits.max_text_bytes {
        return Err(Error::Limit("max_text_bytes"));
    }
    let mut units = Vec::with_capacity(count);
    for &(_, kind, data) in records {
        if units.len() == count || kind != 0x3c {
            break;
        }
        ctx.check()?;
        let (&flags, bytes) = data
            .split_first()
            .ok_or_else(|| bad("empty drawing text continuation"))?;
        if flags & !1 != 0 {
            return Err(bad("invalid drawing text encoding flag"));
        }
        let width = 1 + usize::from(flags & 1 != 0);
        let take = (count - units.len()).min(bytes.len() / width);
        for i in 0..take {
            units.push(if width == 2 {
                u16le(bytes, i * 2)?
            } else {
                u16::from(bytes[i])
            });
        }
        if units.len() < count && !bytes.len().is_multiple_of(width) {
            return Err(bad("split drawing UTF-16 unit"));
        }
    }
    if units.len() != count {
        return Err(bad("truncated drawing text"));
    }
    let s = String::from_utf16(&units).map_err(bad)?;
    if s.len() > ctx.options.limits.max_text_bytes {
        return Err(Error::Limit("max_text_bytes"));
    }
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CancellationToken, Options};
    #[test]
    fn continued_text_switches_encoding_without_eating_formatting_records() {
        let opts = Options::default();
        let cancel = CancellationToken::default();
        let ctx = Context::new(&opts, &cancel);
        let mut header = [0; 18];
        header[10] = 4;
        let parts = [
            (0, 0x3c, &b"\0ab"[..]),
            (0, 0x3c, &[1, 0x3d, 0xd8, 0x00, 0xde][..]),
            (0, 0x3c, &[255; 8][..]),
        ];
        assert_eq!(text(&header, &parts, &ctx).unwrap(), "ab😀");
        assert!(text(&header, &parts[..1], &ctx).is_err());
        assert!(text(&header, &[(0, 0x3c, &[1, 65])], &ctx).is_err());
        let mut limited = opts.clone();
        limited.limits.max_text_bytes = 3;
        assert!(matches!(
            text(&header, &parts, &Context::new(&limited, &cancel)),
            Err(Error::Limit(_))
        ));
        cancel.cancel();
        assert!(text(&header, &parts, &ctx).is_err());
    }
    #[test]
    fn object_identifiers_require_a_complete_common_object_record() {
        assert_eq!(object(&[0x15, 0, 4, 0, 0x19, 0, 7, 0]), Some((7, 0x19)));
        assert_eq!(object(&[0x15, 0, 4, 0, 0x19, 0, 7]), None);
    }
}
