//! Markdown-only native output. A small memory spool spills to a temporary file
//! so parse errors, strict rejection and recovery cannot publish stale prefixes.
use crate::{
    formats, render, CancellationToken, Context, Coverage, Diagnostic, Error, Format, Parser,
    PdfInspection, Source, Status,
};
use serde::{Deserialize, Serialize};
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

#[derive(Debug, Serialize, Deserialize)]
pub struct MarkdownSummary {
    /// Saved files only; empty for writers without an image export directory.
    #[serde(default)]
    pub images: Vec<ExportedImage>,
    #[serde(default)]
    pub image_count: usize,
    pub format: Format,
    /// Parsing status; render_diagnostics also describes any rendering loss.
    pub status: Status,
    pub diagnostics: Vec<Diagnostic>,
    pub render_diagnostics: Vec<Diagnostic>,
    pub coverage: Vec<Coverage>,
    pub pdf_inspection: Option<PdfInspection>,
    pub parser_version: String,
    pub bytes_written: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExportedImage {
    pub id: String,
    pub media_type: String,
    pub source: Source,
    pub path: String,
    pub bytes: usize,
}

fn attempt(
    bytes: &[u8],
    format: Format,
    ctx: &mut Context<'_>,
    spool: &mut tempfile::SpooledTempFile,
    recovery: bool,
) -> Result<Vec<Diagnostic>, Error> {
    crate::assets::prepare(format, bytes, ctx)?;
    let mut bounded = BoundedOutput {
        output: spool,
        remaining: ctx.options.limits.max_markdown_bytes,
        exceeded: false,
    };
    let result = (|| {
        let mut buffered = BufWriter::with_capacity(64 * 1024, &mut bounded);
        let mut output = render::Writer::new(&mut buffered);
        formats::write(format, bytes, ctx, &mut output, recovery)?;
        let diagnostics = output.diagnostics;
        buffered.flush()?;
        Ok(diagnostics)
    })();
    if bounded.exceeded {
        Err(Error::Limit("max_markdown_bytes"))
    } else {
        result
    }
}

struct BoundedOutput<'a, W> {
    output: &'a mut W,
    remaining: usize,
    exceeded: bool,
}
impl<W: Write> Write for BoundedOutput<'_, W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.remaining {
            self.exceeded = true;
            return Err(std::io::Error::other("Markdown output limit exceeded"));
        }
        let n = self.output.write(bytes)?;
        self.remaining -= n;
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.output.flush()
    }
}

impl Parser {
    /// Write Markdown without returning a document model or a Markdown String.
    /// CSV/TSV releases each record; XLS/XLSB/XLSX/ODS releases each worksheet. Other
    /// readers currently build their model, then render tables incrementally.
    /// The caller receives no bytes on parse/strict failure. An I/O failure or
    /// cancellation during final copying can leave a prefix in the caller's writer.
    /// Input and an individual worksheet model are still buffered. Large native
    /// worksheet XML uses bounded fragment storage; recovery emits preview rows.
    pub fn parse_to_markdown(
        &self,
        bytes: &[u8],
        filename: Option<&str>,
        output: &mut impl Write,
    ) -> Result<MarkdownSummary, Error> {
        self.parse_to_markdown_with_cancel(bytes, filename, output, &CancellationToken::default())
    }

    pub fn parse_to_markdown_with_cancel(
        &self,
        bytes: &[u8],
        filename: Option<&str>,
        output: &mut impl Write,
        cancel: &CancellationToken,
    ) -> Result<MarkdownSummary, Error> {
        self.markdown_with_assets(bytes, filename, output, cancel, None)
    }

