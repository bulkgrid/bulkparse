//! Bounded MS-PPT style records. Runs use UTF-16 offsets, including the final mark.
use super::legacy::{u16le, u32le};
use super::style::TextStyle;
use super::*;
#[derive(Default, Clone, Copy)]
pub(super) struct Level {
    pub text: TextStyle,
    pub bullet: Option<bool>,
}
#[derive(Default)]
pub(super) struct Runs {
    paragraphs: Vec<(usize, usize, Option<bool>)>,
    characters: Vec<(usize, TextStyle)>,
}
struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}
impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], Error> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or_else(|| bad("PPT style size overflow"))?;
        let v = self
            .bytes
            .get(self.pos..end)
            .ok_or_else(|| bad("truncated PPT style"))?;
        self.pos = end;
        Ok(v)
    }
    fn short(&mut self) -> Result<u16, Error> {
        u16le(self.take(2)?, 0)
    }
    fn long(&mut self) -> Result<u32, Error> {
        u32le(self.take(4)?, 0)
    }
    fn paragraph(&mut self) -> Result<Option<bool>, Error> {
        let mask = self.long()?;
        if mask & !0x003f_fdff != 0 {
            return Err(bad("unsupported PPT paragraph style mask"));
        }
        let mut bullet = None;
        if mask & 15 != 0 {
            let flags = self.short()?;
            if mask & 1 != 0 {
                bullet = Some(flags & 1 != 0);
            }
        }
        for (bit, size) in [
            (0x80, 2),
            (0x10, 2),
            (0x40, 2),
            (0x20, 4),
            (0x800, 2),
            (0x1000, 2),
            (0x2000, 2),
            (0x4000, 2),
            (0x100, 2),
            (0x400, 2),
            (0x8000, 2),
        ] {
            if mask & bit != 0 {
                self.take(size)?;
            }
        }
        if mask & 0x100000 != 0 {
            let n = self.short()? as usize;
            self.take(n * 4)?;
        }
        if mask & 0x10000 != 0 {
            self.take(2)?;
        }
        if mask & 0xe0000 != 0 {
            self.take(2)?;
        }
        if mask & 0x200000 != 0 {
            self.take(2)?;
        }
        Ok(bullet)
    }
    fn character(&mut self) -> Result<TextStyle, Error> {
        let mask = self.long()?;
        if mask & !0x00ef_ffff != 0 {
            return Err(bad("unsupported PPT character style mask"));
        }
        let mut text = TextStyle::default();
        if mask & 0xffff != 0 {
            let flags = self.short()?;
            if mask & 1 != 0 {
                text.bold = Some(flags & 1 != 0);
            }
            if mask & 2 != 0 {
                text.italic = Some(flags & 2 != 0);
            }
        }
        for (bit, size) in [
            (0x10000, 2),
            (0x200000, 2),
            (0x400000, 2),
            (0x800000, 2),
            (0x20000, 2),
            (0x40000, 4),
            (0x80000, 2),
        ] {
            if mask & bit != 0 {
                self.take(size)?;
            }
        }
        Ok(text)
    }
}
pub(super) fn master(data: &[u8], kind: u16, ctx: &mut Context<'_>) -> Result<Vec<Level>, Error> {
    let mut r = Reader {
        bytes: data,
        pos: 0,
    };
    let n = r.short()? as usize;
    if n > 10 {
        return Err(bad("too many PPT master levels"));
    }
    let mut levels = vec![];
    for i in 0..n {
        ctx.node()?;
        if kind >= 5 && r.short()? as usize != i {
            return Err(bad("unsupported PPT master level order"));
        }
        levels.push(Level {
            bullet: r.paragraph()?,
            text: r.character()?,
        });
    }
    Ok(levels)
}
fn decode(data: &[u8], len: usize, ctx: &mut Context<'_>) -> Result<Runs, Error> {
    let mut r = Reader {
        bytes: data,
        pos: 0,
    };
    let mut result = Runs::default();
    let total = len + 1;
    let mut covered = 0;
    while covered < total {
        ctx.node()?;
        let n = r.long()? as usize;
        let depth = r.short()? as usize;
        if n == 0 || n > total - covered || depth > 8 {
            return Err(bad("invalid PPT paragraph run"));
        }
        covered += n;
        result.paragraphs.push((covered, depth, r.paragraph()?));
    }
    covered = 0;
    while covered < total {
        ctx.node()?;
        let n = r.long()? as usize;
        if n == 0 || n > total - covered {
            return Err(bad("invalid PPT character run"));
        }
        covered += n;
        result.characters.push((covered, r.character()?));
    }
    Ok(result)
}
fn list(
    entries: &mut std::collections::VecDeque<(usize, Block)>,
    src: &Source,
    ctx: &mut Context<'_>,
    depth: usize,
) -> Result<Block, Error> {
    ctx.depth(depth)?;
    let base = entries.front().unwrap().0;
    let mut items: Vec<Vec<Block>> = vec![];
    while let Some((level, _)) = entries.front() {
        if *level < base {
            break;
        }
        if *level == base {
            items.push(vec![entries.pop_front().unwrap().1]);
        } else {
            let nested = list(entries, src, ctx, depth + 1)?;
            items.last_mut().unwrap().push(nested);
        }
    }
    ctx.block(BlockContent::List { start: None, items }, src.clone())
}
pub(super) fn emit(
    text: &str,
    kind: u32,
    data: Option<&[u8]>,
    defaults: &[Level],
    src: &Source,
    ctx: &mut Context<'_>,
    out: &mut Vec<Block>,
) -> Result<(), Error> {
    let runs = if let Some(data) = data {
        match decode(data, text.encode_utf16().count(), ctx) {
            Ok(r) => r,
            Err(e @ Error::Limit(_)) | Err(e @ Error::Cancelled) => return Err(e),
            Err(e) => {
                warn(
                    ctx,
                    "ppt_style_metadata",
                    &format!("Optional text styles could not be decoded ({e}); text retained."),
                    src,
                );
                Runs::default()
            }
        }
    } else {
        Runs::default()
    };
    let mut offset = 0;
    let mut entries = std::collections::VecDeque::new();
    for paragraph in text.split('\r') {
        let pi = runs
            .paragraphs
            .partition_point(|(end, _, _)| *end <= offset);
        let (_, depth, bullet) = runs
            .paragraphs
            .get(pi)
            .copied()
            .unwrap_or((usize::MAX, 0, None));
        let base = defaults.get(depth).copied().unwrap_or_default();
        let mut inlines = vec![];
        let mut buf = String::new();
        let mut previous = TextStyle::default();
        for c in paragraph.chars() {
            let ci = runs.characters.partition_point(|(end, _)| *end <= offset);
            let style = base
                .text
                .merge(runs.characters.get(ci).map(|(_, s)| *s).unwrap_or_default());
            if (c == '\u{b}' || style != previous) && !buf.is_empty() {
                ctx.text(&buf)?;
                inlines.push(previous.text(std::mem::take(&mut buf)));
            }
            if c == '\u{b}' {
                inlines.push(Inline::Break);
            } else {
                buf.push(c);
            }
            previous = style;
            offset += c.len_utf16();
        }
        if !buf.is_empty() {
            ctx.text(&buf)?;
            inlines.push(previous.text(buf));
        }
        offset += 1;
        if inlines.is_empty() {
            continue;
        }
        let title = matches!(kind, 0 | 6);
        let b = ctx.block(
            if title {
                BlockContent::Heading { level: 2, inlines }
            } else {
                BlockContent::Paragraph { inlines }
            },
            src.clone(),
        )?;
        if !title && bullet.or(base.bullet) == Some(true) {
            entries.push_back((depth, b));
        } else {
            while !entries.is_empty() {
                out.push(list(&mut entries, src, ctx, 0)?);
            }
            out.push(b);
        }
    }
    while !entries.is_empty() {
        out.push(list(&mut entries, src, ctx, 0)?);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn encoded(paragraphs: &[(u32, u16)], characters: &[(u32, u16)]) -> Vec<u8> {
        let mut b = vec![];
        for &(n, level) in paragraphs {
            b.extend(n.to_le_bytes());
            b.extend(level.to_le_bytes());
            b.extend(1u32.to_le_bytes());
            b.extend(1u16.to_le_bytes());
        }
        for &(n, flags) in characters {
            b.extend(n.to_le_bytes());
            b.extend(3u32.to_le_bytes());
            b.extend(flags.to_le_bytes());
        }
        b
    }
    #[test]
    fn utf16_character_runs_preserve_astral_text_and_explicit_master_overrides() {
        let o = crate::Options::default();
        let cancel = crate::CancellationToken::default();
        let mut ctx = Context::new(&o, &cancel);
        let mut out = vec![];
        let data = encoded(&[(4, 0)], &[(2, 1), (2, 0)]);
        let mut master = Level::default();
        master.text.bold = Some(true);
        master.text.italic = Some(true);
        emit(
            "😀x",
            1,
            Some(&data),
            &[master],
            &Source::default(),
            &mut ctx,
            &mut out,
        )
        .unwrap();
        let BlockContent::List { items, .. } = &out[0].content else {
            panic!()
        };
        let BlockContent::Paragraph { inlines } = &items[0][0].content else {
            panic!()
        };
        assert!(
            matches!(&inlines[0],Inline::Strong{children} if super::super::style::plain(children)=="😀")
        );
        assert!(matches!(&inlines[1],Inline::Text{text} if text=="x"));
    }
    #[test]
    fn list_depth_creates_nested_items_and_bad_style_keeps_text() {
        let o = crate::Options::default();
        let cancel = crate::CancellationToken::default();
        let mut ctx = Context::new(&o, &cancel);
        let mut out = vec![];
        let data = encoded(&[(2, 0), (2, 1), (2, 0)], &[(6, 0)]);
        emit(
            "A\rB\rC",
            1,
            Some(&data),
            &[],
            &Source::default(),
            &mut ctx,
            &mut out,
        )
        .unwrap();
        let BlockContent::List { items, .. } = &out[0].content else {
            panic!()
        };
        assert_eq!(items.len(), 2);
        assert!(matches!(items[0][1].content, BlockContent::List { .. }));
        emit(
            "retained",
            1,
            Some(&[0; 10]),
            &[],
            &Source::default(),
            &mut ctx,
            &mut out,
        )
        .unwrap();
        assert!(ctx
            .diagnostics
            .iter()
            .any(|d| d.code == "ppt_style_metadata"));
        let BlockContent::Paragraph { inlines } = &out.last().unwrap().content else {
            panic!()
        };
        assert_eq!(super::super::style::plain(inlines), "retained");
    }
    #[test]
    fn style_work_limits_propagate_and_truncated_master_is_rejected() {
        let mut o = crate::Options::default();
        o.limits.max_nodes = 0;
        let cancel = crate::CancellationToken::default();
        let mut ctx = Context::new(&o, &cancel);
        let mut out = vec![];
        assert!(matches!(
            emit(
                "A",
                1,
                Some(&encoded(&[(2, 0)], &[(2, 0)])),
                &[],
                &Source::default(),
                &mut ctx,
                &mut out
            ),
            Err(Error::Limit(_))
        ));
        assert!(master(&[1, 0], 1, &mut ctx).is_err());
    }
}
