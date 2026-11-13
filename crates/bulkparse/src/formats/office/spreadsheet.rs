use super::legacy::{u16le, u32le, utf16};
use super::*;
fn real(b: &[u8], p: usize) -> Result<f64, Error> {
    let v = f64::from_le_bytes(
        b.get(p..p + 8)
            .ok_or_else(|| bad("truncated number"))?
            .try_into()
            .unwrap(),
    );
    if !v.is_finite() {
        return Err(bad("non-finite spreadsheet number"));
    }
    Ok(v)
}
fn rk(n: u32) -> f64 {
    let v = if n & 2 != 0 {
        ((n as i32) >> 2) as f64
    } else {
        f64::from_bits(u64::from(n & !3) << 32)
    };
    if n & 1 != 0 {
        v / 100.0
    } else {
        v
    }
}
fn formatted(
    v: f64,
    style: usize,
    styles: &[u32],
    custom: &HashMap<u32, String>,
    epoch: bool,
    ctx: &mut Context<'_>,
    src: &Source,
) -> String {
    let id = styles.get(style).copied().unwrap_or(0);
    let fmt = custom
        .get(&id)
        .map(String::as_str)
        .unwrap_or(xlsx::builtin(id));
    xlsx::display(&v.to_string(), fmt, epoch).unwrap_or_else(|| {
        warn(
            ctx,
            "sheet_number_format",
            "Unsupported number format; numeric value retained.",
            src,
        );
        v.to_string()
    })
}
fn insert(
    grid: &mut BTreeMap<(u32, u32), String>,
    key: (u32, u32),
    s: String,
    bytes: &mut usize,
    ctx: &Context<'_>,
) -> Result<(), Error> {
    if key.0 == 0 || key.0 > 1_048_576 || key.1 == 0 || key.1 > 16384 {
        return Err(bad("cell outside worksheet bounds"));
    }
    if s.is_empty() {
        return Ok(());
    }
    *bytes = bytes
        .checked_add(s.len())
        .ok_or(Error::Limit("max_text_bytes"))?;
    if *bytes > ctx.options.limits.max_text_bytes {
        return Err(Error::Limit("max_text_bytes"));
    }
    if grid.len() >= ctx.options.limits.max_table_cells {
        return Err(Error::Limit("max_table_cells"));
    }
    if grid.insert(key, s).is_some() {
        return Err(bad("duplicate spreadsheet cell"));
    }
    Ok(())
}
// BIFF8 strings have distinct continuation rules for character data and metadata.
struct Seg<'a> {
    parts: Vec<&'a [u8]>,
    index: usize,
    pos: usize,
}
impl<'a> Seg<'a> {
    fn new(parts: Vec<&'a [u8]>) -> Self {
        Self {
            parts,
            index: 0,
            pos: 0,
        }
    }
    fn byte(&mut self) -> Result<u8, Error> {
        while self.index < self.parts.len() && self.pos == self.parts[self.index].len() {
            self.index += 1;
            self.pos = 0
        }
        let v = *self
            .parts
            .get(self.index)
            .and_then(|b| b.get(self.pos))
            .ok_or_else(|| bad("truncated BIFF string"))?;
        self.pos += 1;
        Ok(v)
    }
    fn n(&mut self, n: usize) -> Result<u32, Error> {
        let mut v = 0;
        for i in 0..n {
            v |= u32::from(self.byte()?) << (i * 8)
        }
        Ok(v)
    }
    fn string(&mut self, short: bool) -> Result<String, Error> {
        let len = self.n(if short { 1 } else { 2 })? as usize;
        let flags = self.byte()?;
        let mut wide = flags & 1 != 0;
        let runs = if flags & 8 != 0 {
            self.n(2)? as usize
        } else {
            0
        };
        let ext = if flags & 4 != 0 {
            self.n(4)? as usize
        } else {
            0
        };
        let mut chars = Vec::with_capacity(len);
        for _ in 0..len {
            if self
                .parts
                .get(self.index)
                .is_some_and(|b| self.pos == b.len())
            {
                self.index += 1;
                self.pos = 0;
                wide = self.byte()? & 1 != 0;
            }
            if wide
                && self
                    .parts
                    .get(self.index)
                    .is_some_and(|b| b.len() - self.pos < 2)
            {
                return Err(bad("split BIFF UTF-16 unit"));
            }
            chars.push(self.n(if wide { 2 } else { 1 })? as u16)
        }
        for _ in 0..runs
            .checked_mul(4)
            .and_then(|n| n.checked_add(ext))
            .ok_or_else(|| bad("BIFF string extension overflow"))?
        {
            self.byte()?;
        }
        String::from_utf16(&chars).map_err(bad)
    }