    fn markdown_with_assets(
        &self,
        bytes: &[u8],
        filename: Option<&str>,
        output: &mut impl Write,
        cancel: &CancellationToken,
        image_directory: Option<&Path>,
    ) -> Result<MarkdownSummary, Error> {
        if bytes.len() > self.options.limits.max_input_bytes {
            return Err(Error::Limit("max_input_bytes"));
        }
        let mut ctx = Context::new(&self.options, cancel);
        ctx.check()?;
        let format = formats::detect(bytes, filename, self.options.format, &mut ctx)?;
        let mut spool = tempfile::spooled_tempfile(64 * 1024);
        let render_diagnostics = match attempt(bytes, format, &mut ctx, &mut spool, false) {
            Ok(diagnostics) => diagnostics,
            Err(Error::Limit(limit))
                if self.options.partial_on_limit
                    && !self.options.strict
                    && matches!(
                        format,
                        Format::Csv | Format::Tsv | Format::Xls | Format::Xlsx | Format::Ods
                    )
                    && matches!(
                        limit,
                        "max_nodes" | "max_table_cells" | "max_text_bytes" | "max_expanded_bytes"
                    ) =>
            {
                // The first attempt can already have written complete rows or
                // sheets. Remove all of them before attempting bounded recovery.
                spool.set_len(0)?;
                spool.seek(SeekFrom::Start(0))?;
                ctx = Context::new(&self.options, cancel);
                ctx.recovering_limits = true;
                let diagnostics = attempt(bytes, format, &mut ctx, &mut spool, true)?;
                ctx.diagnostic("resource_limit_partial",
                    &format!("Normal conversion exceeded {limit}. Bounded recovery returned a partial prefix; omitted content and unexamined tails are not validated. Limits were not increased."),
                    Source::default(), true);
                diagnostics
            }
            Err(error) => return Err(error),
        };
        crate::assets::collect(format, bytes, &mut ctx)?;
        ctx.check()?;
        let status = if ctx.diagnostics.iter().any(|d| d.content_loss) {
            Status::Partial
        } else {
            Status::Complete
        };
        if self.options.strict {
            if status == Status::Partial {
                return Err(Error::Incomplete(ctx.diagnostics.len()));
            }
            if render_diagnostics.iter().any(|d| d.content_loss) {
                return Err(Error::Incomplete(render_diagnostics.len()));
            }
        }
        let mut images = Vec::new();
        if let Some(directory) = image_directory.filter(|_| !ctx.assets.is_empty()) {
            let prefix = directory
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| Error::Malformed("Invalid image directory name".into()))?;
            for asset in &ctx.assets {
                ctx.check()?;
                let path = format!("{prefix}/{}", asset.id);
                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(directory.join(&asset.id))?;
                file.write_all(&asset.data)?;
                images.push(ExportedImage {
                    id: asset.id.clone(),
                    media_type: asset.media_type.clone(),
                    source: asset.source.clone(),
                    path,
                    bytes: asset.data.len(),
                });
            }
        }
        spool.seek(SeekFrom::Start(0))?;
        // Check cancellation between bounded copy chunks as well as during parsing.
        let mut buffer = [0u8; 64 * 1024];
        let mut bytes_written = 0;
        loop {
            ctx.check()?;
            let size = spool.read(&mut buffer)?;
            if size == 0 {
                break;
            }
            output.write_all(&buffer[..size])?;
            bytes_written += size as u64;
        }
        Ok(MarkdownSummary {
            image_count: ctx.assets.len(),
            images,
            format,
            status,
            diagnostics: ctx.diagnostics,
            render_diagnostics,
            coverage: ctx.coverage,
            pdf_inspection: ctx.pdf_inspection,
            parser_version: env!("CARGO_PKG_VERSION").into(),
            bytes_written,
        })
    }

    /// Input is capped but buffered: ZIP and legacy Office readers need random access.
    pub fn parse_reader_to_markdown(
        &self,
        reader: impl Read,
        filename: Option<&str>,
        output: &mut impl Write,
    ) -> Result<MarkdownSummary, Error> {
        let mut bytes = Vec::new();
        reader
            .take((self.options.limits.max_input_bytes as u64).saturating_add(1))
            .read_to_end(&mut bytes)?;
        self.parse_to_markdown(&bytes, filename, output)
    }

    /// Publish a complete Markdown file atomically, without replacing an existing file.
    pub fn parse_file_to_markdown(
        &self,
        input: impl AsRef<Path>,
        output: impl AsRef<Path>,
    ) -> Result<MarkdownSummary, Error> {
        let input = input.as_ref();
        let output = output.as_ref();
        let parent = output
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        if self.options.export_images && !self.options.extract_images {
            return Err(Error::Malformed(
                "export_images requires extract_images".into(),
            ));
        }
        let mut temp = tempfile::NamedTempFile::new_in(parent)?;
        // Unique sibling directory: no document-controlled names or overwrites.
        // RAII removes every staged image on parse, strict, I/O or publication failure.
        let directory = if self.options.export_images {
            Some(
                tempfile::Builder::new()
                    .prefix("bulkparse-images-")
                    .tempdir_in(parent)?,
            )
        } else {
            None
        };
        let summary = if let Some(directory) = &directory {
            let mut bytes = Vec::new();
            std::fs::File::open(input)?
                .take((self.options.limits.max_input_bytes as u64).saturating_add(1))
                .read_to_end(&mut bytes)?;
            self.markdown_with_assets(
                &bytes,
                input.file_name().and_then(|s| s.to_str()),
                &mut temp,
                &CancellationToken::default(),
                Some(directory.path()),
            )?
        } else {
            self.parse_reader_to_markdown(
                std::fs::File::open(input)?,
                input.file_name().and_then(|s| s.to_str()),
                &mut temp,
            )?
        };
        temp.flush()?;
        temp.persist_noclobber(output)
            .map_err(|e| Error::Io(e.error))?;
        if !summary.images.is_empty() {
            if let Some(directory) = directory {
                let _ = directory.keep();
            }
        }
        Ok(summary)
    }
}
