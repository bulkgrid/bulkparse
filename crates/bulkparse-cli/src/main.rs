use bulkparse::{Error, Format, Options, Parser, Status, TextEncoding};
use clap::{Parser as ClapParser, Subcommand, ValueEnum};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

#[derive(ClapParser)]
#[command(
    name = "bulkparse",
    version,
    about = "Offline document conversion to structured JSON and Markdown"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Clone, Copy, ValueEnum)]
enum Output {
    Markdown,
    Json,
}

#[derive(Clone, Copy, ValueEnum)]
enum CsvEncoding {
    Utf8,
    Utf16Le,
    Utf16Be,
    Windows1252,
}

#[derive(Subcommand)]
enum Command {
    /// Parse one document. Use '-' for standard input.
    Parse {
        input: PathBuf,
        #[arg(long, value_enum, default_value = "markdown")]
        to: Output,
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Save embedded Office/EPUB images beside unchanged Markdown.
        #[arg(long, requires = "output")]
        export_images: bool,
        /// Omit embedded image bytes from structured output.
        #[arg(long)]
        no_images: bool,
        /// Required for signature-less standard input.
        #[arg(long)]
        format: Option<String>,
        #[arg(long)]
        strict: bool,
        #[arg(long)]
        csv_has_headers: bool,
        /// Override automatic comma, semicolon, tab or pipe detection.
        #[arg(long)]
        csv_delimiter: Option<char>,
        /// Require an encoding; otherwise detect BOM/UTF-8 with a diagnosed Windows-1252 fallback.
        #[arg(long, value_enum)]
        csv_encoding: Option<CsvEncoding>,
        /// Recover a bounded, explicitly partial spreadsheet/CSV prefix on resource limits.
        #[arg(long, conflicts_with = "strict")]
        partial_on_limit: bool,
        /// Compatibility flag; large-document budgets are now the default.
        #[arg(long, hide = true)]
        large_documents: bool,
        /// Expand PDF presentation forms and ligatures; preserve ActualText.
        #[arg(long, action = clap::ArgAction::Set, num_args = 0..=1, default_missing_value = "true", default_value_t = true, require_equals = true)]
        pdf_normalize_unicode: bool,
        #[arg(long, default_value_t = 33_554_432)]
        max_input_bytes: usize,
        /// Explicit local OCR provider configuration (optional build feature).
        #[cfg(feature = "ocr")]
        #[arg(long)]
        ocr_config: Option<PathBuf>,
        #[cfg(feature = "ocr")]
        #[arg(long, default_value_t = 8, requires = "ocr_config")]
        ocr_max_pages: usize,
        #[cfg(feature = "ocr")]
        #[arg(long, default_value_t = 1_048_576, requires = "ocr_config")]
        ocr_max_output_bytes: usize,
    },
    /// Sequential batch processing with per-file JSONL results on stdout.
    Batch {
        input: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
        #[arg(long, value_enum, default_value = "json")]
        to: Output,
        #[arg(long)]
        strict: bool,
    },
    /// Inspect PDF pages and recommend OCR routes without rendering or inference.
    Inspect {
        input: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
        #[arg(long, default_value_t = 33_554_432)]
        max_input_bytes: usize,
    },
    /// Machine-readable build capabilities.
    Capabilities,
    /// JSON Schema for ParseResult (bindings add Markdown fields).
    Schema,
}

fn config(
    format: Option<String>,
    strict: bool,
    csv_has_headers: bool,
    csv_delimiter: Option<char>,
    csv_encoding: Option<CsvEncoding>,
    max_input_bytes: usize,
) -> Result<Options, Error> {
    let mut options = Options {
        strict,
        csv_has_headers,
        csv_delimiter,
        csv_encoding: csv_encoding.map(|encoding| match encoding {
            CsvEncoding::Utf8 => TextEncoding::Utf8,
            CsvEncoding::Utf16Le => TextEncoding::Utf16Le,
            CsvEncoding::Utf16Be => TextEncoding::Utf16Be,
            CsvEncoding::Windows1252 => TextEncoding::Windows1252,
        }),
        ..Default::default()
    };
    options.format = format
        .map(|s| Format::from_name(&s).ok_or(Error::Unsupported(s)))
        .transpose()?;
    options.limits.max_input_bytes = max_input_bytes;
    Ok(options)
}

