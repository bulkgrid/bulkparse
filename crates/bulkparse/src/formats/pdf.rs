use crate::{Block, BlockContent, Cell, Context, Coverage, Error, Source, Status};
mod aligned_table;
mod cmap;
mod content;
mod fonts;
mod grid;
mod hierarchical;
mod inspection;
mod layout;
mod positioned;
mod prose_table;
mod rules;
mod sparse;
mod standard_fonts;
mod tables;
mod unicode;
fn malformed(error: impl std::fmt::Display) -> Error {
    Error::Malformed(error.to_string())
}
fn page_blocks(
    mut page: positioned::Page,
    source: Source,
    ctx: &mut Context<'_>,
) -> Result<Vec<Block>, Error> {
    // Single-page exports can retain entire neighbouring pages outside the
    // inherited CropBox/MediaBox. They are not visible reading-order content.
    // Keep overlapping and rotated glyphs conservatively; inspection still
    // sees the original stream before this presentation-only filtering.
    if let Some([left, bottom, right, top]) = page.image_coverage.page_box {
        let mut clipped = 0usize;
        page.glyphs.retain(|g| {
            let outside = g.rotation == 0
                && (g.end.max(g.x) < left - 1.
                    || g.x.min(g.end) > right + 1.
                    || g.y + g.em < bottom
                    || g.y - g.em * 0.3 > top);
            if outside {
                clipped += g.text.chars().filter(|c| !c.is_whitespace()).count();
            }
            !outside
        });
        if clipped > 0 {
            ctx.diagnostic(
                "pdf_offpage_text",
                &format!(
                    "Omitted {clipped} non-whitespace characters wholly outside the page box."
                ),
                source.clone(),
                false,
            );
        }
    }
    if ctx.options.pdf_normalize_unicode {
        let mut changed = false;
        let mut bytes = 0usize;
        for glyph in &mut page.glyphs {
            ctx.check()?;
            changed |= unicode::normalize(
                glyph,
                ctx.options.limits.max_text_bytes.saturating_sub(bytes),
            )?;
            bytes = bytes.saturating_add(glyph.text.len());
            if bytes > ctx.options.limits.max_text_bytes {
                return Err(Error::Limit("max_text_bytes"));
            }
        }
        if changed {
            ctx.diagnostic("pdf_unicode_normalized", "Expanded Arabic presentation forms and alphabetic ligatures. ActualText and other characters are preserved; disable pdf_normalize_unicode for original mapped glyph forms.",source.clone(),false);
        }
    }
    if page.approximate {
        ctx.diagnostic(
            "pdf_approximate_metrics",
            "Some font widths are unavailable; spacing uses approximate metrics.",
            source.clone(),
            true,
        );
    }
    if page.unmapped {
        ctx.diagnostic(
            "pdf_unmapped_characters",
            "Some character codes have no valid Unicode mapping.",
            source.clone(),
            true,
        );
    }
    if page.rotated {
        ctx.diagnostic(
            "pdf_rotated_text",
            "Quarter-turn text is grouped in its own orientation; cross-orientation reading order is uncertain.",
            source.clone(),
            true,
        );
    }
    let (tables, used) = tables::extract(&page.glyphs, page.rules, ctx)?;
    let glyphs = page
        .glyphs
        .into_iter()
        .enumerate()
        .filter_map(|(i, g)| (!used[i]).then_some(g))
        .collect();
    let pieces = layout::arrange(glyphs, &tables, &page.image_coverage, ctx)?;
    let mut blocks = Vec::new();
    for piece in pieces {
        match piece {
            layout::Piece::Heading(level, text) => {
                ctx.text(&text)?;
                blocks.push(ctx.block(
                    BlockContent::Heading {
                        level,
                        inlines: vec![crate::Inline::text(text)],
                    },
                    source.clone(),
                )?);
            }
            layout::Piece::Text(text) => {
                blocks.extend(super::text::parse(&text, ctx, source.clone())?)
            }
            layout::Piece::Table(index) => {
                let table = &tables[index];
                let mut rows = Vec::new();
                for (r, row) in table.rows.iter().enumerate() {
                    let mut cells = Vec::new();
                    for cell in row {
                        let cell_source = Source {
                            row: Some(r as u32 + 1),
                            cell: Some(cell.column as u32 + 1),
                            ..source.clone()
                        };
                        cells.push(Cell {
                            blocks: super::text::parse(&cell.text, ctx, cell_source.clone())?,
                            col_span: cell.col_span,
                            row_span: cell.row_span,
                            source: cell_source,
                        });
                    }
                    rows.push(cells);
                }
                blocks.push(ctx.block(
                    BlockContent::Table {
                        rows,
                        header_rows: 0,
                    },
                    source.clone(),
                )?);
                ctx.diagnostic("pdf_table_inferred", "Inferred a table from painted boundaries, aligned data or repeated styled rows. Merged cells use rectangular boundaries or aligned header/label evidence; semantic headers are not inferred.",source.clone(),true);
            }
        }
    }
    Ok(blocks)
}

