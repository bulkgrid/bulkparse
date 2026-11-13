use crate::{Block, BlockContent, Cell, Context, Error, Format, Inline, Source, TextEncoding};
use std::borrow::Cow;

fn windows1252(bytes: &[u8], ctx: &Context<'_>) -> Result<String, Error> {
    // The five undefined code points are rejected, not silently replaced.
    const C1: [char; 32] = [
        '€', '\0', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\0', 'Ž', '\0', '\0',
        '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\0', 'ž', 'Ÿ',
    ];
    let mut out = String::new();
    for &b in bytes {
        ctx.check()?;
        let c = if (0x80..=0x9f).contains(&b) {
            C1[usize::from(b - 0x80)]
        } else {
            char::from(b)
        };
        if c == '\0' || (c.is_control() && !matches!(c, '\t' | '\n' | '\r')) {
            return Err(Error::Malformed(
                "CSV contains undefined Windows-1252 bytes or binary control characters; set csv_encoding explicitly for Unicode input".into(),
            ));
        }
        if out.len().saturating_add(c.len_utf8()) > ctx.options.limits.max_text_bytes {
            return Err(Error::Limit("max_text_bytes"));
        }
        out.push(c);
    }
    Ok(out)
}

fn decode<'a>(bytes: &'a [u8], ctx: &mut Context<'_>) -> Result<Cow<'a, str>, Error> {
    let (bom, body) = if bytes.starts_with(&[0xff, 0xfe]) {
        (Some(TextEncoding::Utf16Le), &bytes[2..])
    } else if bytes.starts_with(&[0xfe, 0xff]) {
        (Some(TextEncoding::Utf16Be), &bytes[2..])
    } else if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        (Some(TextEncoding::Utf8), &bytes[3..])
    } else {
        (None, bytes)
    };
    if let (Some(actual), Some(requested)) = (bom, ctx.options.csv_encoding) {
        if actual != requested {
            return Err(Error::Malformed("CSV encoding conflicts with BOM".into()));
        }
    }
    let encoding = ctx
        .options
        .csv_encoding
        .or(bom)
        .unwrap_or(TextEncoding::Utf8);
    if encoding == TextEncoding::Utf8 {
        match super::utf8(body) {
            Ok(text) => return Ok(Cow::Borrowed(text)),
            Err(e) if bom.is_some() || ctx.options.csv_encoding.is_some() => return Err(e),
            Err(_) => {
                let decoded = windows1252(body, ctx)?;
                ctx.diagnostic(
                    "csv_encoding_inferred",
                    "Input is not UTF-8; decoded as Windows-1252. Encoding is inferred and may be ambiguous; set csv_encoding explicitly when known.",
                    Source::default(),
                    false,
                );
                return Ok(Cow::Owned(decoded));
            }
        }
    }
    if encoding == TextEncoding::Windows1252 {
        return windows1252(body, ctx).map(Cow::Owned);
    }
    if body.len() % 2 != 0 {
        return Err(Error::Malformed("truncated UTF-16 code unit".into()));
    }
    let units = body.as_chunks::<2>().0.iter().map(|p| {
        if encoding == TextEncoding::Utf16Le {
            u16::from_le_bytes([p[0], p[1]])
        } else {
            u16::from_be_bytes([p[0], p[1]])
        }
    });
    let mut decoded = String::new();
    for c in char::decode_utf16(units) {
        ctx.check()?;
        let c = c.map_err(|_| Error::Malformed("invalid UTF-16 surrogate".into()))?;
        if decoded.len().saturating_add(c.len_utf8()) > ctx.options.limits.max_text_bytes {
            return Err(Error::Limit("max_text_bytes"));
        }
        decoded.push(c);
    }
    Ok(Cow::Owned(decoded))
}

fn delimiter(text: &str, ctx: &Context<'_>) -> Result<u8, Error> {
    // Inspect complete CSV records, not physical lines: quotes may contain newlines.
    // Bound both bytes and records. Discard the possibly truncated final record.
    let sample = &text.as_bytes()[..text.len().min(64 * 1024)];
    let truncated = sample.len() < text.len();
    let mut consistent = Vec::new();
    let mut evidence = Vec::new();
    for delimiter in *b",;\t|" {
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(false)
            .flexible(true)
            .delimiter(delimiter)
            .from_reader(sample);
        let mut widths = Vec::new();
        let mut record = csv::ByteRecord::new();
        for _ in 0..32 {
            ctx.check()?;
            match reader.read_byte_record(&mut record) {
                Ok(true) if !truncated || reader.position().byte() < sample.len() as u64 => {
                    widths.push(record.len())
                }
                _ => break,
            }
        }
        if widths.iter().any(|n| *n > 1) {
            evidence.push(delimiter);
        }
        if widths.first().is_some_and(|n| *n > 1) && widths.iter().all(|n| *n == widths[0]) {
            consistent.push(delimiter);
        }
    }
    match consistent.as_slice() {
        [one] => Ok(*one),
        [] => match evidence.as_slice() {
            [one] => Ok(*one),            // The strict parser below reports inconsistent rows.
            [] if !truncated => Ok(b','), // A genuine single-column input.
            _ => Err(Error::Malformed(
                "cannot infer CSV delimiter; set csv_delimiter explicitly".into(),
            )),
        },
        _ => Err(Error::Malformed(
            "ambiguous CSV delimiter; set csv_delimiter explicitly".into(),
        )),
    }
}