fn encode(
    result: &bulkparse::ParseResult,
    to: Output,
    strict: bool,
) -> Result<(String, bool), Error> {
    let mut partial = result.status == Status::Partial;
    let text = match to {
        Output::Json => result
            .to_json()
            .map_err(|e| Error::Malformed(e.to_string()))?,
        Output::Markdown => {
            let rendered = result.to_markdown();
            for diagnostic in &rendered.diagnostics {
                eprintln!(
                    "{} [{}; {}]: {}",
                    diagnostic.code, diagnostic.category, diagnostic.evidence, diagnostic.message
                );
                partial |= diagnostic.content_loss;
            }
            if strict && partial {
                return Err(Error::Incomplete(rendered.diagnostics.len()));
            }
            rendered.text
        }
    };
    Ok((text, partial))
}

fn save(path: &Path, text: &str) -> Result<(), Error> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(text.as_bytes())?;
    temporary.flush()?;
    temporary
        .persist_noclobber(path)
        .map_err(|e| Error::Io(e.error))?;
    Ok(())
}

fn gather(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), Error> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            gather(&entry.path(), files)?;
        } else if kind.is_file() {
            files.push(entry.path());
        }
    }
    Ok(())
}

fn run() -> Result<i32, Error> {
    match Cli::parse().command {
        Command::Inspect {
            input,
            output,
            max_input_bytes,
        } => {
            use std::io::Read;
            let mut options = Options::default();
            options.limits.max_input_bytes = max_input_bytes;
            let reader: Box<dyn Read> = if input == Path::new("-") {
                Box::new(io::stdin())
            } else {
                Box::new(std::fs::File::open(&input)?)
            };
            let mut bytes = Vec::new();
            reader
                .take((max_input_bytes as u64).saturating_add(1))
                .read_to_end(&mut bytes)?;
            let result = Parser::new(options).inspect_pdf(&bytes)?;
            let text = serde_json::to_string_pretty(&result)
                .map_err(|e| Error::Malformed(e.to_string()))?;
            if let Some(output) = output {
                save(&output, &text)?;
            } else {
                io::stdout().lock().write_all(text.as_bytes())?;
            }
            Ok(
                if result
                    .pages
                    .iter()
                    .any(|p| p.route == bulkparse::PdfRoute::Review)
                {
                    2
                } else {
                    0
                },
            )
        }
        Command::Capabilities => {
            println!(
                "{}",
                serde_json::to_string_pretty(&bulkparse::capabilities()).unwrap()
            );
            Ok(0)
        }
        Command::Schema => {
            println!(
                "{}",
                serde_json::to_string_pretty(&bulkparse::json_schema()).unwrap()
            );
            Ok(0)
        }
        Command::Parse {
            input,
            to,
            output,
            export_images,
            no_images,
            format,
            strict,
            csv_has_headers,
            csv_delimiter,
            csv_encoding,
            partial_on_limit,
            large_documents,
            pdf_normalize_unicode,
            max_input_bytes,
            #[cfg(feature = "ocr")]
            ocr_config,
            #[cfg(feature = "ocr")]
            ocr_max_pages,
            #[cfg(feature = "ocr")]
            ocr_max_output_bytes,
        } => {
            let mut options = config(
                format,
                strict,
                csv_has_headers,
                csv_delimiter,
                csv_encoding,
                max_input_bytes,
            )?;
            options.extract_images = !no_images;
            options.export_images = export_images;
            if export_images
                && (no_images || !matches!(to, Output::Markdown) || input == Path::new("-"))
            {
                return Err(Error::Unsupported("--export-images requires Markdown file output, a file input, and image retention".into()));
            }
            #[cfg(feature = "ocr")]
            if export_images && ocr_config.is_some() {
                return Err(Error::Unsupported(
                    "Image file export with the OCR CLI path is not yet supported".into(),
                ));
            }
            options.pdf_normalize_unicode = pdf_normalize_unicode;
            options.partial_on_limit = partial_on_limit;
            if large_documents {
                #[cfg(feature = "ocr")]
                if ocr_config.is_some() {
                    return Err(Error::Unsupported(
                        "--large-documents requires the native Markdown writer without OCR".into(),
                    ));
                }
                options.limits = bulkparse::Limits::large_spreadsheets();
                options.limits.max_input_bytes = max_input_bytes;
            }
            let parser = Parser::new(options);
            #[cfg(feature = "ocr")]
            let stream_markdown = matches!(to, Output::Markdown) && ocr_config.is_none();
            #[cfg(not(feature = "ocr"))]
            let stream_markdown = matches!(to, Output::Markdown);
            if stream_markdown {
                let summary = parse_to_markdown_input(&parser, &input, output.as_deref())?;
                let mut partial = summary.status == Status::Partial;
                for diagnostic in summary
                    .diagnostics
                    .iter()
                    .chain(&summary.render_diagnostics)
                {
                    eprintln!(
                        "{} [{}; {}]: {}",
                        diagnostic.code,
                        diagnostic.category,
                        diagnostic.evidence,
                        diagnostic.message
                    );
                    partial |= diagnostic.content_loss;
                }
                return Ok(if partial { 2 } else { 0 });
            }
            #[cfg(feature = "ocr")]
            let result = match ocr_config {
                Some(config) => parse_with_ocr(
                    &parser,
                    &input,
                    &config,
                    bulkparse::PdfOcrBudget {
                        max_pages: ocr_max_pages,
                        max_output_bytes: ocr_max_output_bytes,
                    },
                )?,
                None => parse_input(&parser, &input)?,
            };
            #[cfg(not(feature = "ocr"))]
            let result = parse_input(&parser, &input)?;
            for diagnostic in &result.diagnostics {
                eprintln!(
                    "{} [{}; {}]: {}",
                    diagnostic.code, diagnostic.category, diagnostic.evidence, diagnostic.message
                );
            }
            if matches!(to, Output::Json) {
                // Retain the complete structured model, but avoid a second
                // document-sized JSON String when writing it to a file/stdout.
                if let Some(output) = output {
                    save_json(&output, &result)?;
                } else {
                    write_json(&result, io::stdout().lock())?;
                }
                return Ok(if result.status == Status::Partial {
                    2
                } else {
                    0
                });
            }
            let (text, partial) = encode(&result, to, strict)?;
            if let Some(output) = output {
                save(&output, &text)?;
            } else {
                io::stdout().lock().write_all(text.as_bytes())?;
            }
            Ok(if partial { 2 } else { 0 })
        }
        Command::Batch {
            input,
            output,
            to,
            strict,
        } => {
            let root = std::fs::canonicalize(input)?;
            std::fs::create_dir_all(&output)?;
            let destination = std::fs::canonicalize(output)?;
            if destination.starts_with(&root) {
                return Err(Error::Malformed(
                    "batch output must be outside the input directory".into(),
                ));
            }
            let mut files = Vec::new();
            gather(&root, &mut files)?;
            files.sort();
            let parser = Parser::new(Options {
                strict,
                ..Default::default()
            });
            let mut failed = false;
            let mut partial = false;
            for path in files {
                let relative = path
                    .strip_prefix(&root)
                    .map_err(|e| Error::Malformed(e.to_string()))?;
                let suffix = match to {
                    Output::Json => ".json",
                    Output::Markdown => ".md",
                };
                let mut name = relative.as_os_str().to_os_string();
                name.push(suffix);
                let target = destination.join(name);
                let converted = (|| -> Result<(Vec<bulkparse::Diagnostic>, bool), Error> {
                    if let Some(parent) = target.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    if matches!(to, Output::Markdown) {
                        let summary = parser.parse_file_to_markdown(&path, &target)?;
                        let partial = summary.status == Status::Partial
                            || summary.render_diagnostics.iter().any(|d| d.content_loss);
                        return Ok((summary.diagnostics, partial));
                    }
                    let result = parser.parse_file(&path)?;
                    let (text, was_partial) = encode(&result, to, strict)?;
                    if let Some(parent) = target.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    save(&target, &text)?;
                    Ok((result.diagnostics, was_partial))
                })();
                let record = match converted {
                    Ok((diagnostics, was_partial)) => {
                        partial |= was_partial;
                        serde_json::json!({"input":relative,"output":target,"status":if was_partial {"partial"} else {"complete"},"diagnostics":diagnostics})
                    }
                    Err(error) => {
                        failed = true;
                        serde_json::json!({"input":relative,"status":"failed","error":{"code":error.code(),"message":error.to_string()}})
                    }
                };
                writeln!(io::stdout().lock(), "{record}")?;
            }
            Ok(if failed {
                1
            } else if partial {
                2
            } else {
                0
            })
        }
    }
}

