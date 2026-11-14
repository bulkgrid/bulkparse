//! Native OCR pipeline. No Python interpreter, subprocess, or runtime download.
#![forbid(unsafe_code)]
use bulkparse::{
    Error, ImageOcrPage, ImageOcrProvider, PdfOcrOutput, PdfOcrProvider, PdfOcrRequest,
};
use image::RgbImage;
mod headers;
mod images;
mod layout;
mod marks;
mod recognition;
mod tables;
#[cfg(test)]
mod tests;
use oar_ocr::core::config::onnx::OrtSessionConfig;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    path::PathBuf,
    sync::{Mutex, TryLockError},
    time::{Duration, Instant},
};

#[derive(Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub models: PathBuf,
    pub onnxruntime_library: PathBuf,
    pub pdfium_library: Option<PathBuf>,
    pub dpi: u32,
    pub max_pixels: u64,
    pub max_input_bytes: usize,
    pub max_frames: u32,
    pub page_timeout_ms: u64,
    pub document_timeout_ms: u64,
    pub threads: usize,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            models: PathBuf::new(),
            onnxruntime_library: PathBuf::new(),
            pdfium_library: None,
            dpi: 200,
            max_pixels: 8_000_000,
            max_input_bytes: 256 * 1024 * 1024,
            max_frames: 2000,
            page_timeout_ms: 30_000,
            document_timeout_ms: 120_000,
            threads: 1,
        }
    }
}
#[derive(Deserialize)]
struct Artifact {
    path: PathBuf,
    sha256: String,
}
impl Artifact {
    fn verify(&self) -> Result<(), Error> {
        if !self.path.is_absolute() {
            return Err(bad("model paths must be absolute"));
        }
        let mut file = std::fs::File::open(&self.path)?;
        if file.metadata()?.len() > 1024 * 1024 * 1024 {
            return Err(Error::Limit("ocr_model_bytes"));
        }
        let mut hasher = Sha256::new();
        let mut bytes = [0; 65536];
        loop {
            let count = file.read(&mut bytes)?;
            if count == 0 {
                break;
            }
            hasher.update(&bytes[..count]);
        }
        let digest: String = hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        if digest != self.sha256 {
            return Err(bad("OCR model fingerprint mismatch"));
        }
        Ok(())
    }
}
#[derive(Deserialize)]
struct Models {
    det: Artifact,
    rec: Artifact,
    cls: Option<Artifact>,
}
#[derive(Deserialize)]
struct Manifest {
    models: Models,
    dictionary: Option<Artifact>,
    layout: Option<layout::Config>,
}
struct Runtime {
    engine: recognition::Engine,
    layout: Option<layout::Layout>,
}
pub struct NativeOcr {
    config: Config,
    runtime: Mutex<Option<Runtime>>,
}
fn bad(error: impl std::fmt::Display) -> Error {
    Error::Malformed(error.to_string())
}
impl NativeOcr {
    pub fn new(config: Config) -> Result<Self, Error> {
        if !(72..=600).contains(&config.dpi)
            || config.max_pixels == 0
            || config.max_pixels > 100_000_000
            || config.max_input_bytes == 0
            || config.max_frames == 0
            || config.max_frames > 2000
            || config.threads == 0
            || config.threads > 16
            || config.page_timeout_ms == 0
            || config.document_timeout_ms == 0
            || config.document_timeout_ms > 3_600_000
        {
            return Err(bad("invalid native OCR budgets"));
        }
        if !config.models.is_absolute()
            || !config.models.is_file()
            || !config.onnxruntime_library.is_absolute()
            || !config.onnxruntime_library.is_file()
        {
            return Err(bad(
                "native OCR requires explicit model manifest and ONNX Runtime library paths",
            ));
        }
        Ok(Self {
            config,
            runtime: Mutex::new(None),
        })
    }
    pub fn from_json(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() > 16384 {
            return Err(Error::Limit("ocr_config_bytes"));
        }
        Self::new(serde_json::from_slice(bytes).map_err(bad)?)
    }
    fn load(&self) -> Result<Runtime, Error> {
        let mut data = Vec::new();
        std::fs::File::open(&self.config.models)?
            .take(16385)
            .read_to_end(&mut data)?;
        if data.len() > 16384 {
            return Err(Error::Limit("ocr_manifest_bytes"));
        }
        let manifest: Manifest = serde_json::from_slice(&data).map_err(bad)?;
        manifest.models.det.verify()?;
        manifest.models.rec.verify()?;
        if let Some(dictionary) = &manifest.dictionary {
            dictionary.verify()?;
            if std::fs::metadata(&dictionary.path)?.len() > 4 * 1024 * 1024 {
                return Err(Error::Limit("ocr_dictionary_bytes"));
            }
        }
        if let Some(cls) = &manifest.models.cls {
            cls.verify()?;
        }
        let _ = ort::init_from(&self.config.onnxruntime_library)
            .map_err(bad)?
            .commit();
        let dictionary = if let Some(dictionary) = manifest.dictionary {
            std::fs::read_to_string(dictionary.path)?
        } else {
            // The verified recognition model may carry its exact character vocabulary.
            let session = ort::session::Session::builder()
                .map_err(bad)?
                .with_intra_threads(1)
                .map_err(bad)?
                .commit_from_file(&manifest.models.rec.path)
                .map_err(bad)?;
            let dictionary=session.metadata().map_err(bad)?.custom("character")
                .ok_or_else(||bad("recognition model needs an embedded character vocabulary or dictionary artifact"))?;
            dictionary
        };
        let engine = recognition::Engine::load(
            &manifest.models,
            dictionary,
            OrtSessionConfig::new()
                .with_intra_threads(self.config.threads)
                .with_inter_threads(1)
                .with_parallel_execution(false)
                .with_memory_pattern(false),
        )?;
        let layout = manifest
            .layout
            .map(|config| {
                layout::Layout::load(
                    config,
                    OrtSessionConfig::new()
                        .with_intra_threads(self.config.threads)
                        .with_inter_threads(1)
                        .with_parallel_execution(false)
                        .with_memory_pattern(false),
                )
            })
            .transpose()?;
        Ok(Runtime { engine, layout })
    }
    fn check_pixels(&self, width: u32, height: u32) -> Result<(), Error> {
        if width == 0
            || height == 0
            || u64::from(width) * u64::from(height) > self.config.max_pixels
        {
            return Err(Error::Limit("ocr_pixels"));
        }
        Ok(())
    }
    fn pdf(&self, bytes: &[u8], page: u32) -> Result<RgbImage, Error> {
        let path = self
            .config
            .pdfium_library
            .as_ref()
            .ok_or_else(|| bad("PDF OCR requires a PDFium library path"))?;
        let engine = firecrawl_pdfium::Pdfium::load_from_path(path).map_err(bad)?;
        let document = engine.load_document(bytes.to_vec(), None).map_err(bad)?;
        if page == 0 || page as usize > document.page_count() {
            return Err(bad("OCR page outside PDF"));
        }
        document.enable_form_rendering().map_err(bad)?;
        let page = document.page(page as usize - 1).map_err(bad)?;
        let size = page.size();
        let scale = self.config.dpi as f64 / 72.;
        self.check_pixels(
            (f64::from(size.width) * scale).ceil() as u32,
            (f64::from(size.height) * scale).ceil() as u32,
        )?;
        let render = page
            .render(
                &firecrawl_pdfium::RenderConfig::new()
                    .dpi(self.config.dpi as f32)
                    .pixel_format(firecrawl_pdfium::PixelFormat::Bgr8)
                    .annotations(true)
                    .form_fields(true)
                    .max_output_bytes(self.config.max_pixels * 3),
            )
            .map_err(bad)?;
        let (width, height) = (render.width(), render.height());
        let mut pixels = render.into_pixels();
        for rgb in pixels.as_chunks_mut::<3>().0 {
            rgb.swap(0, 2);
        }
        RgbImage::from_raw(width, height, pixels)
            .ok_or_else(|| bad("invalid rendered pixel buffer"))
    }
    fn recognize_input(
        &self,
        request: PdfOcrRequest<'_>,
        image: bool,
    ) -> Result<ImageOcrPage, Error> {
        let deadline = Instant::now() + Duration::from_millis(self.config.page_timeout_ms);
        let check = || {
            if request.cancellation.is_cancelled() {
                Err(Error::Cancelled)
            } else if Instant::now() >= deadline {
                Err(bad("native OCR page timeout"))
            } else {
                Ok(())
            }
        };
        check()?;
        if request.document.len() > self.config.max_input_bytes {
            return Err(Error::Limit("ocr_input_bytes"));
        }
        let mut runtime = loop {
            check()?;
            match self.runtime.try_lock() {
                Ok(slot) => break slot,
                Err(TryLockError::WouldBlock) => std::thread::sleep(Duration::from_millis(5)),
                Err(_) => return Err(bad("OCR runtime lock poisoned")),
            }
        };
        let (pixels, page_count) = if image {
            self.image(request.document, request.page, &check)?
        } else {
            (self.pdf(request.document, request.page)?, 1)
        };
        check()?;
        let dimensions = [pixels.width(), pixels.height()];
        if runtime.is_none() {
            *runtime = Some(self.load()?);
        }
        check()?;
        let pixels = std::sync::Arc::new(pixels);
        let lines = runtime
            .as_ref()
            .unwrap()
            .engine
            .recognize(&pixels, &check)?;
        let mut regions = tables::regions(&pixels, &lines);
        // Rule-based tables claim only their own evidence. Remaining page text
        // may contain additional tables or headings and still needs layout.
        let needs_layout = regions.as_ref().is_none_or(|regions| {
            regions
                .iter()
                .any(|r| !matches!(r.content, bulkparse::PdfOcrRegionContent::Table { .. }))
        });
        if needs_layout {
            if let Some(layout) = &runtime.as_ref().unwrap().layout {
                let seeds = regions
                    .as_ref()
                    .map(|regions| {
                        regions
                            .iter()
                            .filter(|r| {
                                matches!(r.content, bulkparse::PdfOcrRegionContent::Table { .. })
                            })
                            .cloned()
                            .collect()
                    })
                    .unwrap_or_default();
                regions = layout.regions(&pixels, &lines, seeds, &check)?;
            }
        }
        let output = if let Some(regions) = regions {
            PdfOcrOutput::Regions {
                image_size: dimensions,
                regions,
            }
        } else {
            PdfOcrOutput::Lines {
                image_size: dimensions,
                lines,
            }
        };
        check()?;
        output.validate(request.remaining_output_bytes)?;
        Ok(ImageOcrPage { page_count, output })
    }
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
            .map_err(bad)?
            .unwrap_or_default();
        let budget: bulkparse::PdfOcrBudget = budget
            .map(serde_json::from_str)
            .transpose()
            .map_err(bad)?
            .unwrap_or_default();
        let parser = bulkparse::Parser::new(options);
        let result = self.parse_document(&parser, bytes, filename, budget)?;
        bulkparse::conversion_json(result, parser.options.strict)
    }

    /// Parse with cooperative whole-document cancellation, including CLI callers.
    pub fn parse_document(
        &self,
        parser: &bulkparse::Parser,
        bytes: &[u8],
        filename: Option<&str>,
        budget: bulkparse::PdfOcrBudget,
    ) -> Result<bulkparse::ParseResult, Error> {
        let token = bulkparse::CancellationToken::default();
        let _deadline = Deadline::start(token.clone(), self.config.document_timeout_ms)?;
        let result = if bulkparse::is_image(bytes) {
            parser.parse_image_with_ocr(bytes, &token, self, budget)?
        } else {
            parser.parse_with_pdf_ocr(bytes, filename, &token, self, budget)?
        };
        if token.is_cancelled() {
            return Err(Error::Cancelled);
        }
        Ok(result)
    }
}
impl PdfOcrProvider for NativeOcr {
    fn recognize(&self, request: PdfOcrRequest<'_>) -> Result<String, Error> {
        self.recognize_page(request).map(PdfOcrOutput::into_text)
    }
    fn recognize_page(&self, request: PdfOcrRequest<'_>) -> Result<PdfOcrOutput, Error> {
        self.recognize_input(request, false).map(|page| page.output)
    }
}
impl ImageOcrProvider for NativeOcr {
    fn recognize_image_page(&self, request: PdfOcrRequest<'_>) -> Result<ImageOcrPage, Error> {
        self.recognize_input(request, true)
    }
}

// Cancellation is cooperative between native inference stages; an ONNX call
// already in progress must return before its result can be discarded.
struct Deadline {
    stop: Option<std::sync::mpsc::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Deadline {
    fn start(token: bulkparse::CancellationToken, millis: u64) -> Result<Self, Error> {
        let (stop, stopped) = std::sync::mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("bulkparse-native-ocr-deadline".into())
            .spawn(move || {
                if matches!(
                    stopped.recv_timeout(Duration::from_millis(millis)),
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout)
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
