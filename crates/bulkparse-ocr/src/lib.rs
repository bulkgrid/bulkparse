//! Optional native OCR process boundary. No inference runtime is linked into Rust.
#![forbid(unsafe_code)]

use bulkparse::{Error, PdfOcrLine, PdfOcrOutput, PdfOcrProvider, PdfOcrRegion, PdfOcrRequest};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

mod worker;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Trusted host command, e.g. an explicitly provisioned Python interpreter.
    pub executable: PathBuf,
    /// Trusted prefix arguments, normally the path to rapidocr_backend.py.
    pub args: Vec<String>,
    /// Explicit local backend settings: model fingerprints (RapidOCR) or a
    /// configured layout-service endpoint (GLM-OCR). Never downloaded implicitly.
    pub models: PathBuf,
    #[serde(default = "default_dpi")]
    pub dpi: u32,
    #[serde(default = "default_pixels")]
    pub max_pixels: u64,
    #[serde(default = "default_timeout")]
    pub page_timeout_ms: u64,
    /// Whole conversion deadline used by native binding parse calls.
    #[serde(default = "default_document_timeout")]
    pub document_timeout_ms: u64,
    #[serde(default = "default_input")]
    pub max_input_bytes: usize,
    /// Opt into the bundled backend's framed, persistent worker protocol.
    /// Legacy/custom single-page commands remain the default for existing settings.
    #[serde(default)]
    pub reuse_process: bool,
    /// Recycle a reused worker after this many pages to bound its lifetime.
    #[serde(default = "default_worker_pages")]
    pub max_pages_per_worker: u32,
}
fn default_worker_pages() -> u32 {
    128
}
fn default_dpi() -> u32 {
    200
}
fn default_pixels() -> u64 {
    8_000_000
}
fn default_document_timeout() -> u64 {
    120_000
}
fn default_timeout() -> u64 {
    30_000
}
fn default_input() -> usize {
    32 * 1024 * 1024
}

