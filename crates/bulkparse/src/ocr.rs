//! Opt-in OCR boundary. Providers own rendering and inference; the portable core
//! neither downloads models nor spawns processes. Calls are synchronous.
use crate::{CancellationToken, Error};
use serde::{Deserialize, Serialize};
#[cfg(any(feature = "pdf", test))]
mod layout;
mod regions;
pub use regions::{PdfOcrCell, PdfOcrRegion, PdfOcrRegionContent};

/// A recognized line. Geometry uses rendered-page pixel coordinates, not PDF points.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PdfOcrLine {
    pub text: String,
    pub quad: [[f64; 2]; 4],
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum PdfOcrOutput {
    Text(String),
    /// Semantic regions in provider reading order, validated before assembly.
    Regions {
        image_size: [u32; 2],
        regions: Vec<PdfOcrRegion>,
    },
    Lines {
        image_size: [u32; 2],
        lines: Vec<PdfOcrLine>,
    },
}

impl PdfOcrOutput {
    /// Validate before using geometry or accepting text. The budget includes one
    /// newline between lines. Geometry is separately limited to 4096 lines/page.
    pub fn validate(&self, limit: usize) -> Result<usize, Error> {
        let bytes = match self {
            Self::Text(text) => text.len(),
            Self::Regions {
                image_size,
                regions,
            } => regions::validate(*image_size, regions)?,
            Self::Lines { image_size, lines } => {
                if image_size.contains(&0) || image_size.iter().any(|v| *v > 100_000) {
                    return Err(Error::Malformed("invalid OCR image dimensions".into()));
                }
                if lines.len() > 4096 {
                    return Err(Error::Limit("pdf_ocr_lines"));
                }
                let mut bytes = lines.len().saturating_sub(1);
                for line in lines {
                    if line.text.trim().is_empty()
                        || line.text.contains(['\n', '\r'])
                        || !line.confidence.is_finite()
                        || !(0.0..=1.0).contains(&line.confidence)
                        || line.quad.iter().any(|p| {
                            p.iter().enumerate().any(|(axis, v)| {
                                !v.is_finite() || *v < 0.0 || *v > image_size[axis] as f64
                            })
                        })
                    {
                        return Err(Error::Malformed("invalid OCR line evidence".into()));
                    }
                    // Require a non-degenerate convex perimeter in either winding.
                    let mut sign = 0.0_f64;
                    for i in 0..4 {
                        let [a, b, c] =
                            [line.quad[i], line.quad[(i + 1) % 4], line.quad[(i + 2) % 4]];
                        let cross = (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0]);
                        if cross == 0.0 || (sign != 0.0 && cross.signum() != sign) {
                            return Err(Error::Malformed("invalid OCR quadrilateral".into()));
                        }
                        sign = cross.signum();
                    }
                    bytes = bytes
                        .checked_add(line.text.len())
                        .ok_or(Error::Limit("pdf_ocr_output_bytes"))?;
                }
                bytes
            }
        };
        if bytes > limit {
            return Err(Error::Limit("pdf_ocr_output_bytes"));
        }
        Ok(bytes)
    }

    /// Plain engine order for compatibility. Core parsing uses geometry instead.
    pub fn into_text(self) -> String {
        match self {
            Self::Text(text) => text,
            Self::Regions { mut regions, .. } => {
                regions.sort_by_key(|r| r.order);
                regions
                    .into_iter()
                    .flat_map(|r| r.content.into_texts())
                    .collect::<Vec<_>>()
                    .join("\n")
            }
            Self::Lines { lines, .. } => lines
                .into_iter()
                .map(|l| l.text)
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }
}

#[cfg(feature = "pdf")]
pub(crate) fn blocks(
    output: PdfOcrOutput,
    ctx: &mut crate::Context<'_>,
    source: crate::Source,
) -> Result<Vec<crate::Block>, Error> {
    match output {
        PdfOcrOutput::Text(text) => crate::formats::text::parse(&text, ctx, source),
        PdfOcrOutput::Regions {
            image_size,
            regions,
        } => regions::blocks(image_size, regions, ctx, source),
        PdfOcrOutput::Lines { image_size, lines } => {
            let mut blocks = Vec::with_capacity(lines.len());
            for line in layout::order(lines, ctx.cancel)? {
                ctx.check()?;
                if !blocks.is_empty() {
                    ctx.text("\n")?;
                }
                ctx.text(&line.text)?;
                let mut located = source.clone();
                located.ocr = Some(Box::new(crate::OcrSource {
                    image_size,
                    quad: line.quad,
                    confidence: line.confidence,
                }));
                blocks.push(ctx.block(
                    crate::BlockContent::Paragraph {
                        inlines: vec![crate::Inline::text(line.text)],
                    },
                    located,
                )?);
            }
            if !blocks.is_empty() {
                ctx.diagnostic("pdf_ocr_layout_inferred", "OCR lines ordered using whitespace and left-to-right column heuristics; paragraphs, tables and reading direction remain unverified.", source, true);
            }
            Ok(blocks)
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PdfOcrBudget {
    /// Maximum inspector-routed pages submitted, including failed calls.
    pub max_pages: usize,
    /// Maximum aggregate UTF-8 bytes accepted from this provider per document.
    pub max_output_bytes: usize,
}
impl Default for PdfOcrBudget {
    fn default() -> Self {
        Self {
            max_pages: 8,
            max_output_bytes: 1024 * 1024,
        }
    }
}
pub struct PdfOcrRequest<'a> {
    /// Original PDF bytes. The provider must render only the requested page.
    pub document: &'a [u8],
    /// One-based page number.
    pub page: u32,
    pub remaining_output_bytes: usize,
    pub cancellation: &'a CancellationToken,
}
/// A provider must enforce its own time, image-pixel and memory budgets and
/// cooperate with cancellation. The core cannot preempt an in-process callback.
/// Return plain text in logical Unicode order, not Markdown or HTML.
pub trait PdfOcrProvider {
    fn recognize(&self, request: PdfOcrRequest<'_>) -> Result<String, Error>;
    /// Override to preserve line geometry. Legacy providers need no changes.
    fn recognize_page(&self, request: PdfOcrRequest<'_>) -> Result<PdfOcrOutput, Error> {
        self.recognize(request).map(PdfOcrOutput::Text)
    }
}