    // BIFF5/7 lengths count encoded bytes. CONTINUE has no Unicode flags byte,
    // and a multibyte character can cross the record boundary.
    fn byte_string(
        &mut self,
        short: bool,
        encoding: &'static encoding_rs::Encoding,
    ) -> Result<String, Error> {
        let len = self.n(if short { 1 } else { 2 })? as usize;
        if let Some(raw) = self
            .parts
            .get(self.index)
            .and_then(|p| p.get(self.pos..self.pos + len))
        {
            self.pos += len;
            return biff_decode(raw, encoding);
        }
        let mut raw = Vec::with_capacity(len);
        for _ in 0..len {
            raw.push(self.byte()?);
        }
        biff_decode(&raw, encoding)
    }
}

fn biff_decode(raw: &[u8], encoding: &'static encoding_rs::Encoding) -> Result<String, Error> {
    let (text, errors) = encoding.decode_without_bom_handling(raw);
    if errors {
        return Err(bad("invalid BIFF code-page text"));
    }
    Ok(text.into_owned())
}

fn biff_codepage(cp: u16) -> Result<&'static encoding_rs::Encoding, Error> {
    use encoding_rs::*;
    Ok(match cp {
        874 => WINDOWS_874,
        932 => SHIFT_JIS,
        936 => GBK,
        949 => EUC_KR,
        950 => BIG5,
        1250 => WINDOWS_1250,
        1251 => WINDOWS_1251,
        1252 | 32769 => WINDOWS_1252,
        1253 => WINDOWS_1253,
        1254 => WINDOWS_1254,
        1255 => WINDOWS_1255,
        1256 => WINDOWS_1256,
        1257 => WINDOWS_1257,
        1258 => WINDOWS_1258,
        866 => IBM866,
        10000 | 32768 => MACINTOSH,
        10007 => X_MAC_CYRILLIC,
        65001 => UTF_8,
        _ => return Err(Error::Unsupported(format!("XLS code page {cp}"))),
    })
}

fn record_string(
    rec: &[(usize, u16, &[u8])],
    index: usize,
    offset: usize,
    short: bool,
    legacy: Option<&'static encoding_rs::Encoding>,
) -> Result<String, Error> {
    let data = rec[index]
        .2
        .get(offset..)
        .ok_or_else(|| bad("truncated BIFF string field"))?;
    let mut parts = vec![data];
    if let Some(encoding) = legacy {
        parts.extend(
            rec[index + 1..]
                .iter()
                .take_while(|r| r.1 == 0x003C)
                .map(|r| r.2),
        );
        Seg::new(parts).byte_string(short, encoding)
    } else {
        Seg::new(parts).string(short)
    }
}
pub fn xls(b: &[u8], ctx: &mut Context<'_>) -> Result<Vec<Block>, Error> {
    xls_output(b, ctx, None)
}

// Check the same headers/budget before allocating the BIFF record index. Large
// rejected workbooks otherwise leave a grown multi-megabyte index in the heap.
fn biff_record_count(b: &[u8], ctx: &Context<'_>) -> Result<usize, Error> {
    let mut budget = ctx.budget_probe();
    let mut count = 0;
    let mut p = 0;
    while p + 4 <= b.len() {
        if ctx.recovering_limits && count >= ctx.options.limits.max_nodes / 2 {
            break;
        }
        budget.node()?;
        let kind = u16le(b, p)?;
        let len = u16le(b, p + 2)? as usize;
        let end = p + 4 + len;
        b.get(p + 4..end)
            .ok_or_else(|| bad("truncated BIFF record"))?;
        count += 1;
        p = end;
        if kind == 0 && len == 0 && b[p..].iter().all(|v| *v == 0) {
            break;
        }
    }
    Ok(count)
}