fn write_json(result: &bulkparse::ParseResult, output: impl Write) -> Result<(), Error> {
    let mut buffered = io::BufWriter::with_capacity(64 * 1024, output);
    serde_json::to_writer_pretty(&mut buffered, result)
        .map_err(|e| Error::Io(io::Error::other(e)))?;
    buffered.flush()?;
    Ok(())
}

fn save_json(path: &Path, result: &bulkparse::ParseResult) -> Result<(), Error> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    write_json(result, &mut temporary)?;
    temporary
        .persist_noclobber(path)
        .map_err(|e| Error::Io(e.error))?;
    Ok(())
}

fn parse_to_markdown_input(
    parser: &Parser,
    input: &Path,
    output: Option<&Path>,
) -> Result<bulkparse::MarkdownSummary, Error> {
    if input != Path::new("-") {
        if let Some(output) = output {
            return parser.parse_file_to_markdown(input, output);
        }
    }
    let reader: Box<dyn io::Read> = if input == Path::new("-") {
        Box::new(io::stdin())
    } else {
        Box::new(std::fs::File::open(input)?)
    };
    let filename = if input == Path::new("-") {
        None
    } else {
        input.file_name().and_then(|s| s.to_str())
    };
    if let Some(output) = output {
        let parent = output
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut temp = tempfile::NamedTempFile::new_in(parent)?;
        let summary = parser.parse_reader_to_markdown(reader, filename, &mut temp)?;
        temp.flush()?;
        temp.persist_noclobber(output)
            .map_err(|e| Error::Io(e.error))?;
        Ok(summary)
    } else {
        parser.parse_reader_to_markdown(reader, filename, &mut io::stdout().lock())
    }
}