fn load(
    bytes: &[u8],
    ctx: &Context<'_>,
) -> Result<
    (
        lopdf::Document,
        std::collections::BTreeMap<u32, lopdf::ObjectId>,
    ),
    Error,
> {
    ctx.check()?;
    let document = lopdf::Document::load_mem(bytes).map_err(malformed)?;
    ctx.check()?;
    if document.is_encrypted() {
        return Err(Error::Encrypted);
    }
    let pages = document.get_pages();
    if pages.len() > ctx.options.limits.max_pages {
        return Err(Error::Limit("max_pages"));
    }
    if pages.is_empty() {
        return Err(malformed("PDF contains no pages"));
    }
    Ok((document, pages))
}

pub(crate) fn inspect(bytes: &[u8], ctx: &mut Context<'_>) -> Result<crate::PdfInspection, Error> {
    let (document, pages) = load(bytes, ctx)?;
    let mut inspected = Vec::with_capacity(pages.len());
    let mut text_bytes = 0usize;
    for (number, id) in pages {
        let source = Source {
            page: Some(number),
            ..Default::default()
        };
        match positioned::extract(&document, id, source, ctx) {
            Ok(page) => {
                // Inspection doesn't build blocks, but its aggregate decoded
                // text must obey the same document limit as conversion.
                for glyph in &page.glyphs {
                    text_bytes = text_bytes.saturating_add(glyph.text.len());
                }
                if text_bytes > ctx.options.limits.max_text_bytes {
                    return Err(Error::Limit("max_text_bytes"));
                }
                inspected.push(inspection::classify(&page, number, ctx)?);
            }
            Err(error @ (Error::Limit(_) | Error::Cancelled)) => return Err(error),
            Err(_) => inspected.push(inspection::failed(number)),
        }
    }
    ctx.check()?;
    Ok(inspection::report(inspected))
}