pub struct ProcessOcr {
    config: Config,
    worker: Mutex<Option<worker::Worker>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Response {
    version: u32,
    #[serde(default)]
    page_count: Option<u32>,
    page: u32,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    error: Option<BackendError>,
    #[serde(default)]
    image_size: Option<[u32; 2]>,
    #[serde(default)]
    lines: Option<Vec<PdfOcrLine>>,
    #[serde(default)]
    regions: Option<Vec<PdfOcrRegion>>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BackendError {
    code: String,
    message: String,
}

impl ProcessOcr {
    pub fn new(config: Config) -> Result<Self, Error> {
        if !(72..=600).contains(&config.dpi)
            || config.max_pixels == 0
            || config.max_pixels > 100_000_000
            || config.page_timeout_ms == 0
            || config.page_timeout_ms > 300_000
            || config.document_timeout_ms == 0
            || config.document_timeout_ms > 3_600_000
            || config.max_input_bytes == 0
            || config.max_input_bytes > 256 * 1024 * 1024
            || config.max_pages_per_worker == 0
            || config.max_pages_per_worker > 4096
        {
            return Err(Error::Malformed("invalid OCR budgets".into()));
        }
        if !config.models.is_absolute() || !config.models.is_file() {
            return Err(Error::Malformed(
                "OCR requires an existing absolute model manifest path".into(),
            ));
        }
        Ok(Self {
            config,
            worker: Mutex::new(None),
        })
    }

    pub fn from_json(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() > 16 * 1024 {
            return Err(Error::Limit("ocr_config_bytes"));
        }
        Self::new(
            serde_json::from_slice(bytes)
                .map_err(|e| Error::Malformed(format!("OCR config: {e}")))?,
        )
    }
}

struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl PdfOcrProvider for ProcessOcr {
    fn recognize(&self, request: PdfOcrRequest<'_>) -> Result<String, Error> {
        self.recognize_page(request).map(PdfOcrOutput::into_text)
    }

    fn recognize_page(&self, request: PdfOcrRequest<'_>) -> Result<PdfOcrOutput, Error> {
        self.recognize_output(request).map(|(output, _)| output)
    }
}

impl bulkparse::ImageOcrProvider for ProcessOcr {
    fn recognize_image_page(
        &self,
        request: PdfOcrRequest<'_>,
    ) -> Result<bulkparse::ImageOcrPage, Error> {
        let (output, count) = self.recognize_output(request)?;
        let page_count = count.filter(|n| *n > 0).ok_or_else(|| {
            Error::Unsupported("OCR backend did not report image frame count".into())
        })?;
        Ok(bulkparse::ImageOcrPage { output, page_count })
    }
}

impl ProcessOcr {
    pub fn parse_json(
        &self,
        bytes: &[u8],
        filename: Option<&str>,
        options: Option<&str>,
        budget: Option<&str>,
    ) -> Result<String, Error> {
        let options: bulkparse::Options = options
            .map(serde_json::from_str)
            .transpose()
            .map_err(|e| Error::Malformed(format!("options: {e}")))?
            .unwrap_or_default();
        let budget: bulkparse::PdfOcrBudget = budget
            .map(serde_json::from_str)
            .transpose()
            .map_err(|e| Error::Malformed(format!("OCR budget: {e}")))?
            .unwrap_or_default();
        let parser = bulkparse::Parser::new(options);
        let token = bulkparse::CancellationToken::default();
        let _deadline = Deadline::start(token.clone(), self.config.document_timeout_ms)?;
        let result = if bulkparse::is_image(bytes) {
            parser.parse_image_with_ocr(bytes, &token, self, budget)?
        } else {
            parser.parse_with_pdf_ocr(bytes, filename, &token, self, budget)?
        };
        let json = bulkparse::conversion_json(result, parser.options.strict)?;
        if token.is_cancelled() {
            return Err(Error::Cancelled);
        }
        Ok(json)
    }

    fn recognize_output(
        &self,
        request: PdfOcrRequest<'_>,
    ) -> Result<(PdfOcrOutput, Option<u32>), Error> {
        if request.cancellation.is_cancelled() {
            return Err(Error::Cancelled);
        }
        if request.page == 0 {
            return Err(Error::Malformed("OCR page numbers start at one".into()));
        }
        if request.document.len() > self.config.max_input_bytes {
            return Err(Error::Limit("ocr_input_bytes"));
        }
        if request.remaining_output_bytes == 0 {
            return Err(Error::Limit("pdf_ocr_output_bytes"));
        }
        let output_limit = request.remaining_output_bytes.min(16 * 1024 * 1024);
        // Each nonempty line requires at least one text byte. Allow bounded
        // geometry overhead separately; decode also caps the total line count.
        let wire_limit = output_limit * 6 + 2 * 1024 * 1024 + 1024;
        let deadline = Instant::now() + Duration::from_millis(self.config.page_timeout_ms);
        if self.config.reuse_process {
            return worker::recognize(
                &self.config,
                &self.worker,
                request,
                output_limit,
                wire_limit,
                deadline,
            );
        }
        let mut input = tempfile::NamedTempFile::new()?;
        input.write_all(request.document)?;
        input.flush()?;
        if request.cancellation.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let mut command = Command::new(&self.config.executable);
        command
            .args(&self.config.args)
            .arg("--input")
            .arg(input.path())
            .arg("--page")
            .arg(request.page.to_string())
            .arg("--models")
            .arg(&self.config.models)
            .arg("--dpi")
            .arg(self.config.dpi.to_string())
            .arg("--max-pixels")
            .arg(self.config.max_pixels.to_string())
            .arg("--max-input-bytes")
            .arg(self.config.max_input_bytes.to_string())
            .arg("--max-output-bytes")
            .arg(output_limit.to_string())
            .env("OMP_NUM_THREADS", "1")
            .env("OPENBLAS_NUM_THREADS", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = OwnedChild(command.spawn()?);
        let stdout = child.0.stdout.take().unwrap();
        let (tx, rx) = mpsc::sync_channel(1);
        let reader = thread::Builder::new()
            .name("bulkparse-ocr-output".into())
            .spawn(move || {
                let mut bytes = vec![];
                let result = stdout
                    .take(wire_limit as u64 + 1)
                    .read_to_end(&mut bytes)
                    .map(|_| bytes);
                let _ = tx.send(result);
            })?;
        let mut output = None;
        let result = loop {
            if request.cancellation.is_cancelled() {
                break Err(Error::Cancelled);
            }
            if Instant::now() >= deadline {
                break Err(Error::Malformed("OCR page timeout".into()));
            }
            if output.is_none() {
                match rx.try_recv() {
                    Ok(Ok(bytes)) if bytes.len() > wire_limit => {
                        break Err(Error::Limit("ocr_response_bytes"))
                    }
                    Ok(Ok(bytes)) => output = Some(bytes),
                    Ok(Err(error)) => break Err(Error::Io(error)),
                    Err(mpsc::TryRecvError::Disconnected) => {
                        break Err(Error::Malformed("OCR output reader stopped".into()))
                    }
                    Err(mpsc::TryRecvError::Empty) => (),
                }
            }
            match child.0.try_wait() {
                Ok(Some(status)) => {
                    if !status.success() {
                        break Err(Error::Malformed(format!("OCR backend exited: {status}")));
                    }
                    if let Some(bytes) = output.take() {
                        break decode(&bytes, request.page, output_limit);
                    }
                }
                Ok(None) => (),
                Err(error) => break Err(Error::Io(error)),
            }
            thread::sleep(Duration::from_millis(10));
        };
        // The configured backend must not fork descendants that inherit stdout.
        drop(child);
        let _ = reader.join();
        if request.cancellation.is_cancelled() {
            return Err(Error::Cancelled);
        }
        result
    }
}

fn decode(bytes: &[u8], page: u32, limit: usize) -> Result<(PdfOcrOutput, Option<u32>), Error> {
    let reply: Response = serde_json::from_slice(bytes)
        .map_err(|e| Error::Malformed(format!("OCR response: {e}")))?;
    decode_response(reply, page, limit)
}

fn decode_response(
    reply: Response,
    page: u32,
    limit: usize,
) -> Result<(PdfOcrOutput, Option<u32>), Error> {
    if ![1, 2, 3].contains(&reply.version) || reply.page != page {
        return Err(Error::Malformed("invalid OCR response envelope".into()));
    }
    if let Some(error) = reply.error {
        if reply.text.is_some()
            || reply.lines.is_some()
            || reply.image_size.is_some()
            || reply.regions.is_some()
        {
            return Err(Error::Malformed("ambiguous OCR error response".into()));
        }
        return if error.code == "resource_limit" {
            Err(Error::Limit("ocr_backend_budget"))
        } else {
            Err(Error::Malformed(format!(
                "OCR {}: {}",
                error.code, error.message
            )))
        };
    }
    let output = match (
        reply.version,
        reply.text,
        reply.image_size,
        reply.lines,
        reply.regions,
    ) {
        (1, Some(text), None, None, None) => PdfOcrOutput::Text(text),
        (2, None, Some(image_size), Some(lines), None) => PdfOcrOutput::Lines { image_size, lines },
        (3, None, Some(image_size), None, Some(regions)) => PdfOcrOutput::Regions {
            image_size,
            regions,
        },
        _ => return Err(Error::Malformed("invalid OCR response envelope".into())),
    };
    output.validate(limit)?;
    if reply
        .page_count
        .is_some_and(|count| count == 0 || page > count)
    {
        return Err(Error::Malformed("invalid OCR image frame count".into()));
    }
    Ok((output, reply.page_count))
}

struct Deadline {
    stop: Option<mpsc::Sender<()>>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Deadline {
    fn start(token: bulkparse::CancellationToken, millis: u64) -> Result<Self, Error> {
        let (stop, stopped) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("bulkparse-ocr-deadline".into())
            .spawn(move || {
                if matches!(
                    stopped.recv_timeout(Duration::from_millis(millis)),
                    Err(mpsc::RecvTimeoutError::Timeout)
                ) {
                    token.cancel();
                }
            })?;
        Ok(Self {
            stop: Some(stop),
            thread: Some(thread),
        })
    }
}
impl Drop for Deadline {
    fn drop(&mut self) {
        drop(self.stop.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