fn parse_input(parser: &Parser, input: &Path) -> Result<bulkparse::ParseResult, Error> {
    if input == Path::new("-") {
        parser.parse_reader(io::stdin().lock(), None)
    } else {
        parser.parse_file(input)
    }
}

#[cfg(feature = "ocr")]
fn parse_with_ocr(
    parser: &Parser,
    input: &Path,
    config: &Path,
    budget: bulkparse::PdfOcrBudget,
) -> Result<bulkparse::ParseResult, Error> {
    use std::io::Read;
    let mut settings = Vec::new();
    std::fs::File::open(config)?
        .take(16 * 1024 + 1)
        .read_to_end(&mut settings)?;
    let provider = bulkparse_ocr::ProcessOcr::from_json(&settings)?;
    let reader: Box<dyn Read> = if input == Path::new("-") {
        Box::new(io::stdin())
    } else {
        Box::new(std::fs::File::open(input)?)
    };
    let mut bytes = Vec::new();
    reader
        .take((parser.options.limits.max_input_bytes as u64).saturating_add(1))
        .read_to_end(&mut bytes)?;
    parser.parse_with_pdf_ocr(
        &bytes,
        input.file_name().and_then(|p| p.to_str()),
        &bulkparse::CancellationToken::default(),
        &provider,
        budget,
    )
}

fn main() {
    let code = match run() {
        Ok(code) => code,
        Err(Error::Io(error)) if error.kind() == io::ErrorKind::BrokenPipe => 0,
        Err(error) => {
            eprintln!("{}", bulkparse::error_json(&error));
            1
        }
    };
    std::process::exit(code);
}

#[cfg(test)]
mod json_tests {
    use super::*;
    #[test]
    fn buffered_json_matches_existing_serialization_and_refuses_overwrite() {
        let result = Parser::default()
            .parse_bytes(b"a,b\n1,2\n", Some("a.csv"))
            .unwrap();
        let mut output = Vec::new();
        write_json(&result, &mut output).unwrap();
        assert_eq!(output, result.to_json().unwrap().as_bytes());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("result.json");
        save_json(&path, &result).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), output);
        assert!(save_json(&path, &result).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), output);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn json_writer_propagates_io_failure() {
        struct Broken;
        impl Write for Broken {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::ErrorKind::BrokenPipe.into())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let result = Parser::default().parse_bytes(b"a", Some("a.txt")).unwrap();
        assert!(matches!(write_json(&result, Broken), Err(Error::Io(_))));
    }
}