pub fn parse(bytes: &[u8], format: Format, ctx: &mut Context<'_>) -> Result<Vec<Block>, Error> {
    parse_output(bytes, format, ctx, None)
}
pub(super) fn parse_output(
    bytes: &[u8],
    format: Format,
    ctx: &mut Context<'_>,
    mut output: Option<&mut crate::render::Writer<'_>>,
) -> Result<Vec<Block>, Error> {
    let text = decode(bytes, ctx)?;
    let explicit = ctx
        .options
        .csv_delimiter
        .map(|c| {
            if c.is_ascii() && !matches!(c, '\0' | '\r' | '\n' | '"') {
                Ok(c as u8)
            } else {
                Err(Error::Malformed(
                    "csv_delimiter must be an ASCII character other than NUL, newline or quote"
                        .into(),
                ))
            }
        })
        .transpose()?;
    if format == Format::Tsv && explicit.is_some_and(|c| c != b'\t') {
        return Err(Error::Malformed("TSV requires a tab delimiter".into()));
    }
    let delimiter = match explicit {
        Some(d) => d,
        None if format == Format::Tsv => b'\t',
        None => delimiter(&text, ctx)?,
    };
    {
        // Reject a doomed allocation/rendering pass before constructing a large
        // table or staged output only to discard it for recovery.
        // Apply the exact same per-cell budget order as extraction below.
        let mut budget = ctx.budget_probe();
        let mut scan = csv::ReaderBuilder::new()
            .has_headers(false)
            .flexible(false)
            .delimiter(delimiter)
            .from_reader(text.as_bytes());
        let mut record = csv::StringRecord::new();
        let mut any = false;
        while scan
            .read_record(&mut record)
            .map_err(|e| Error::Malformed(e.to_string()))?
        {
            any = true;
            for value in &record {
                budget.cell()?;
                budget.text(value)?;
                budget.node()?; // Paragraph block.
            }
        }
        if any {
            budget.node()?;
        } // Table block.
    }
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(false)
        .delimiter(delimiter)
        .from_reader(text.as_bytes());
    let mut rows = Vec::new();
    let mut count = 0usize;
    for (index, record) in reader.records().enumerate() {
        let record = record.map_err(|e| Error::Malformed(e.to_string()))?;
        let mut cells = Vec::with_capacity(record.len().min(ctx.options.limits.max_table_cells));
        for value in record.iter() {
            ctx.cell()?;
            ctx.text(value)?;
            let source = Source {
                row: Some(index as u32 + 1),
                ..Default::default()
            };
            cells.push(Cell {
                blocks: vec![ctx.block(
                    BlockContent::Paragraph {
                        inlines: vec![Inline::text(value)],
                    },
                    source.clone(),
                )?],
                col_span: 1,
                row_span: 1,
                source,
            });
        }
        if let Some(writer) = output.as_deref_mut() {
            writer.record(&cells, index, ctx.options.csv_has_headers, cells.len())?;
        } else {
            rows.push(cells);
        }
        count += 1;
    }
    if count == 0 {
        return Ok(vec![]);
    }
    let header_rows = usize::from(ctx.options.csv_has_headers);
    Ok(vec![ctx.block(
        BlockContent::Table { rows, header_rows },
        Source::default(),
    )?])
}

pub(super) fn preview(
    bytes: &[u8],
    format: Format,
    ctx: &mut Context<'_>,
) -> Result<Vec<Block>, Error> {
    preview_output(bytes, format, ctx, None)
}
pub(super) fn preview_output(
    bytes: &[u8],
    format: Format,
    ctx: &mut Context<'_>,
    mut output: Option<&mut crate::render::Writer<'_>>,
) -> Result<Vec<Block>, Error> {
    let text = decode(bytes, ctx)?;
    let d = match ctx.options.csv_delimiter {
        Some(c) if c.is_ascii() && !matches!(c, '\0' | '\r' | '\n' | '"') => c as u8,
        Some(_) => return Err(Error::Malformed("invalid csv_delimiter".into())),
        None if format == Format::Tsv => b'\t',
        None => delimiter(&text, ctx)?,
    };
    if format == Format::Tsv && d != b'\t' {
        return Err(Error::Malformed("TSV requires a tab delimiter".into()));
    }
    let mut table = ctx.block(
        BlockContent::Table {
            rows: vec![],
            header_rows: usize::from(ctx.options.csv_has_headers),
        },
        Source::default(),
    )?;
    let BlockContent::Table { rows, .. } = &mut table.content else {
        unreachable!()
    };
    let mut count = 0usize;
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(false)
        .delimiter(d)
        .from_reader(text.as_bytes());
    for (i, record) in reader.records().enumerate() {
        ctx.check()?;
        let record = record.map_err(|e| Error::Malformed(e.to_string()))?;
        let size = record.iter().map(str::len).sum::<usize>();
        if !ctx.fits(record.len().saturating_mul(2), record.len(), size) {
            ctx.diagnostic("csv_rows_truncated", &format!("Retained {} complete source records; next record exceeds the remaining output budget.", count), Source {row: Some(i as u32 + 1), ..Default::default()}, true);
            break;
        }
        let source = Source {
            row: Some(i as u32 + 1),
            ..Default::default()
        };
        let mut cells = Vec::with_capacity(record.len().min(ctx.options.limits.max_table_cells));
        for v in record.iter() {
            ctx.cell()?;
            ctx.text(v)?;
            cells.push(Cell {
                blocks: vec![ctx.block(
                    BlockContent::Paragraph {
                        inlines: vec![Inline::text(v)],
                    },
                    source.clone(),
                )?],
                col_span: 1,
                row_span: 1,
                source: source.clone(),
            });
        }
        if let Some(writer) = output.as_deref_mut() {
            writer.record(&cells, count, ctx.options.csv_has_headers, cells.len())?;
        } else {
            rows.push(cells);
        }
        count += 1;
    }
    if count == 0 {
        return Err(Error::Limit("no_content_within_recovery_budget"));
    }
    Ok(vec![table])
}