pub(super) fn xls_output(
    b: &[u8],
    ctx: &mut Context<'_>,
    mut output: Option<&mut crate::render::Writer<'_>>,
) -> Result<Vec<Block>, Error> {
    let mut completed = false;
    biff_record_count(b, ctx)?;
    // Preserve the established allocation pattern for accepted indexes. Exact
    // reservations reduced isolated peaks but raised persistent-worker RSS.
    let mut rec = vec![];
    let mut p = 0;
    let mut records_truncated = false;
    while p + 4 <= b.len() {
        if ctx.recovering_limits && rec.len() >= ctx.options.limits.max_nodes / 2 {
            records_truncated = true;
            break;
        }
        ctx.node()?;
        let kind = u16le(b, p)?;
        let len = u16le(b, p + 2)? as usize;
        let end = p + 4 + len;
        let data = b
            .get(p + 4..end)
            .ok_or_else(|| bad("truncated BIFF record"))?;
        rec.push((p, kind, data));
        p = end;
        if kind == 0 && len == 0 && b[p..].iter().all(|b| *b == 0) {
            break;
        }
    }
    if rec.is_empty() && records_truncated {
        return Err(Error::Limit("max_nodes"));
    }
    if !matches!(rec.first(), Some((_, 0x0809, _))) {
        return Err(Error::Unsupported("pre-BIFF5 spreadsheet".into()));
    }
    let version = u16le(rec[0].2, 0)?;
    if !matches!(version, 0x0500 | 0x0600) {
        return Err(Error::Unsupported(
            "only BIFF5/7 and BIFF8 XLS are supported".into(),
        ));
    }
    if rec.iter().any(|r| r.1 == 0x002F) {
        return Err(Error::Encrypted);
    }
    // Resolve the workbook's encoding before decoding any strings, even when
    // CODEPAGE appears after BOUNDSHEET or FORMAT. BIFF8 needs no extra scan.
    let legacy = if version == 0x0500 {
        let cp = rec
            .iter()
            .take_while(|r| r.1 != 0x000A)
            .find(|r| r.1 == 0x0042);
        Some(if let Some(cp) = cp {
            biff_codepage(u16le(cp.2, 0)?)?
        } else {
            ctx.diagnostic(
                "xls_codepage_assumed",
                "Workbook has no code page; legacy text decoded as Windows-1252.",
                Source::default(),
                false,
            );
            encoding_rs::WINDOWS_1252
        })
    } else {
        None
    };
    let mut strings = vec![];
    let mut sheets = vec![];
    let mut custom = HashMap::new();
    let mut styles = vec![];
    let mut epoch = false;
    let mut i = 0;
    while i < rec.len() {
        let (_, kind, data) = rec[i];
        match kind {
            0x000A => break,
            0x0022 => epoch = u16le(data, 0)? != 0,
            0x0085 => {
                let offset = u32le(data, 0)? as usize;
                let state = *data.get(4).ok_or_else(|| bad("sheet state"))?;
                let kind = *data.get(5).ok_or_else(|| bad("sheet type"))?;
                let name = record_string(&rec, i, 6, true, legacy)?;
                sheets.push((offset, state, kind, name));
            }
            0x00FC if legacy.is_none() => {
                let total = u32le(data, 4)? as usize;
                if total > ctx.options.limits.max_nodes {
                    return Err(Error::Limit("max_nodes"));
                }
                let mut parts = vec![data.get(8..).ok_or_else(|| bad("SST header"))?];
                while rec.get(i + 1).is_some_and(|r| r.1 == 0x003C) {
                    i += 1;
                    parts.push(rec[i].2)
                }
                let mut reader = Seg::new(parts);
                let mut bytes = 0usize;
                for _ in 0..total {
                    let s = reader.string(false)?;
                    bytes = bytes.saturating_add(s.len());
                    if bytes > ctx.options.limits.max_text_bytes {
                        return Err(Error::Limit("max_text_bytes"));
                    }
                    strings.push(s)
                }
            }
            0x041E => {
                custom.insert(
                    u16le(data, 0)? as u32,
                    record_string(&rec, i, 2, legacy.is_some(), legacy)?,
                );
            }
            0x00E0 => styles.push(u16le(data, 2)? as u32),
            _ => {}
        }
        i += 1;
    }
    let mut out = vec![];
    let mut bytes = 0;
    for (index, (offset, state, kind, name)) in sheets.into_iter().enumerate() {
        if index >= ctx.options.limits.max_pages {
            return Err(Error::Limit("max_pages"));
        }
        let src = Source {
            part: Some(format!("Workbook/{name}").into()),
            ..Default::default()
        };
        if state & 3 != 0 {
            ctx.diagnostic(
                "office_hidden_content",
                "Hidden worksheet omitted.",
                src,
                false,
            );
            continue;
        }
        if kind != 0 {
            warn(
                ctx,
                "xls_sheet_type",
                "Non-worksheet substream omitted.",
                &src,
            );
            continue;
        }
        let checkpoint = out.len();
        let result = (|| -> Result<(), Error> {
            let start = rec.binary_search_by_key(&offset, |r| r.0).map_err(|_| {
                if records_truncated {
                    Error::Limit("max_nodes")
                } else {
                    bad("sheet offset is not a record boundary")
                }
            })?;
            // The workbook-global BOF selects the string layout. Some legacy
            // writers use a newer BOF version in the individual sheet headers.
            if rec[start].1 != 0x0809 || u16le(rec[start].2, 2)? != 0x0010 {
                return Err(bad("invalid worksheet substream"));
            }
            out.push(heading(name, 2, &src, ctx)?);
            let mut grid = BTreeMap::new();
            let mut merges = vec![];
            let mut pending = None;
            let mut nested = 0usize;
            let mut ended = false;
            let mut object = None;
            let mut object_text = Vec::new();
            let mut notes = HashMap::new();
            for (record_index, &(_, kind, data)) in rec[start + 1..].iter().enumerate() {
                ctx.check()?;
                // Embedded charts have their own BOF/EOF and cell coordinates.
                // Do not insert their cached series values into the worksheet or
                // mistake their EOF for the end of the containing sheet.
                if kind == 0x0809 {
                    nested = nested.checked_add(1).ok_or(Error::Limit("max_depth"))?;
                    ctx.depth(nested)?;
                    warn(
                        ctx,
                        "xls_embedded_substream",
                        "Embedded chart/object substream omitted; worksheet cells retained.",
                        &src,
                    );
                    continue;
                }
                if kind == 0x000A {
                    if nested > 0 {
                        nested -= 1;
                        continue;
                    }
                    ended = true;
                    break;
                }
                // Text objects inside chart substreams are useful captions;
                // their cell records must still never enter the worksheet grid.
                if nested > 0 && kind != 0x01B6 {
                    continue;
                }
                if kind == 0x00E5 {
                    let n = u16le(data, 0)? as usize;
                    for j in 0..n {
                        let p = 2 + j * 8;
                        merges.push((
                            u16le(data, p)? as u32 + 1,
                            u16le(data, p + 4)? as u32 + 1,
                            u16le(data, p + 2)? as u32 + 1,
                            u16le(data, p + 6)? as u32 + 1,
                        ));
                    }
                    continue;
                }
                if kind == 0x0207 {
                    if let Some(key) = pending.take() {
                        insert(
                            &mut grid,
                            key,
                            record_string(&rec, start + record_index + 1, 0, false, legacy)?,
                            &mut bytes,
                            ctx,
                        )?;
                    }
                    continue;
                }
                if kind == 0x001C && legacy.is_some() {
                    // BIFF5/7 NOTE text is inline rather than a BIFF8 object ID.
                    warn(
                        ctx,
                        "xls_legacy_comment",
                        "Legacy inline cell comment omitted; worksheet values retained.",
                        &src,
                    );
                    continue;
                }
                if kind == 0x001C && data.len() >= 8 {
                    notes.insert(
                        u16le(data, 6)?,
                        (
                            u32::from(u16le(data, 0)?) + 1,
                            u32::from(u16le(data, 2)?) + 1,
                        ),
                    );
                }
                if matches!(kind, 0x00EC | 0x005D | 0x01B6) {
                    if kind == 0x005D {
                        object = super::xls_text::object(data);
                    } else if kind == 0x01B6 {
                        let owner = object.take();
                        match super::xls_text::text(data, &rec[start + record_index + 2..], ctx) {
                            Ok(text) if !text.trim().is_empty() => {
                                object_text.push((owner, para(text, &src, ctx)?));
                            }
                            Err(Error::Malformed(_)) => warn(ctx, "xls_drawing_text_unreadable", "Drawing/comment text could not be decoded completely; available worksheet cells retained.", &src),
                            Err(error) => return Err(error),
                            _ => {}
                        }
                    }
                    warn(
                        ctx,
                        "xls_visual_content",
                        "Drawing geometry and embedded objects are not reconstructed; drawing/comment text and supported image assets are retained where available.",
                        &src,
                    );
                    continue;
                }
                if kind == 0x0208 && data.get(12).is_some_and(|b| b & 0x20 != 0)
                    || kind == 0x007D && u16le(data, 8).unwrap_or(0) & 1 != 0
                {
                    ctx.diagnostic(
                        "xls_hidden_content",
                        "Hidden rows/columns retained with their original coordinates.",
                        src.clone(),
                        false,
                    );
                }
                if kind == 0x00BD {
                    let row = u16le(data, 0)? as u32 + 1;
                    let col = u16le(data, 2)? as u32 + 1;
                    if data.len() < 6 || (data.len() - 6) % 6 != 0 {
                        return Err(bad("invalid MULRK"));
                    }
                    for (j, p) in (4..data.len() - 2).step_by(6).enumerate() {
                        let t = formatted(
                            rk(u32le(data, p + 2)?),
                            u16le(data, p)? as usize,
                            &styles,
                            &custom,
                            epoch,
                            ctx,
                            &src,
                        );
                        insert(&mut grid, (row, col + j as u32), t, &mut bytes, ctx)?;
                    }
                    continue;
                }
                if !matches!(
                    kind,
                    0x00FD | 0x0204 | 0x00D6 | 0x0203 | 0x027E | 0x0205 | 0x0006
                ) {
                    continue;
                }
                let key = (u16le(data, 0)? as u32 + 1, u16le(data, 2)? as u32 + 1);
                let style = u16le(data, 4)? as usize;
                let value = match kind {
                    0x00FD => strings
                        .get(u32le(data, 6)? as usize)
                        .ok_or_else(|| bad("SST index outside table"))?
                        .clone(),
                    0x0204 | 0x00D6 => {
                        record_string(&rec, start + record_index + 1, 6, false, legacy)?
                    }
                    0x0203 => formatted(real(data, 6)?, style, &styles, &custom, epoch, ctx, &src),
                    0x027E => formatted(
                        rk(u32le(data, 6)?),
                        style,
                        &styles,
                        &custom,
                        epoch,
                        ctx,
                        &src,
                    ),
                    0x0205 => {
                        let v = *data.get(6).ok_or_else(|| bad("boolean/error"))?;
                        if data.get(7) == Some(&0) {
                            if v == 0 {
                                "FALSE".into()
                            } else {
                                "TRUE".into()
                            }
                        } else {
                            format!("#ERROR({v})")
                        }
                    }
                    0x0006 => {
                        ctx.diagnostic(
                            "sheet_formula_cache",
                            "Formula cached results retained; formulas are not evaluated.",
                            src.clone(),
                            false,
                        );
                        if data.get(12..14) == Some(&[255, 255]) {
                            match data.get(6) {
                                Some(0) => {
                                    pending = Some(key);
                                    String::new()
                                }
                                Some(1) => {
                                    if data.get(8) == Some(&0) {
                                        "FALSE".into()
                                    } else {
                                        "TRUE".into()
                                    }
                                }
                                Some(2) => format!("#ERROR({})", data.get(8).unwrap_or(&0)),
                                Some(3) => String::new(),
                                _ => return Err(bad("unknown formula cache type")),
                            }
                        } else {
                            formatted(real(data, 6)?, style, &styles, &custom, epoch, ctx, &src)
                        }
                    }
                    _ => unreachable!(),
                };
                insert(&mut grid, key, value, &mut bytes, ctx)?;
            }
            if !ended {
                return Err(if records_truncated {
                    Error::Limit("max_nodes")
                } else {
                    bad("unterminated worksheet substream")
                });
            }
            if pending.is_some() {
                warn(
                    ctx,
                    "xls_formula_missing_cache",
                    "String formula cache is missing.",
                    &src,
                )
            }
            let has_table = !grid.is_empty();
            if let Some(t) = table_output(
                grid,
                merges,
                &src,
                ctx,
                output.as_deref_mut(),
                Some(&mut out),
            )? {
                out.push(t)
            }
            for (owner, mut text) in object_text {
                let title = if let Some((id, 0x19)) = owner {
                    if let Some(&(row, column)) = notes.get(&id) {
                        text.source.row = Some(row);
                        text.source.cell = Some(column);
                        format!("Comment (row {row}, column {column})")
                    } else {
                        "Comment".into()
                    }
                } else {
                    "Drawing text".into()
                };
                let location = text.source.clone();
                let label = heading(title, 4, &location, ctx)?;
                out.push(ctx.block(
                    BlockContent::Quote {
                        blocks: vec![label, text],
                    },
                    location,
                )?);
            }
            completed |= output.is_some() && has_table;
            warn(ctx,"xls_formatting","Cell values and merges retained; rich text, links, print layout and advanced formatting are not reconstructed.",&src);
            Ok(())
        })();
        if let Err(e) = result {
            if ctx.recovering_limits && matches!(e, Error::Limit(_)) {
                out.truncate(checkpoint);
                if out
                    .iter()
                    .any(|b| matches!(b.content, BlockContent::Table { .. }))
                    || completed
                {
                    warn(ctx, "xls_sheets_truncated", "Retained completed worksheets before the resource limit. The current worksheet and later sheets are omitted.", &src);
                    return Ok(out);
                }
            }
            return Err(e);
        }
        if let Some(writer) = output.as_deref_mut() {
            completed |= out
                .iter()
                .any(|b| matches!(b.content, BlockContent::Table { .. }));
            for block in out.drain(..) {
                writer.block(&block)?;
            }
        }
    }
    Ok(out)
}
// XLSB record identifiers and lengths use independent little-endian 7-bit groups.
fn binary_records<'a>(b: &'a [u8], ctx: &mut Context<'_>) -> Result<Vec<(u16, &'a [u8])>, Error> {
    let mut pos = 0;
    let mut out = vec![];
    while pos < b.len() {
        ctx.node()?;
        let first = b[pos];
        pos += 1;
        let id = if first & 128 != 0 {
            let second = *b.get(pos).ok_or_else(|| bad("truncated XLSB type"))?;
            pos += 1;
            if second & 128 != 0 {
                return Err(bad("invalid XLSB record type"));
            }
            u16::from(first & 127) | (u16::from(second) << 7)
        } else {
            u16::from(first)
        };
        let mut size = 0usize;
        let mut done = false;
        for shift in (0..28).step_by(7) {
            let v = *b.get(pos).ok_or_else(|| bad("truncated XLSB size"))?;
            pos += 1;
            size |= usize::from(v & 127) << shift;
            if v & 128 == 0 {
                done = true;
                break;
            }
        }
        if !done {
            return Err(bad("oversized XLSB length"));
        }
        let end = pos
            .checked_add(size)
            .ok_or_else(|| bad("XLSB record overflow"))?;
        out.push((
            id,
            b.get(pos..end)
                .ok_or_else(|| bad("truncated XLSB record"))?,
        ));
        pos = end;
    }
    Ok(out)
}
fn wide(b: &[u8], p: &mut usize) -> Result<String, Error> {
    let len = u32le(b, *p)? as usize;
    *p += 4;
    let end = p
        .checked_add(
            len.checked_mul(2)
                .ok_or_else(|| bad("wide string overflow"))?,
        )
        .ok_or_else(|| bad("wide string overflow"))?;
    let value = utf16(b.get(*p..end).ok_or_else(|| bad("truncated wide string"))?)?;
    *p = end;
    Ok(value)
}
pub fn xlsb(bytes: &[u8], ctx: &mut Context<'_>) -> Result<Vec<Block>, Error> {
    xlsb_output(bytes, ctx, None)
}
pub(super) fn xlsb_output(
    bytes: &[u8],
    ctx: &mut Context<'_>,
    mut output: Option<&mut crate::render::Writer<'_>>,
) -> Result<Vec<Block>, Error> {
    let mut pkg = Package::new(bytes, ctx)?;
    let part = pkg.main("xl/workbook.bin", ctx)?;
    let b = pkg
        .read_bytes(&part)?
        .ok_or_else(|| bad("missing binary workbook"))?;
    let rels = pkg.rels(&part, ctx)?;
    let mut epoch = false;
    let mut sheets = vec![];
    for (kind, data) in binary_records(&b, ctx)? {
        match kind {
            153 => epoch = u32le(data, 0)? & 1 != 0,
            156 => {
                let state = u32le(data, 0)?;
                let mut p = 8;
                if u32le(data, p)? == u32::MAX {
                    return Err(bad("sheet has no relationship"));
                }
                let id = wide(data, &mut p)?;
                let name = wide(data, &mut p)?;
                sheets.push((state, id, name));
            }
            _ => {}
        }
    }
    let mut strings = vec![];
    if let Some(r) = rels
        .values()
        .find(|r| r.kind.ends_with("/sharedStrings") && !r.external)
    {
        let path = resolve(&part, &r.target)?;
        let b = pkg
            .read_bytes(&path)?
            .ok_or_else(|| bad("missing shared strings"))?;
        let mut total = 0usize;
        for (k, d) in binary_records(&b, ctx)? {
            if k == 19 {
                let s = wide(d, &mut 1)?;
                total = total.saturating_add(s.len());
                if total > ctx.options.limits.max_text_bytes {
                    return Err(Error::Limit("max_text_bytes"));
                }
                strings.push(s)
            }
        }
    }
    let mut styles = vec![];
    let mut custom = HashMap::new();
    if let Some(r) = rels
        .values()
        .find(|r| r.kind.ends_with("/styles") && !r.external)
    {
        let path = resolve(&part, &r.target)?;
        let b = pkg
            .read_bytes(&path)?
            .ok_or_else(|| bad("missing binary styles"))?;
        let mut in_xfs = false;
        for (k, d) in binary_records(&b, ctx)? {
            match k {
                617 => in_xfs = true,
                618 => in_xfs = false,
                44 => {
                    custom.insert(u16le(d, 0)? as u32, wide(d, &mut 2)?);
                }
                47 if in_xfs => styles.push(u16le(d, 2)? as u32),
                _ => {}
            }
        }
    }
    let mut out = vec![];
    let mut total = 0;
    for (i, (state, id, name)) in sheets.into_iter().enumerate() {
        if i >= ctx.options.limits.max_pages {
            return Err(Error::Limit("max_pages"));
        }
        let path = linked(&rels, &id, &part)?;
        let src = Source {
            part: Some(path.clone().into()),
            ..Default::default()
        };
        if state != 0 {
            ctx.diagnostic(
                "office_hidden_content",
                "Hidden worksheet omitted.",
                src,
                false,
            );
            continue;
        }
        let b = pkg
            .read_bytes(&path)?
            .ok_or_else(|| bad("missing binary worksheet"))?;
        let mut row = None;
        let mut grid = BTreeMap::new();
        let mut merges = vec![];
        out.push(heading(name, 2, &src, ctx)?);
        for (k, d) in binary_records(&b, ctx)? {
            if k == 0 {
                if d.len() < 12 {
                    return Err(bad("truncated binary row header"));
                }
                row = Some(
                    u32le(d, 0)?
                        .checked_add(1)
                        .ok_or_else(|| bad("row overflow"))?,
                );
                continue;
            }
            if k == 176 {
                merges.push((
                    u32le(d, 0)?
                        .checked_add(1)
                        .ok_or_else(|| bad("merge overflow"))?,
                    u32le(d, 8)?
                        .checked_add(1)
                        .ok_or_else(|| bad("merge overflow"))?,
                    u32le(d, 4)?
                        .checked_add(1)
                        .ok_or_else(|| bad("merge overflow"))?,
                    u32le(d, 12)?
                        .checked_add(1)
                        .ok_or_else(|| bad("merge overflow"))?,
                ));
                continue;
            }
            if !matches!(k, 2..=11 | 62) {
                continue;
            }
            let r = row.ok_or_else(|| bad("cell precedes row header"))?;
            let col = u32le(d, 0)?
                .checked_add(1)
                .ok_or_else(|| bad("column overflow"))?;
            let style = (u32le(d, 4)? & 0xffffff) as usize;
            let value = match k {
                2 => formatted(rk(u32le(d, 8)?), style, &styles, &custom, epoch, ctx, &src),
                5 | 9 => formatted(real(d, 8)?, style, &styles, &custom, epoch, ctx, &src),
                3 | 11 => format!("#ERROR({})", d.get(8).ok_or_else(|| bad("error cell"))?),
                4 | 10 => {
                    if d.get(8) == Some(&0) {
                        "FALSE".into()
                    } else if d.get(8) == Some(&1) {
                        "TRUE".into()
                    } else {
                        return Err(bad("boolean cell"));
                    }
                }
                6 | 8 => wide(d, &mut 8)?,
                62 => wide(d, &mut 9)?,
                7 => strings
                    .get(u32le(d, 8)? as usize)
                    .ok_or_else(|| bad("SST index outside table"))?
                    .clone(),
                _ => unreachable!(),
            };
            if matches!(k, 8..=11) {
                ctx.diagnostic(
                    "sheet_formula_cache",
                    "Formula cached results retained; formulas are not evaluated.",
                    src.clone(),
                    false,
                )
            }
            insert(&mut grid, (r, col), value, &mut total, ctx)?;
        }
        if let Some(t) = table_output(
            grid,
            merges,
            &src,
            ctx,
            output.as_deref_mut(),
            Some(&mut out),
        )? {
            out.push(t)
        }
        if let Some(writer) = output.as_deref_mut() {
            for block in out.drain(..) {
                writer.block(&block)?;
            }
        }
        warn(ctx,"xlsb_formatting","Values and merges extracted; rich styles, drawings, links and visibility metadata are not reconstructed.",&src);
    }
    Ok(out)
}

