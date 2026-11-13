//! Bounded semantic OCR output. Providers infer structure; the core validates it
//! and projects it into the same document model as native extraction.
use crate::Error;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const MAX_REGIONS: usize = 1024;
const MAX_GRID: usize = 16_384;
const MAX_SIDE: usize = 256;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PdfOcrRegion {
    pub order: u32,
    /// Rendered pixels: left, top, right, bottom.
    pub bbox: [f64; 4],
    pub label: String,
    #[serde(default)]
    pub confidence: Option<f64>,
    pub content: PdfOcrRegionContent,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum PdfOcrRegionContent {
    Text {
        text: String,
    },
    Heading {
        level: u8,
        text: String,
    },
    Table {
        rows: Vec<Vec<PdfOcrCell>>,
        header_rows: usize,
    },
    Formula {
        tex: String,
    },
    /// A detected visual region; no image asset or visual interpretation promised.
    Figure {
        description: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PdfOcrCell {
    pub text: String,
    pub col_span: u32,
    pub row_span: u32,
}

impl PdfOcrRegionContent {
    pub fn texts(&self) -> Vec<&str> {
        match self {
            Self::Text { text } | Self::Heading { text, .. } => vec![text],
            Self::Formula { tex } => vec![tex],
            Self::Figure { description } => vec![description],
            Self::Table { rows, .. } => rows.iter().flatten().map(|c| c.text.as_str()).collect(),
        }
    }
    pub(super) fn into_texts(self) -> Vec<String> {
        match self {
            Self::Text { text } | Self::Heading { text, .. } => vec![text],
            Self::Formula { tex } => vec![tex],
            Self::Figure { description } => vec![description],
            Self::Table { rows, .. } => rows.into_iter().flatten().map(|c| c.text).collect(),
        }
    }
}

/// Find logical cell columns, including prior row spans. Reject overlaps,
/// missing cells, dangling spans and expansion bombs before allocating blocks.
fn columns(rows: &[Vec<PdfOcrCell>], header_rows: usize) -> Result<Vec<Vec<usize>>, Error> {
    if rows.is_empty() || rows.len() > MAX_SIDE || header_rows > rows.len() {
        return Err(Error::Malformed("invalid OCR table dimensions".into()));
    }
    let width = rows[0]
        .iter()
        .try_fold(0usize, |n, c| n.checked_add(c.col_span as usize))
        .ok_or(Error::Limit("pdf_ocr_table_cells"))?;
    if width == 0 {
        return Err(Error::Malformed("empty OCR table width".into()));
    }
    if width > MAX_SIDE || width * rows.len() > MAX_GRID {
        return Err(Error::Limit("pdf_ocr_table_cells"));
    }
    let mut occupied = vec![vec![false; width]; rows.len()];
    let mut positions = Vec::with_capacity(rows.len());
    for (r, row) in rows.iter().enumerate() {
        let mut col = 0;
        let mut positions_row = vec![];
        for cell in row {
            while col < width && occupied[r][col] {
                col += 1;
            }
            let (w, h) = (cell.col_span as usize, cell.row_span as usize);
            if w == 0 || h == 0 || w > width.saturating_sub(col) || h > rows.len() - r {
                return Err(Error::Malformed("invalid OCR table span".into()));
            }
            for occupied_row in occupied.iter_mut().skip(r).take(h) {
                for used in &mut occupied_row[col..col + w] {
                    if *used {
                        return Err(Error::Malformed("overlapping OCR table cells".into()));
                    }
                    *used = true;
                }
            }
            positions_row.push(col);
            col += w;
        }
        if occupied[r].iter().any(|used| !used) {
            return Err(Error::Malformed("incomplete OCR table grid".into()));
        }
        positions.push(positions_row);
    }
    Ok(positions)
}

pub(super) fn validate(image_size: [u32; 2], regions: &[PdfOcrRegion]) -> Result<usize, Error> {
    if image_size.contains(&0) || image_size.iter().any(|v| *v > 100_000) {
        return Err(Error::Malformed("invalid OCR image dimensions".into()));
    }
    if regions.len() > MAX_REGIONS {
        return Err(Error::Limit("pdf_ocr_regions"));
    }
    let mut orders = BTreeSet::new();
    let mut bytes = 0usize;
    let mut cells = 0usize;
    for region in regions {
        let [l, t, r, b] = region.bbox;
        if region.bbox.iter().any(|v| !v.is_finite())
            || l < 0.0
            || t < 0.0
            || r <= l
            || b <= t
            || r > image_size[0] as f64
            || b > image_size[1] as f64
            || region.label.is_empty()
            || region.label.len() > 64
            || region.label.chars().any(char::is_control)
            || !orders.insert(region.order)
            || region
                .confidence
                .is_some_and(|v| !v.is_finite() || !(0.0..=1.0).contains(&v))
        {
            return Err(Error::Malformed("invalid OCR region evidence".into()));
        }
        match &region.content {
            PdfOcrRegionContent::Heading { level, .. } if !(1..=6).contains(level) => {
                return Err(Error::Malformed("invalid OCR heading level".into()));
            }
            PdfOcrRegionContent::Table { rows, header_rows } => {
                columns(rows, *header_rows)?;
                let width: usize = rows[0].iter().map(|c| c.col_span as usize).sum();
                cells += width * rows.len();
                if cells > MAX_GRID {
                    return Err(Error::Limit("pdf_ocr_table_cells"));
                }
            }
            _ => (),
        }
        for text in region.content.texts() {
            // A separator per region/cell also bounds empty tables and figures.
            bytes = bytes
                .checked_add(text.len())
                .and_then(|n| n.checked_add(1))
                .ok_or(Error::Limit("pdf_ocr_output_bytes"))?;
        }
    }
    Ok(bytes)
}

#[cfg(feature = "pdf")]
pub(super) fn blocks(
    image_size: [u32; 2],
    mut regions: Vec<PdfOcrRegion>,
    ctx: &mut crate::Context<'_>,
    source: crate::Source,
) -> Result<Vec<crate::Block>, Error> {
    use crate::{BlockContent, Cell, Inline, PdfRegionSource};
    regions.sort_by_key(|r| r.order);
    let mut blocks = vec![];
    for region in regions {
        ctx.check()?;
        let mut located = source.clone();
        located.pdf_region = Some(Box::new(PdfRegionSource {
            image_size,
            bbox: region.bbox,
            order: region.order,
            label: region.label,
            confidence: region.confidence,
        }));
        for text in region.content.texts() {
            ctx.text(text)?;
            ctx.text("\n")?;
        }
        let content = match region.content {
            PdfOcrRegionContent::Text { text } => BlockContent::Paragraph {
                inlines: vec![Inline::text(text)],
            },
            PdfOcrRegionContent::Heading { level, text } => BlockContent::Heading {
                level,
                inlines: vec![Inline::text(text)],
            },
            PdfOcrRegionContent::Formula { tex } => BlockContent::Paragraph {
                inlines: vec![Inline::Math { tex, display: true }],
            },
            PdfOcrRegionContent::Figure { description } => {
                ctx.diagnostic("pdf_ocr_figure_unavailable", "Detected figure retained as a region; image assets and visual content are not extracted.", located.clone(), true);
                BlockContent::Raw {
                    media_type: "text/plain".into(),
                    text: description,
                }
            }
            PdfOcrRegionContent::Table { rows, header_rows } => {
                let positions = columns(&rows, header_rows)?;
                let mut table = Vec::with_capacity(rows.len());
                for (r, row) in rows.into_iter().enumerate() {
                    let mut cells = vec![];
                    for (i, cell) in row.into_iter().enumerate() {
                        // Charge expanded area too, not just anchor cells.
                        for _ in 0..cell.col_span * cell.row_span {
                            ctx.cell()?;
                        }
                        let mut cell_source = located.clone();
                        cell_source.row = Some(r as u32 + 1);
                        cell_source.cell = Some(positions[r][i] as u32 + 1);
                        let paragraph = ctx.block(
                            BlockContent::Paragraph {
                                inlines: vec![Inline::text(cell.text)],
                            },
                            cell_source.clone(),
                        )?;
                        cells.push(Cell {
                            blocks: vec![paragraph],
                            col_span: cell.col_span,
                            row_span: cell.row_span,
                            source: cell_source,
                        });
                    }
                    table.push(cells);
                }
                BlockContent::Table {
                    rows: table,
                    header_rows,
                }
            }
        };
        blocks.push(ctx.block(content, located)?);
    }
    if !blocks.is_empty() {
        ctx.diagnostic("pdf_ocr_structure_inferred", "Semantic regions, reading order, tables and formulas supplied by the configured model; recognition and completeness remain unverified.", source, true);
    }
    Ok(blocks)
}