pub fn parse(bytes: &[u8], ctx: &mut Context<'_>) -> Result<Vec<Block>, Error> {
    use crate::{PdfOcrStatus as OcrStatus, PdfRoute};
    let (document, pages) = load(bytes, ctx)?;
    ctx.diagnostic("pdf_text_only", "Experimental positioned native text and geometric table extraction. Images and complex layouts are not fully reconstructed; complete visual-content coverage cannot be verified.", Source::default(), true);
    let mut blocks = Vec::new();
    let mut inspections = Vec::with_capacity(pages.len());
    let (mut ocr_pages, mut ocr_bytes) = (0usize, 0usize);
    for (number, id) in pages {
        ctx.check()?;
        let source = Source {
            page: Some(number),
            ..Default::default()
        };
        match positioned::extract(&document, id, source.clone(), ctx) {
            Ok(page) => {
                let mut inspected = inspection::classify(&page, number, ctx)?;
                let mut native = match page_blocks(page, source.clone(), ctx) {
                    Ok(blocks) => blocks,
                    Err(error @ (Error::Limit(_) | Error::Cancelled)) => return Err(error),
                    Err(error) => {
                        ctx.diagnostic(
                            "pdf_layout_error",
                            &error.to_string(),
                            source.clone(),
                            true,
                        );
                        Vec::new()
                    }
                };
                if inspected.route == PdfRoute::Ocr {
                    ctx.diagnostic(
                        "pdf_ocr_recommended",
                        &format!("Page inspection recommends OCR: {:?}.", inspected.reasons),
                        source.clone(),
                        true,
                    );
                    if let Some((provider, budget)) = ctx.ocr {
                        if ocr_pages < budget.max_pages && ocr_bytes < budget.max_output_bytes {
                            ocr_pages += 1;
                            let remaining = (budget.max_output_bytes - ocr_bytes).min(
                                ctx.options
                                    .limits
                                    .max_text_bytes
                                    .saturating_sub(ctx.text_bytes),
                            );
                            ctx.check()?;
                            let result = provider
                                .recognize_page(crate::PdfOcrRequest {
                                    document: bytes,
                                    page: number,
                                    remaining_output_bytes: remaining,
                                    cancellation: ctx.cancel,
                                })
                                .and_then(|output| {
                                    let bytes = output.validate(remaining)?;
                                    Ok((output, bytes))
                                });
                            ctx.check()?;
                            match result {
                                Ok((recognized, size)) => {
                                    ocr_bytes += size;
                                    let usable = match &recognized {
                                        crate::PdfOcrOutput::Text(text) => {
                                            usable_ocr_text(std::iter::once(text.as_str()))
                                        }
                                        crate::PdfOcrOutput::Lines { lines, .. } => {
                                            usable_ocr_text(lines.iter().map(|l| l.text.as_str()))
                                        }
                                        crate::PdfOcrOutput::Regions { regions, .. } => {
                                            usable_ocr_text(
                                                regions.iter().flat_map(|r| r.content.texts()),
                                            )
                                        }
                                    };
                                    if !usable {
                                        inspected.ocr_status = OcrStatus::Failed;
                                        ctx.diagnostic("pdf_ocr_rejected", "OCR returned predominantly invalid Unicode text; native evidence is retained.", source.clone(), true);
                                    }
                                    let recognized = if usable {
                                        crate::ocr::blocks(recognized, ctx, source.clone())?
                                    } else {
                                        Vec::new()
                                    };
                                    if recognized.is_empty() && usable {
                                        inspected.ocr_status = OcrStatus::Empty;
                                        ctx.diagnostic("pdf_ocr_empty", "OCR returned no usable text; native text is retained. This does not prove the page is blank.", source.clone(), true);
                                    } else if usable {
                                        inspected.ocr_status = OcrStatus::Applied;
                                        // Replace the whole page, never append duplicated native text.
                                        native = recognized;
                                        ctx.diagnostic("pdf_ocr_applied", "Text supplied by the configured OCR provider; accuracy and layout remain unverified.", source.clone(), true);
                                    }
                                }
                                Err(error @ (Error::Limit(_) | Error::Cancelled)) => {
                                    return Err(error)
                                }
                                Err(error) => {
                                    inspected.ocr_status = OcrStatus::Failed;
                                    ctx.diagnostic(
                                        "pdf_ocr_error",
                                        &error.to_string(),
                                        source.clone(),
                                        true,
                                    );
                                }
                            }
                        } else {
                            inspected.ocr_status = OcrStatus::BudgetExhausted;
                            ctx.diagnostic(
                                "pdf_ocr_budget",
                                "The document OCR budget is exhausted; native text is retained.",
                                source.clone(),
                                true,
                            );
                        }
                    } else {
                        ctx.diagnostic("pdf_ocr_needed", "Page inspection recommends OCR. Configure an optional provider; available native text is retained.", source.clone(), true);
                    }
                }
                if native.is_empty() {
                    ctx.diagnostic(
                        "pdf_no_text",
                        "Page has no extracted text; it may need OCR or review.",
                        source.clone(),
                        true,
                    );
                }
                blocks.extend(native);
                inspections.push(inspected);
            }
            Err(error @ (Error::Limit(_) | Error::Cancelled)) => return Err(error),
            Err(error) => {
                ctx.diagnostic("pdf_page_error", &error.to_string(), source.clone(), true);
                inspections.push(inspection::failed(number));
            }
        }
        ctx.coverage.push(Coverage {
            source,
            status: Status::Partial,
        });
    }
    ctx.pdf_inspection = Some(inspection::report(inspections));
    Ok(blocks)
}

fn usable_ocr_text<'a>(texts: impl Iterator<Item = &'a str>) -> bool {
    let (mut total, mut bad) = (0usize, 0usize);
    for c in texts.flat_map(str::chars).filter(|c| !c.is_whitespace()) {
        total += 1;
        bad += usize::from(inspection::suspicious(c));
    }
    !(bad > 0 && (bad == total || (bad >= 3 && bad * 5 >= total)))
}