#[cfg(test)]
mod record_index_tests {
    use super::*;
    use crate::{CancellationToken, Options};

    #[test]
    fn preflight_preserves_node_budget_and_zero_padding() {
        let mut options = Options::default();
        options.limits.max_nodes = 3;
        let cancel = CancellationToken::default();
        let mut ctx = Context::new(&options, &cancel);
        ctx.node().unwrap();
        // One ordinary record, one padding marker, then ignored zero padding.
        let bytes = [10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(biff_record_count(&bytes, &ctx).unwrap(), 2);
        // Planning must not spend the extraction attempt's node budget.
        ctx.node().unwrap();
        assert!(matches!(
            biff_record_count(&bytes, &ctx),
            Err(Error::Limit("max_nodes"))
        ));
    }

    #[test]
    fn preflight_preserves_limit_and_malformed_error_order() {
        let cancel = CancellationToken::default();
        let mut options = Options::default();
        options.limits.max_nodes = 1;
        let ctx = Context::new(&options, &cancel);
        assert!(matches!(
            biff_record_count(&[10, 0, 2, 0, 1], &ctx),
            Err(Error::Malformed(_))
        ));
        // The next header exceeds the budget before its missing payload is read.
        assert!(matches!(
            biff_record_count(&[10, 0, 0, 0, 10, 0, 2, 0], &ctx),
            Err(Error::Limit("max_nodes"))
        ));
    }

    #[test]
    fn recovery_preflight_stops_at_the_existing_record_cap() {
        let cancel = CancellationToken::default();
        let mut options = Options::default();
        options.limits.max_nodes = 2;
        let mut ctx = Context::new(&options, &cancel);
        ctx.recovering_limits = true;
        assert_eq!(
            biff_record_count(&[10, 0, 0, 0, 10, 0, 2, 0], &ctx).unwrap(),
            1
        );
    }
}
