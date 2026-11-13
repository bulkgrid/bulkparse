use super::*;
pub(super) fn u16le(b: &[u8], p: usize) -> Result<u16, Error> {
    Ok(u16::from_le_bytes(
        b.get(p..)
            .and_then(|b| b.get(..2))
            .ok_or_else(|| bad("truncated 16-bit field"))?
            .try_into()
            .unwrap(),
    ))
}
pub(super) fn u32le(b: &[u8], p: usize) -> Result<u32, Error> {
    Ok(u32::from_le_bytes(
        b.get(p..)
            .and_then(|b| b.get(..4))
            .ok_or_else(|| bad("truncated 32-bit field"))?
            .try_into()
            .unwrap(),
    ))
}
pub(super) fn utf16(b: &[u8]) -> Result<String, Error> {
    if !b.len().is_multiple_of(2) {
        return Err(bad("odd UTF-16 byte count"));
    }
    String::from_utf16(
        &b.as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect::<Vec<_>>(),
    )
    .map_err(bad)
}
pub(super) struct Ole<'a> {
    file: cfb::CompoundFile<Cursor<&'a [u8]>>,
    remaining: usize,
}
impl<'a> Ole<'a> {
    fn new(b: &'a [u8], ctx: &Context<'_>) -> Result<Self, Error> {
        let file = cfb::CompoundFile::open(Cursor::new(b)).map_err(bad)?;
        if file.walk().count() > ctx.options.limits.max_archive_entries {
            return Err(Error::Limit("max_archive_entries"));
        }
        Ok(Self {
            file,
            remaining: ctx.options.limits.max_expanded_bytes,
        })
    }
    fn has(&self, p: &str) -> bool {
        self.file.is_stream(p)
    }
    pub(super) fn stream(&mut self, p: &str) -> Result<Vec<u8>, Error> {
        let mut s = self.file.open_stream(p).map_err(bad)?;
        if s.len() > self.remaining as u64 {
            return Err(Error::Limit("max_expanded_bytes"));
        }
        let mut b = vec![];
        s.by_ref()
            .take((self.remaining as u64).saturating_add(1))
            .read_to_end(&mut b)?;
        if b.len() > self.remaining {
            return Err(Error::Limit("max_expanded_bytes"));
        }
        self.remaining -= b.len();
        Ok(b)
    }
}
pub fn detect(b: &[u8], ctx: &Context<'_>) -> Result<Option<Format>, Error> {
    let p = Ole::new(b, ctx)?;
    if p.has("/EncryptedPackage") {
        return Err(Error::Encrypted);
    }
    let found = [
        ("/WordDocument", Format::Doc),
        ("/PowerPoint Document", Format::Ppt),
        ("/Workbook", Format::Xls),
        ("/Book", Format::Xls),
    ]
    .into_iter()
    .filter(|(s, _)| p.has(s))
    .map(|(_, f)| f)
    .collect::<Vec<_>>();
    if found.len() > 1 {
        return Err(bad("ambiguous compound document"));
    }
    Ok(found.first().copied())
}
pub(super) fn write_xls(
    b: &[u8],
    ctx: &mut Context<'_>,
    output: &mut crate::render::Writer<'_>,
) -> Result<(), Error> {
    let mut p = Ole::new(b, ctx)?;
    let s = p.stream(if p.has("/Workbook") {
        "/Workbook"
    } else {
        "/Book"
    })?;
    crate::assets::xls_images(&s, ctx)?;
    super::spreadsheet::xls_output(&s, ctx, Some(output))?;
    Ok(())
}
pub fn parse(f: Format, b: &[u8], ctx: &mut Context<'_>) -> Result<Vec<Block>, Error> {
    let mut p = Ole::new(b, ctx)?;
    match f {
        Format::Doc => doc(&mut p, ctx),
        Format::Ppt => ppt(&mut p, ctx),
        Format::Xls => {
            let s = p.stream(if p.has("/Workbook") {
                "/Workbook"
            } else {
                "/Book"
            })?;
            crate::assets::xls_images(&s, ctx)?;
            super::spreadsheet::xls(&s, ctx)
        }
        _ => unreachable!(),
    }
}
fn doc(p: &mut Ole<'_>, ctx: &mut Context<'_>) -> Result<Vec<Block>, Error> {
    let b = p.stream("/WordDocument")?;
    if u16le(&b, 0)? != 0xA5EC {
        return Err(bad("invalid Word FIB"));
    }
    let flags = u16le(&b, 10)?;
    if flags & 0x100 != 0 {
        return Err(Error::Encrypted);
    }
    if u16le(&b, 2)? < 0xC1 {
        return Err(Error::Unsupported("Word pre-97 binary format".into()));
    }
    let main = u32le(&b, 0x4C)?;
    let mut auxiliary = super::word_aux::Stories::new(&b, main)?;
    let mut final_cp = main;
    let lang = if flags & 0x4000 != 0 {
        u16le(&b, 0x3C)?
    } else {
        u16le(&b, 6)?
    } & 0x3ff;
    let encoding = match lang {
        0x19 | 0x22 | 0x23 => encoding_rs::WINDOWS_1251,
        0x11 => encoding_rs::SHIFT_JIS,
        0x12 => encoding_rs::EUC_KR,
        0x04 => encoding_rs::GBK,
        0x01 => encoding_rs::WINDOWS_1256,
        0x0D => encoding_rs::WINDOWS_1255,
        0x08 => encoding_rs::WINDOWS_1253,
        0x1F => encoding_rs::WINDOWS_1254,
        _ => encoding_rs::WINDOWS_1252,
    };
    let table_name = if flags & 0x200 != 0 {
        "/1Table"
    } else {
        "/0Table"
    };
    let table_stream = if p.has(table_name) {
        p.stream(table_name)?
    } else {
        vec![]
    };
    let formatting = super::word_styles::Formatting::load(&b, &table_stream, ctx)?;
    let data = if p.has("/Data") {
        p.stream("/Data")?
    } else {
        vec![]
    };
    formatting.retain_images(&data, ctx)?;
    let mut positions = vec![];
    let mut text = String::new();
    let clx_len = u32le(&b, 0x1A6)? as usize;
    if clx_len == 0 {
        if flags & 4 != 0 {
            return Err(bad("complex Word file has no piece table"));
        }
        let start = u32le(&b, 0x18)? as usize;
        let end = u32le(&b, 0x1C)? as usize;
        let raw = b
            .get(start..end)
            .ok_or_else(|| bad("Word text interval outside stream"))?;
        let raw = raw
            .get(..main as usize)
            .ok_or_else(|| bad("truncated Word main story"))?;
        let (decoded, errors) = encoding.decode_without_bom_handling(raw);
        if errors {
            return Err(bad("invalid Word text encoding"));
        }
        text = decoded.into_owned();
        let mut fc = start;
        for c in text.chars() {
            positions.push(fc as u32);
            fc += encoding.encode(c.encode_utf8(&mut [0; 4])).0.len();
        }
    } else {
        let start = u32le(&b, 0x1A2)? as usize;
        let len = u32le(&b, 0x1A6)? as usize;
        let clx = table_stream
            .get(
                start
                    ..start
                        .checked_add(len)
                        .ok_or_else(|| bad("piece table overflow"))?,
            )
            .ok_or_else(|| bad("piece table outside stream"))?;
        let mut pos = 0;
        while clx.get(pos) == Some(&1) {
            let len = u16le(clx, pos + 1)? as usize;
            pos = pos
                .checked_add(3 + len)
                .filter(|n| *n <= clx.len())
                .ok_or_else(|| bad("truncated piece properties"))?;
        }
        if clx.get(pos) != Some(&2) {
            return Err(bad("missing Word piece table"));
        }
        let len = u32le(clx, pos + 1)? as usize;
        let data = clx
            .get(pos + 5..)
            .and_then(|b| b.get(..len))
            .ok_or_else(|| bad("truncated Word pieces"))?;
        if len < 4 || !(len - 4).is_multiple_of(12) {
            return Err(bad("invalid Word piece table size"));
        }
        let count = (len - 4) / 12;
        if count > ctx.options.limits.max_nodes {
            return Err(Error::Limit("max_nodes"));
        }
        let mut expected = 0;
        for i in 0..count {
            ctx.node()?;
            let cp = u32le(data, i * 4)?;
            let end = u32le(data, (i + 1) * 4)?;
            if end < cp || cp != expected {
                return Err(bad("noncontiguous Word pieces"));
            }
            expected = end;
            let encoded_offset = u32le(data, (count + 1) * 4 + i * 8 + 2)?;
            auxiliary.append_piece(&b, (cp, end), encoded_offset, encoding, text.len(), ctx)?;
            if cp >= main {
                continue;
            }
            let n = (end.min(main) - cp) as usize;
            let fc = u32le(data, (count + 1) * 4 + i * 8 + 2)?;
            let compressed = fc & 0x40000000 != 0;
            let offset = (fc & 0x3fffffff) as usize / if compressed { 2 } else { 1 };
            let size = n
                .checked_mul(if compressed { 1 } else { 2 })
                .ok_or_else(|| bad("piece size overflow"))?;
            let slice = b
                .get(
                    offset
                        ..offset
                            .checked_add(size)
                            .ok_or_else(|| bad("piece offset overflow"))?,
                )
                .ok_or_else(|| bad("Word piece outside stream"))?;
            let value = if compressed {
                let (s, err) = encoding.decode_without_bom_handling(slice);
                if err {
                    return Err(bad("invalid Word text encoding"));
                }
                s.into_owned()
            } else {
                utf16(slice)?
            };
            if text.len().saturating_add(value.len()) > ctx.options.limits.max_text_bytes {
                return Err(Error::Limit("max_text_bytes"));
            }
            let mut fc = offset;
            for c in value.chars() {
                positions.push(fc as u32);
                fc += if compressed {
                    encoding.encode(c.encode_utf8(&mut [0; 4])).0.len()
                } else {
                    c.len_utf16() * 2
                };
            }
            text.push_str(&value);
        }
        final_cp = expected;
        if expected < main {
            return Err(bad("Word main story is truncated"));
        }
    }
    if text.len() > ctx.options.limits.max_text_bytes {
        return Err(Error::Limit("max_text_bytes"));
    }
    let src = Source {
        part: Some("WordDocument".into()),
        ..Default::default()
    };
    warn(ctx,"doc_formatting","Main-story text, style inheritance, emphasis and paragraph roles extracted; full lists, nested/ambiguous table layouts, floating objects and auxiliary stories remain incomplete.",&src);
    let mut out = vec![];
    let mut tables = super::word_table::Builder::default();
    let mut field = vec![];
    let mut inlines = vec![];
    let mut run = String::new();
    let mut previous = super::style::TextStyle::default();
    let mut paragraph = super::style::ParagraphStyle::default();
    for (index, c) in text.chars().enumerate() {
        let current = formatting.at(positions[index]);
        if matches!(c, '\r' | '\u{7}') || current.text != previous {
            if !run.is_empty() {
                ctx.text(&run)?;
                inlines.push(previous.text(std::mem::take(&mut run)));
            }
            previous = current.text;
        }
        if matches!(c, '\r' | '\u{7}') {
            paragraph = current;
            let info = match formatting.table_at(positions[index], &data) {
                Ok(info) => info,
                Err(e) => {
                    warn(
                        ctx,
                        "doc_table_metadata",
                        &format!("Invalid optional table properties ({e}); text retained."),
                        &src,
                    );
                    Default::default()
                }
            };
            tables.paragraph(
                std::mem::take(&mut inlines),
                (paragraph, info, c == '\u{7}'),
                &src,
                ctx,
                &mut out,
            )?;
            continue;
        }
        paragraph = current;
        match c {
            '\u{13}' => {
                ctx.depth(field.len() + 1)?;
                field.push(false)
            }
            '\u{14}' => {
                if let Some(v) = field.last_mut() {
                    *v = true
                }
            }
            '\u{15}' => {
                field.pop();
            }
            _ if field.iter().all(|v| *v) => match c {
                '\u{1}' | '\u{8}' => {
                    if let Some(image) = formatting
                        .picture_at(positions[index])
                        .and_then(|offset| ctx.image_inline(&format!("Data/picture-{offset}"), ""))
                    {
                        if !run.is_empty() {
                            ctx.text(&run)?;
                            inlines.push(previous.text(std::mem::take(&mut run)));
                        }
                        inlines.push(image);
                    } else {
                        warn(ctx, "doc_object", "Inline object is not reconstructed in the text flow; supported image payloads are retained separately when enabled.", &src);
                    }
                }
                '\u{b}' | '\u{c}' => run.push('\n'),
                c if c >= ' ' || matches!(c, '\t' | '\u{7}') => run.push(c),
                _ => {}
            },
            _ => {}
        }
    }
    if !run.is_empty() {
        ctx.text(&run)?;
        inlines.push(previous.text(run));
    }
    if !inlines.is_empty() {
        tables.paragraph(
            inlines,
            (paragraph, Default::default(), false),
            &src,
            ctx,
            &mut out,
        )?;
    }

    tables.finish(&src, ctx, &mut out)?;
    auxiliary.emit(final_cp, &formatting, ctx, &mut out)?;
    Ok(out)
}
#[derive(Clone, Copy)]
struct Rec<'a> {
    version: u16,
    kind: u16,
    data: &'a [u8],
}
fn record(b: &[u8], offset: usize) -> Result<(Rec<'_>, usize), Error> {
    let version = u16le(b, offset)?;
    let kind = u16le(b, offset + 2)?;
    let len = u32le(b, offset + 4)? as usize;
    let end = offset
        .checked_add(8)
        .and_then(|v| v.checked_add(len))
        .ok_or_else(|| bad("PowerPoint record overflow"))?;
    Ok((
        Rec {
            version,
            kind,
            data: b
                .get(offset + 8..end)
                .ok_or_else(|| bad("truncated PowerPoint record"))?,
        },
        end,
    ))
}
fn records<'a>(b: &'a [u8], ctx: &mut Context<'_>) -> Result<Vec<Rec<'a>>, Error> {
    let mut pos = 0;
    let mut out = vec![];
    while pos < b.len() {
        ctx.node()?;
        let (r, next) = record(b, pos)?;
        out.push(r);
        pos = next;
    }
    Ok(out)
}
type MasterStyles = HashMap<u16, Vec<super::ppt_styles::Level>>;
fn ppt_texts(
    rs: &[Rec<'_>],
    depth: usize,
    src: &Source,
    ctx: &mut Context<'_>,
    out: &mut Vec<Block>,
    master: &MasterStyles,
    image_parts: &[Option<String>],
) -> Result<(), Error> {
    ctx.depth(depth)?;
    let mut kind = 1u32;
    let mut text = String::new();
    let mut style = None;
    let flush = |text: &mut String,
                 kind: u32,
                 style: Option<&[u8]>,
                 out: &mut Vec<Block>,
                 ctx: &mut Context<'_>|
     -> Result<(), Error> {
        if !text.is_empty() {
            super::ppt_styles::emit(
                text,
                kind,
                style,
                master
                    .get(&(kind as u16))
                    .map(Vec::as_slice)
                    .unwrap_or_default(),
                src,
                ctx,
                out,
            )?;
            text.clear();
        }
        Ok(())
    };
    for r in rs {
        ctx.node()?;
        if r.kind == 0x2f14 {
            return Err(Error::Encrypted);
        }
        if r.version & 15 == 15 {
            flush(&mut text, kind, style.take(), out, ctx)?;
            ppt_texts(
                &records(r.data, ctx)?,
                depth + 1,
                src,
                ctx,
                out,
                master,
                image_parts,
            )?;
        } else {
            match r.kind {
                0xf00b | 0xf121 => {
                    let count = usize::from(r.version >> 4);
                    if count.checked_mul(6).is_some_and(|n| n <= r.data.len()) {
                        for property in r.data[..count * 6].as_chunks::<6>().0 {
                            let op = u16le(property, 0)?;
                            if op & 0x3fff == 0x0104 && op & 0x8000 == 0 {
                                let index = u32le(property, 2)? as usize;
                                if let Some(part) = index
                                    .checked_sub(1)
                                    .and_then(|i| image_parts.get(i))
                                    .and_then(Option::as_deref)
                                {
                                    if let Some(image) = ctx.image_inline(part, "") {
                                        flush(&mut text, kind, style.take(), out, ctx)?;
                                        out.push(ctx.block(
                                            BlockContent::Paragraph {
                                                inlines: vec![image],
                                            },
                                            src.clone(),
                                        )?);
                                    }
                                }
                            }
                        }
                    }
                }
                0x0f9f => {
                    flush(&mut text, kind, style.take(), out, ctx)?;
                    kind = u32le(r.data, 0)?;
                }
                0x0fa0 | 0x0fa8 => {
                    let value = if r.kind == 0x0fa0 {
                        utf16(r.data)?
                    } else {
                        r.data.iter().map(|b| char::from(*b)).collect()
                    };
                    if text.len().saturating_add(value.len()) > ctx.options.limits.max_text_bytes {
                        return Err(Error::Limit("max_text_bytes"));
                    }
                    text.push_str(&value);
                }
                0x0fa1 => style = Some(r.data),
                _ => {}
            }
        }
    }
    flush(&mut text, kind, style, out, ctx)
}
fn ppt_master(r: Rec<'_>, ctx: &mut Context<'_>) -> Result<MasterStyles, Error> {
    let mut styles = HashMap::new();
    for r in records(r.data, ctx)? {
        if r.kind == 0x0fa3 {
            let kind = r.version >> 4;
            match super::ppt_styles::master(r.data, kind, ctx) {
                Ok(levels) => {
                    styles.entry(kind).or_insert(levels);
                }
                Err(e @ Error::Limit(_)) | Err(e @ Error::Cancelled) => return Err(e),
                Err(e) => warn(
                    ctx,
                    "ppt_master_style",
                    &format!("Optional master style could not be decoded ({e})."),
                    &Default::default(),
                ),
            }
        }
    }
    Ok(styles)
}
type SlideGroup<'a> = (u32, u32, Vec<Rec<'a>>);
fn ppt_groups<'a>(list: Rec<'a>, ctx: &mut Context<'_>) -> Result<Vec<SlideGroup<'a>>, Error> {
    let mut out = vec![];
    let mut current = None;
    let mut text = vec![];
    for r in records(list.data, ctx)? {
        if r.kind == 0x03F3 {
            if let Some((id, owner)) = current.take() {
                out.push((id, owner, std::mem::take(&mut text)));
            }
            current = Some((u32le(r.data, 0)?, u32le(r.data, 12).unwrap_or(0)));
        } else if current.is_some() {
            text.push(r);
        }
    }
    if let Some((id, owner)) = current {
        out.push((id, owner, text));
    }
    Ok(out)
}
// Resolve one-based OfficeArt picture IDs through the active document's BStore.
fn ppt_image_parts(
    rs: &[Rec<'_>],
    depth: usize,
    ctx: &mut Context<'_>,
    out: &mut Vec<Option<String>>,
) -> Result<(), Error> {
    ctx.depth(depth)?;
    for r in rs {
        if r.kind == 0xf001 {
            for bse in records(r.data, ctx)? {
                if bse.kind == 0xf007 {
                    out.push(
                        u32le(bse.data, 28)
                            .ok()
                            .filter(|v| *v != u32::MAX)
                            .map(|offset| format!("Pictures/{offset}")),
                    );
                }
            }
        } else if r.version & 15 == 15 || r.kind == 0x040b {
            ppt_image_parts(&records(r.data, ctx)?, depth + 1, ctx, out)?;
        }
    }
    Ok(())
}
fn ppt(p: &mut Ole<'_>, ctx: &mut Context<'_>) -> Result<Vec<Block>, Error> {
    if ctx.options.extract_images && p.has("/Pictures") {
        crate::assets::office_art(&p.stream("/Pictures")?, "Pictures", ctx)?;
    }
    let b = p.stream("/PowerPoint Document")?;
    let user = p.stream("/Current User")?;
    if u32le(&user, 12)? == 0xF3D1C4DF {
        return Err(Error::Encrypted);
    }
    let mut edit = u32le(&user, 16)? as usize;
    let mut seen = HashSet::new();
    let mut persist = HashMap::new();
    let mut docid = None;
    while edit != 0 {
        ctx.node()?;
        if !seen.insert(edit) {
            return Err(bad("cyclic PowerPoint edit chain"));
        }
        let (r, _) = record(&b, edit)?;
        if r.kind != 0x0FF5 {
            return Err(bad("invalid PowerPoint edit"));
        }
        if docid.is_none() {
            docid = Some(u32le(r.data, 16)?)
        }
        let (dir, _) = record(&b, u32le(r.data, 12)? as usize)?;
        if dir.kind != 0x1772 {
            return Err(bad("invalid persistence directory"));
        }
        let mut pos = 0;
        while pos < dir.data.len() {
            let head = u32le(dir.data, pos)?;
            pos += 4;
            let start = head & 0xFFFFF;
            let count = head >> 20;
            for i in 0..count {
                ctx.node()?;
                let offset = u32le(dir.data, pos)?;
                pos += 4;
                persist.entry(start + i).or_insert(offset);
            }
        }
        edit = u32le(r.data, 8)? as usize;
    }
    let offset = persist
        .get(&docid.ok_or_else(|| bad("missing active document"))?)
        .ok_or_else(|| bad("unresolved active document"))?;
    let (doc, _) = record(&b, *offset as usize)?;
    if doc.kind != 0x03E8 {
        return Err(bad("invalid document container"));
    }
    let document_records = records(doc.data, ctx)?;
    let mut image_parts = Vec::new();
    if ctx.options.extract_images {
        ppt_image_parts(&document_records, 0, ctx, &mut image_parts)?;
    }
    let mut masters = HashMap::new();
    if let Some(list) = document_records
        .iter()
        .find(|r| r.kind == 0x0ff0 && r.version >> 4 == 1)
    {
        for (persist_id, master_id, _) in ppt_groups(*list, ctx)? {
            if let Some(offset) = persist.get(&persist_id) {
                let (master, _) = record(&b, *offset as usize)?;
                if master.kind == 0x03f8 {
                    masters.insert(master_id, ppt_master(master, ctx)?);
                }
            }
        }
    }
    // Only live persistence entries may provide defaults; never scan obsolete records.
    if masters.is_empty() {
        let mut offsets: Vec<_> = persist.values().copied().collect();
        offsets.sort_unstable();
        offsets.dedup();
        for offset in offsets {
            let (r, _) = record(&b, offset as usize)?;
            if r.kind == 0x03f8 {
                masters.insert(0, ppt_master(r, ctx)?);
                break;
            }
        }
    }
    let empty_master = MasterStyles::new();
    let mut notes: BTreeMap<u32, Vec<Block>> = BTreeMap::new();
    if let Some(list) = document_records
        .iter()
        .find(|r| r.kind == 0x0FF0 && r.version >> 4 == 2)
    {
        for (id, _, texts) in ppt_groups(*list, ctx)? {
            let Some(offset) = persist.get(&id) else {
                warn(
                    ctx,
                    "ppt_notes",
                    "Missing active notes container.",
                    &Default::default(),
                );
                continue;
            };
            let (note, _) = record(&b, *offset as usize)?;
            if note.kind != 0x03F0 {
                return Err(bad("wrong persisted notes type"));
            }
            let owner = records(note.data, ctx)?
                .iter()
                .find(|r| r.kind == 0x03F1)
                .map(|r| u32le(r.data, 0))
                .transpose()?
                .unwrap_or(0);
            if owner & 0x80000000 != 0 {
                continue;
            }
            let src = Source {
                part: Some("PowerPoint Document/notes".into()),
                ..Default::default()
            };
            let mut blocks = vec![];
            ppt_texts(
                &texts,
                0,
                &src,
                ctx,
                &mut blocks,
                &empty_master,
                &image_parts,
            )?;
            ppt_texts(
                &[note],
                0,
                &src,
                ctx,
                &mut blocks,
                &empty_master,
                &image_parts,
            )?;
            notes.entry(owner).or_default().extend(blocks);
        }
    }
    let list = document_records
        .into_iter()
        .find(|r| r.kind == 0x0FF0 && r.version >> 4 == 0)
        .ok_or_else(|| bad("missing active slide list"))?;
    let mut out = vec![];
    let mut slide = 0usize;
    let groups = ppt_groups(list, ctx)?;
    for (id, slide_id, texts) in groups {
        slide += 1;
        if slide > ctx.options.limits.max_pages {
            return Err(Error::Limit("max_pages"));
        }
        let src = Source {
            page: Some(slide as u32),
            part: Some("PowerPoint Document".into()),
            ..Default::default()
        };
        out.push(heading(format!("Slide {slide}"), 1, &src, ctx)?);
        let off = persist
            .get(&id)
            .ok_or_else(|| bad("unresolved active slide"))?;
        let (r, _) = record(&b, *off as usize)?;
        if r.kind != 0x03EE {
            return Err(bad("wrong persisted slide type"));
        }
        let master_id = records(r.data, ctx)?
            .iter()
            .find(|r| r.kind == 0x03ef)
            .map(|r| u32le(r.data, 12))
            .transpose()?
            .unwrap_or(0);
        let master = masters.get(&master_id).unwrap_or(&empty_master);
        if master_id != 0 && !masters.contains_key(&master_id) {
            warn(
                ctx,
                "ppt_master_missing",
                "Referenced master is unavailable; local text styles retained.",
                &src,
            );
        }
        let mut blocks = vec![];
        ppt_texts(&texts, 0, &src, ctx, &mut blocks, master, &image_parts)?;
        ppt_texts(&[r], 0, &src, ctx, &mut blocks, master, &image_parts)?;
        out.extend(blocks);
        if slide_id != 0 {
            if let Some(mut blocks) = notes.remove(&slide_id) {
                for block in &mut blocks {
                    block.source.page = src.page;
                }
                if !blocks.is_empty() {
                    out.push(ctx.block(BlockContent::Quote { blocks }, src.clone())?);
                }
            }
        }
        warn(ctx,"ppt_layout","Active-slide text, basic master/local emphasis, bullets and linked notes extracted; shape geometry, full rich styles, tables and images remain incomplete.",&src);
    }
    for blocks in notes.into_values().filter(|b| !b.is_empty()) {
        warn(
            ctx,
            "ppt_note_owner",
            "Active notes with unresolved slide owner retained at the end.",
            &Default::default(),
        );
        out.push(ctx.block(
            BlockContent::Quote { blocks },
            Source {
                part: Some("PowerPoint Document/notes".into()),
                ..Default::default()
            },
        )?);
    }
    Ok(out)
}

#[cfg(test)]
mod image_tests {
    use super::*;
    #[test]
    fn slide_image_ids_resolve_through_bstore_instead_of_stream_order() {
        let options = crate::Options::default();
        let cancel = crate::CancellationToken::default();
        let mut ctx = Context::new(&options, &cancel);
        ctx.retain_image(
            b"first",
            "png",
            Source {
                part: Some("Pictures/120".into()),
                ..Default::default()
            },
        )
        .unwrap();
        ctx.retain_image(
            b"second",
            "png",
            Source {
                part: Some("Pictures/0".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let mut bse = vec![0; 36];
        bse[28..32].copy_from_slice(&120u32.to_le_bytes());
        let mut records = vec![];
        records.extend(0u16.to_le_bytes());
        records.extend(0xf007u16.to_le_bytes());
        records.extend(36u32.to_le_bytes());
        records.extend(&bse);
        let mut parts = vec![];
        ppt_image_parts(
            &[Rec {
                version: 15,
                kind: 0xf001,
                data: &records,
            }],
            0,
            &mut ctx,
            &mut parts,
        )
        .unwrap();
        let mut prop = vec![];
        prop.extend(0x4104u16.to_le_bytes());
        prop.extend(1u32.to_le_bytes());
        let src = Source {
            page: Some(3),
            part: Some("PowerPoint Document".into()),
            ..Default::default()
        };
        let mut out = vec![];
        ppt_texts(
            &[Rec {
                version: 16,
                kind: 0xf00b,
                data: &prop,
            }],
            0,
            &src,
            &mut ctx,
            &mut out,
            &MasterStyles::new(),
            &parts,
        )
        .unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].source.page, Some(3));
        let BlockContent::Paragraph { inlines } = &out[0].content else {
            panic!()
        };
        assert!(
            matches!(&inlines[0],Inline::Image{url,..} if url==&format!("asset:{}",ctx.assets[0].id))
        );
    }
}
