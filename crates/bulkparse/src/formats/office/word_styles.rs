//! Word 97+ STSH and FKP style runs, mapped into BulkParse-owned semantic styles.
use super::legacy::{u16le, u32le, utf16};
use super::style::ParagraphStyle;
use super::*;
#[derive(Default)]
pub(super) struct Formatting {
    styles: HashMap<u16, ParagraphStyle>,
    paragraphs: Vec<Run>,
    characters: Vec<Run>,
}
struct Run {
    start: u32,
    end: u32,
    style: u16,
    properties: Vec<u8>,
}
struct Definition {
    parent: u16,
    style: ParagraphStyle,
    pap: Vec<u8>,
    chp: Vec<u8>,
}
fn interval(b: &[u8], start: usize, size: usize) -> Result<&[u8], Error> {
    b.get(start..)
        .and_then(|b| b.get(..size))
        .ok_or_else(|| bad("Word formatting interval outside stream"))
}
fn properties(
    b: &[u8],
    mut apply: impl FnMut(u16, &[u8]) -> Result<(), Error>,
) -> Result<(), Error> {
    let mut pos = 0;
    while pos + 2 <= b.len() {
        let op = u16le(b, pos)?;
        pos += 2;
        let (prefix, size) = match op >> 13 {
            0 | 1 => (0, 1),
            2 | 4 | 5 => (0, 2),
            3 => (0, 4),
            7 => (0, 3),
            _ if matches!(op, 0xd608 | 0xd606) => (
                2,
                (u16le(b, pos)? as usize)
                    .checked_sub(1)
                    .ok_or_else(|| bad("invalid Word table operand"))?,
            ),
            _ => (
                1,
                *b.get(pos).ok_or_else(|| bad("truncated Word property"))? as usize,
            ),
        };
        pos += prefix;
        let value = interval(b, pos, size)?;
        pos += size;
        apply(op, value)?;
    }
    Ok(())
}
fn property_ops(b: &[u8], p: &mut ParagraphStyle) -> Result<(), Error> {
    let base = *p;
    properties(b, |op, v| {
        if let Some(&v) = v.first() {
            let toggle = |old: Option<bool>| match v {
                0 => Some(false),
                1 => Some(true),
                128 => old,
                129 => Some(!old.unwrap_or(false)),
                _ => old,
            };
            match op {
                0x0835 => p.text.bold = toggle(base.text.bold),
                0x0836 => p.text.italic = toggle(base.text.italic),
                0x0837 => p.text.strike = toggle(base.text.strike),
                0x2640 => p.heading = Some(if v <= 8 { v + 1 } else { 0 }),
                _ => {}
            }
        }
        Ok(())
    })
}
#[derive(Default)]
pub(super) struct TableInfo {
    pub inside: bool,
    pub end_row: bool,
    pub nested: bool,
    pub header: bool,
    pub left: i64,
    pub cells: Vec<super::edge_table::Properties>,
}
fn table_properties(b: &[u8], data: &[u8], info: &mut TableInfo) -> Result<(), Error> {
    properties(b, |op, v| {
        match op {
            0x2416 => info.inside = v[0] != 0,
            0x2417 => info.end_row = v[0] != 0,
            0x3404 => info.header = v[0] != 0,
            0x6649 => info.nested |= u32le(v, 0)? > 1,
            0x244b | 0x244c => info.nested |= v[0] != 0,
            0x6646 if !data.is_empty() => {
                let offset = u32le(v, 0)? as usize;
                let size = u16le(data, offset)? as usize;
                // One indirection only: recursive huge-PAPX references are not followed.
                table_properties(interval(data, offset + 2, size)?, &[], info)?;
            }
            0xd608 => {
                let count = *v.first().ok_or_else(|| bad("empty Word row definition"))? as usize;
                if count > 63 {
                    return Err(bad("invalid Word column count"));
                }
                let boundary = |i| u16le(v, 1 + i * 2).map(|x| i64::from(x as i16));
                info.left = boundary(0)?;
                info.cells.clear();
                let start = 1 + (count + 1) * 2;
                for i in 0..count {
                    let flags = if start + i * 20 + 20 <= v.len() {
                        u16le(v, start + i * 20)?
                    } else {
                        0
                    };
                    info.cells.push(super::edge_table::Properties {
                        right: Some(boundary(i + 1)?),
                        horizontal_start: flags & 3 >= 2,
                        horizontal_continue: flags & 3 == 1,
                        vertical_start: (flags >> 5) & 3 == 3,
                        vertical_continue: (flags >> 5) & 3 == 1,
                    });
                }
            }
            0xd62b if v.len() >= 2 => {
                if let Some(cell) = info.cells.get_mut(v[0] as usize) {
                    cell.vertical_continue = v[1] == 1;
                    cell.vertical_start = v[1] == 3;
                }
            }
            _ => {}
        }
        Ok(())
    })
}
fn definitions(
    word: &[u8],
    table: &[u8],
    ctx: &mut Context<'_>,
) -> Result<HashMap<u16, ParagraphStyle>, Error> {
    let size = u32le(word, 0xa6)? as usize;
    if size == 0 {
        return Ok(HashMap::new());
    }
    let b = interval(table, u32le(word, 0xa2)? as usize, size)?;
    let count = u16le(b, 2)?;
    let fixed = (u16le(b, 4)? as usize).max(10);
    let mut pos = 2 + u16le(b, 0)? as usize;
    let mut defs = HashMap::new();
    for id in 0..count {
        ctx.node()?;
        let size = u16le(b, pos)? as usize;
        pos += 2;
        if size == 0 {
            continue;
        }
        let s = interval(b, pos, size)?;
        pos += size;
        let sti = u16le(s, 0)? & 0xfff;
        let base = u16le(s, 2)?;
        let family = base & 15;
        let parent = base >> 4;
        let count = u16le(s, 4)? & 15;
        let length = u16le(s, fixed)? as usize;
        let name = utf16(interval(s, fixed + 2, length * 2)?)?;
        let mut style = ParagraphStyle::named(&name);
        if (1..=9).contains(&sti) {
            style.heading = Some(sti as u8);
        }
        let mut off = fixed + 4 + length * 2;
        let mut upx = vec![];
        for _ in 0..count {
            off += off % 2;
            let size = u16le(s, off)? as usize;
            upx.push(interval(s, off + 2, size)?.to_vec());
            off += 2 + size;
        }
        let (pap, chp) = if family == 1 {
            (
                upx.first()
                    .and_then(|p| p.get(2..))
                    .unwrap_or_default()
                    .to_vec(),
                upx.get(1).cloned().unwrap_or_default(),
            )
        } else if family == 2 {
            (vec![], upx.first().cloned().unwrap_or_default())
        } else {
            // Table/numbering style UPX payloads are not character SPRMs.
            (vec![], vec![])
        };
        defs.insert(
            id,
            Definition {
                parent,
                style,
                pap,
                chp,
            },
        );
    }
    let mut out = HashMap::new();
    for &id in defs.keys() {
        let mut seen = HashSet::new();
        let mut chain = vec![];
        let mut cur = id;
        while cur != 0xfff {
            ctx.depth(chain.len())?;
            if !seen.insert(cur) {
                return Err(bad("cyclic Word style inheritance"));
            }
            let Some(d) = defs.get(&cur) else {
                break;
            };
            chain.push(d);
            if d.parent == cur {
                break;
            }
            cur = d.parent;
        }
        let mut p = ParagraphStyle::default();
        for d in chain.into_iter().rev() {
            p = p.merge(d.style);
            property_ops(&d.pap, &mut p)?;
            property_ops(&d.chp, &mut p)?;
        }
        out.insert(id, p);
    }
    Ok(out)
}
fn runs(
    word: &[u8],
    table: &[u8],
    fib: usize,
    paragraph: bool,
    ctx: &mut Context<'_>,
) -> Result<Vec<Run>, Error> {
    let size = u32le(word, fib + 4)? as usize;
    if size == 0 {
        return Ok(vec![]);
    }
    if size < 4 || !(size - 4).is_multiple_of(8) {
        return Err(bad("invalid Word formatting page index"));
    }
    let plc = interval(table, u32le(word, fib)? as usize, size)?;
    let n = (size - 4) / 8;
    let mut out = vec![];
    let mut pages = HashSet::new();
    for i in 0..n {
        ctx.node()?;
        let pn = u32le(plc, (n + 1) * 4 + i * 4)? & 0x3fffff;
        if !pages.insert(pn) {
            continue;
        }
        let page = interval(word, pn as usize * 512, 512)?;
        let count = page[511] as usize;
        if (count + 1) * 4 + count * if paragraph { 13 } else { 1 } > 511 {
            return Err(bad("invalid Word formatting page"));
        }
        for k in 0..count {
            ctx.node()?;
            let start = u32le(page, k * 4)?;
            let end = u32le(page, (k + 1) * 4)?;
            if end < start {
                return Err(bad("reversed Word style run"));
            }
            let offset = page[(count + 1) * 4 + k * if paragraph { 13 } else { 1 }] as usize * 2;
            let data = if offset == 0 {
                &[][..]
            } else {
                let cb = page[offset] as usize;
                if paragraph {
                    if cb == 0 {
                        interval(
                            page,
                            offset + 2,
                            *page.get(offset + 1).ok_or_else(|| bad("truncated PAPX"))? as usize
                                * 2,
                        )?
                    } else {
                        interval(page, offset + 1, cb * 2 - 1)?
                    }
                } else {
                    interval(page, offset + 1, cb)?
                }
            };
            let (style, properties) = if paragraph && !data.is_empty() {
                (u16le(data, 0)?, data[2..].to_vec())
            } else {
                (0, data.to_vec())
            };
            out.push(Run {
                start,
                end,
                style,
                properties,
            });
        }
    }
    out.sort_by_key(|r| r.start);
    Ok(out)
}
impl Formatting {
    pub fn retain_images(&self, data: &[u8], ctx: &mut Context<'_>) -> Result<(), Error> {
        if !ctx.options.extract_images {
            return Ok(());
        }
        let mut offsets = std::collections::BTreeSet::new();
        for run in &self.characters {
            let result = properties(&run.properties, |op, v| {
                if op == 0x6a03 {
                    offsets.insert(u32le(v, 0)? as usize);
                }
                Ok(())
            });
            if result.is_err() {
                ctx.image_gap(
                    "Malformed Word character properties prevent image reference extraction.",
                    Source::default(),
                );
            }
        }
        for offset in offsets {
            ctx.check()?;
            // CPicLocation also points at fields and OLE storage metadata.
            // Only shape pictures have an OfficeArt payload (MS-DOC MFPF).
            let Some(raw) = data.get(offset..) else {
                continue;
            };
            if u16le(raw, 4).ok() != Some(68) || !matches!(u16le(raw, 6).ok(), Some(100 | 102)) {
                continue;
            }
            let pic = (|| {
                let size = u32le(raw, 0).ok()? as usize;
                let mut header = 68usize;
                if u16le(raw, 6).ok()? == 102 {
                    header = header.checked_add(1 + usize::from(*raw.get(68)?))?;
                }
                raw.get(..size)?.get(header..)
            })();
            let origin = format!("Data/picture-{offset}");
            if let Some(pic) = pic {
                crate::assets::office_art(pic, &origin, ctx)?;
            } else {
                ctx.image_gap(
                    "Word picture reference has no supported PICF payload.",
                    Source {
                        part: Some(origin.into()),
                        ..Default::default()
                    },
                );
            }
        }
        Ok(())
    }
    pub fn load(word: &[u8], table: &[u8], ctx: &mut Context<'_>) -> Result<Self, Error> {
        if table.is_empty() {
            return Ok(Self::default());
        }
        let parse = |ctx: &mut Context<'_>| -> Result<Self, Error> {
            Ok(Self {
                styles: definitions(word, table, ctx)?,
                paragraphs: runs(word, table, 0x102, true, ctx)?,
                characters: runs(word, table, 0xfa, false, ctx)?,
            })
        };
        match parse(ctx) {
            Ok(s) => Ok(s),
            Err(e @ Error::Limit(_)) | Err(e @ Error::Cancelled) => Err(e),
            Err(e) => {
                warn(
                    ctx,
                    "doc_style_metadata",
                    &format!("Malformed optional style metadata ({e}); main-story text retained."),
                    &Default::default(),
                );
                Ok(Self::default())
            }
        }
    }
    pub fn picture_at(&self, fc: u32) -> Option<usize> {
        let index = self
            .characters
            .partition_point(|r| r.start <= fc)
            .checked_sub(1)?;
        let run = self.characters.get(index).filter(|r| fc < r.end)?;
        let mut offset = None;
        properties(&run.properties, |op, v| {
            if op == 0x6a03 {
                offset = Some(u32le(v, 0)? as usize);
            }
            Ok(())
        })
        .ok()?;
        offset
    }
    pub fn table_at(&self, fc: u32, data: &[u8]) -> Result<TableInfo, Error> {
        let mut info = TableInfo::default();
        let i = self.paragraphs.partition_point(|r| r.start <= fc);
        if let Some(r) = i
            .checked_sub(1)
            .and_then(|i| self.paragraphs.get(i))
            .filter(|r| fc < r.end)
        {
            table_properties(&r.properties, data, &mut info)?;
        }
        Ok(info)
    }
    pub fn at(&self, fc: u32) -> ParagraphStyle {
        let lookup = |rs: &[Run]| {
            let i = rs.partition_point(|r| r.start <= fc);
            i.checked_sub(1).filter(|&i| fc < rs[i].end)
        };
        let mut p = ParagraphStyle::default();
        if let Some(i) = lookup(&self.paragraphs) {
            let r = &self.paragraphs[i];
            p = self.styles.get(&r.style).copied().unwrap_or_default();
            let _ = property_ops(&r.properties, &mut p);
        }
        if let Some(i) = lookup(&self.characters) {
            let props = &self.characters[i].properties;
            let _ = properties(props, |op, v| {
                if op == 0x4a30 {
                    if let Some(style) = self.styles.get(&u16le(v, 0)?) {
                        p.text = p.text.merge(style.text);
                    }
                }
                Ok(())
            });
            let _ = property_ops(props, &mut p);
        }
        p
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn auxiliary_picture_positions_follow_encoded_character_offsets() {
        for compressed in [false, true] {
            let options = crate::Options::default();
            let token = crate::CancellationToken::default();
            let mut ctx = Context::new(&options, &token);
            let source = Source {
                part: Some("Data/picture-123".into()),
                ..Default::default()
            };
            ctx.retain_image(b"test image payload", "png", source)
                .unwrap();
            let mut fib = vec![0; 0x6c];
            fib[0x50..0x54].copy_from_slice(&4u32.to_le_bytes());
            let mut stories = super::super::word_aux::Stories::new(&fib, 0).unwrap();
            let text = "é\u{1}X\r";
            let mut word = vec![0; 32];
            let (encoded_offset, image_fc) = if compressed {
                word.extend([0xe9, 1, b'X', 13]);
                (0x40000040, 33)
            } else {
                for c in text.encode_utf16() {
                    word.extend(c.to_le_bytes());
                }
                (32, 34)
            };
            stories
                .append_piece(
                    &word,
                    (0, 4),
                    encoded_offset,
                    encoding_rs::WINDOWS_1252,
                    0,
                    &ctx,
                )
                .unwrap();
            let formatting = Formatting {
                characters: vec![Run {
                    start: image_fc,
                    end: image_fc + 1,
                    style: 0,
                    properties: vec![3, 0x6a, 123, 0, 0, 0],
                }],
                ..Default::default()
            };
            let mut blocks = vec![];
            stories.emit(4, &formatting, &mut ctx, &mut blocks).unwrap();
            let BlockContent::Paragraph { inlines } = &blocks[1].content else {
                panic!("missing auxiliary paragraph")
            };
            assert!(matches!(&inlines[1],Inline::Image {url,..} if url.starts_with("asset:")));
            assert_eq!(
                blocks[1].source.part.as_deref(),
                Some("WordDocument/Footnotes")
            );
        }
    }
    #[test]
    fn toggles_use_style_base_and_table_operands_do_not_shift_following_properties() {
        let mut p = ParagraphStyle::default();
        p.text.bold = Some(true);
        property_ops(&[0x35, 8, 129, 0x35, 8, 129], &mut p).unwrap();
        assert_eq!(p.text.bold, Some(false));
        // cb counts its remainder plus one, unlike ordinary one-byte lengths.
        let bytes = [8, 0xd6, 4, 0, 0, 0, 0, 0x36, 8, 1];
        property_ops(&bytes, &mut p).unwrap();
        assert_eq!(p.text.italic, Some(true));
        assert!(property_ops(&bytes[..5], &mut p).is_err());
    }
    #[test]
    fn table_style_payload_is_not_interpreted_as_character_sprms() {
        let mut word = vec![0u8; 0xaa];
        let mut table = vec![0u8; 20];
        table[0..2].copy_from_slice(&18u16.to_le_bytes());
        table[2..4].copy_from_slice(&1u16.to_le_bytes());
        table[4..6].copy_from_slice(&10u16.to_le_bytes());
        let mut def = vec![0; 10];
        def[2..4].copy_from_slice(&0xfff4u16.to_le_bytes());
        def[4] = 1;
        def.extend([1, 0, b'T', 0, 0, 0, 2, 0, 12, 0]);
        table.extend((def.len() as u16).to_le_bytes());
        table.extend(def);
        word[0xa6..0xaa].copy_from_slice(&(table.len() as u32).to_le_bytes());
        let o = crate::Options::default();
        let cancel = crate::CancellationToken::default();
        let mut ctx = Context::new(&o, &cancel);
        assert_eq!(definitions(&word, &table, &mut ctx).unwrap().len(), 1);
    }
}
