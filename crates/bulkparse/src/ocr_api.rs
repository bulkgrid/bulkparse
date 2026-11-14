//! Portable OCR evidence transport, shared by native and browser bindings.
use crate::*;
use serde::{Deserialize, Serialize};

/// Signature detection only; decoding and pixel limits belong to the provider.
pub fn is_image(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        || bytes.starts_with(b"\xff\xd8\xff")
        || bytes.starts_with(b"GIF87a")
        || bytes.starts_with(b"GIF89a")
        || bytes.starts_with(b"II\x2a\0")
        || bytes.starts_with(b"MM\0\x2a")
        || bytes.starts_with(b"II\x2b\0")
        || bytes.starts_with(b"MM\0\x2b")
        || (bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP"))
}

pub struct ImageOcrPage {
    pub page_count: u32,
    pub output: PdfOcrOutput,
}

/// The request document contains the original encoded image, including all frames.
pub trait ImageOcrProvider {
    fn recognize_image_page(&self, request: PdfOcrRequest<'_>) -> Result<ImageOcrPage, Error>;
}

impl Parser {
    #[cfg(feature = "pdf")]
    pub fn parse_image_with_ocr(
        &self,
        bytes: &[u8],
        cancel: &CancellationToken,
        provider: &dyn ImageOcrProvider,
        budget: PdfOcrBudget,
    ) -> Result<ParseResult, Error> {
        if bytes.len() > self.options.limits.max_input_bytes {
            return Err(Error::Limit("max_input_bytes"));
        }
        if !is_image(bytes) || self.options.format.is_some_and(|f| f != Format::Image) {
            return Err(Error::Malformed(
                "expected a supported image signature".into(),
            ));
        }
        if budget.max_pages == 0
            || budget.max_output_bytes == 0
            || self.options.limits.max_pages == 0
        {
            return Err(Error::Limit("image_ocr_budget"));
        }
        let mut ctx = Context::new(&self.options, cancel);
        let mut remaining = budget
            .max_output_bytes
            .min(self.options.limits.max_text_bytes);
        let mut blocks = vec![];
        let mut page = 1;
        let mut page_count = None;
        loop {
            ctx.check()?;
            let recognized = provider.recognize_image_page(PdfOcrRequest {
                document: bytes,
                page,
                remaining_output_bytes: remaining,
                cancellation: cancel,
            })?;
            if recognized.page_count == 0 || page_count.is_some_and(|n| n != recognized.page_count)
            {
                return Err(Error::Malformed("inconsistent image frame count".into()));
            }
            let count = recognized.page_count;
            if count as usize > self.options.limits.max_pages {
                return Err(Error::Limit("max_pages"));
            }
            page_count = Some(count);
            let source = Source {
                page: Some(page),
                ..Source::default()
            };
            remaining -= recognized.output.validate(remaining)?;
            let before = ctx.diagnostics.len();
            let page_blocks = crate::ocr::blocks(recognized.output, &mut ctx, source.clone())?;
            if page_blocks.is_empty() {
                ctx.diagnostic(
                    "image_ocr_empty",
                    "OCR returned no text for this image frame.",
                    source.clone(),
                    true,
                );
            }
            ctx.coverage.push(Coverage {
                source,
                status: if ctx.diagnostics[before..].iter().any(|d| d.content_loss) {
                    Status::Partial
                } else {
                    Status::Complete
                },
            });
            blocks.extend(page_blocks);
            if page == count {
                break;
            }
            page += 1;
            if page as usize > budget.max_pages || remaining == 0 {
                for skipped in page..=count {
                    ctx.coverage.push(Coverage {
                        source: Source {
                            page: Some(skipped),
                            ..Source::default()
                        },
                        status: Status::Partial,
                    });
                }
                ctx.diagnostic(
                    "image_ocr_budget_exhausted",
                    "OCR budget exhausted; remaining image frames were not recognized.",
                    Source {
                        page: Some(page),
                        ..Source::default()
                    },
                    true,
                );
                break;
            }
        }
        ctx.check()?;
        let status = if ctx.diagnostics.iter().any(|d| d.content_loss) {
            Status::Partial
        } else {
            Status::Complete
        };
        if self.options.strict && status == Status::Partial {
            return Err(Error::Incomplete(ctx.diagnostics.len()));
        }
        Ok(ParseResult {
            document: Document {
                schema_version: SCHEMA_VERSION.into(),
                format: Format::Image,
                blocks,
                assets: vec![],
            },
            pdf_inspection: None,
            status,
            diagnostics: ctx.diagnostics,
            coverage: ctx.coverage,
            parser_version: env!("CARGO_PKG_VERSION").into(),
        })
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OcrPage {
    pub page: u32,
    pub output: PdfOcrOutput,
}

/// Host-supplied evidence. Recognition happens outside WASM; validation and
/// assembly use exactly the same Rust code as native OCR.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OcrPages {
    pub page_count: u32,
    pub pages: Vec<OcrPage>,
}
impl OcrPages {
    fn output(&self, request: PdfOcrRequest<'_>) -> Result<PdfOcrOutput, Error> {
        if request.cancellation.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let output = &self
            .pages
            .iter()
            .find(|p| p.page == request.page)
            .ok_or_else(|| Error::Unsupported("OCR evidence missing for requested page".into()))?
            .output;
        output.validate(request.remaining_output_bytes)?;
        Ok(output.clone())
    }
}
impl PdfOcrProvider for OcrPages {
    fn recognize(&self, request: PdfOcrRequest<'_>) -> Result<String, Error> {
        self.output(request).map(PdfOcrOutput::into_text)
    }
    fn recognize_page(&self, request: PdfOcrRequest<'_>) -> Result<PdfOcrOutput, Error> {
        self.output(request)
    }
}
impl ImageOcrProvider for OcrPages {
    fn recognize_image_page(&self, request: PdfOcrRequest<'_>) -> Result<ImageOcrPage, Error> {
        Ok(ImageOcrPage {
            page_count: self.page_count,
            output: self.output(request)?,
        })
    }
}

#[cfg(feature = "pdf")]
pub fn parse_with_ocr_json(
    bytes: &[u8],
    filename: Option<&str>,
    options: Option<&str>,
    evidence: &str,
    budget: Option<&str>,
) -> Result<String, Error> {
    let options: Options = options
        .map(serde_json::from_str)
        .transpose()
        .map_err(|e| Error::Malformed(format!("options: {e}")))?
        .unwrap_or_default();
    if bytes.len() > options.limits.max_input_bytes {
        return Err(Error::Limit("max_input_bytes"));
    }
    if evidence.len() > 64 * 1024 * 1024 {
        return Err(Error::Limit("ocr_evidence_bytes"));
    }
    let pages: OcrPages = serde_json::from_str(evidence)
        .map_err(|e| Error::Malformed(format!("OCR evidence: {e}")))?;
    let budget: PdfOcrBudget = budget
        .map(serde_json::from_str)
        .transpose()
        .map_err(|e| Error::Malformed(format!("OCR budget: {e}")))?
        .unwrap_or_default();
    if pages.page_count == 0
        || pages.page_count as usize > options.limits.max_pages
        || pages.pages.len() > pages.page_count as usize
    {
        return Err(Error::Limit("max_pages"));
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut remaining = budget.max_output_bytes.min(options.limits.max_text_bytes);
    for page in &pages.pages {
        if page.page == 0 || page.page > pages.page_count || !seen.insert(page.page) {
            return Err(Error::Malformed("invalid or duplicate OCR page".into()));
        }
        remaining -= page.output.validate(remaining)?;
    }
    let parser = Parser::new(options);
    let token = CancellationToken::default();
    let result = if is_image(bytes) {
        parser.parse_image_with_ocr(bytes, &token, &pages, budget)?
    } else {
        parser.parse_with_pdf_ocr(bytes, filename, &token, &pages, budget)?
    };
    if result
        .pdf_inspection
        .as_ref()
        .is_some_and(|i| i.page_count != pages.page_count as usize)
    {
        return Err(Error::Malformed("OCR page count differs from PDF".into()));
    }
    conversion_json(result, parser.options.strict)
}

pub fn conversion_json(result: ParseResult, strict: bool) -> Result<String, Error> {
    let rendered = result.to_markdown();
    if strict && rendered.diagnostics.iter().any(|d| d.content_loss) {
        return Err(Error::Incomplete(rendered.diagnostics.len()));
    }
    serde_json::to_string(&Conversion {
        result,
        markdown: rendered.text,
        render_diagnostics: rendered.diagnostics,
    })
    .map_err(|e| Error::Malformed(e.to_string()))
}
