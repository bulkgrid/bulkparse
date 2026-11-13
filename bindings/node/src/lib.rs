use napi::bindgen_prelude::*;
use napi_derive::napi;

pub struct ConvertTask {
    inspect: bool,
    bytes: Vec<u8>,
    filename: Option<String>,
    options: Option<String>,
}

impl Task for ConvertTask {
    type Output = String;
    type JsValue = String;
    fn compute(&mut self) -> Result<Self::Output> {
        if self.inspect {
            return bulkparse::inspect_pdf_json(&self.bytes, self.options.as_deref())
                .map_err(|e| Error::from_reason(bulkparse::error_json(&e)));
        }
        bulkparse::parse_json(
            &self.bytes,
            self.filename.as_deref(),
            self.options.as_deref(),
        )
        .map_err(|e| Error::from_reason(bulkparse::error_json(&e)))
    }
    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output)
    }
}

/// CPU work runs in libuv's worker pool, not on Node's event loop.
#[napi]
pub fn parse_json(
    bytes: Buffer,
    filename: Option<String>,
    options: Option<String>,
) -> AsyncTask<ConvertTask> {
    AsyncTask::new(ConvertTask {
        inspect: false,
        bytes: bytes.to_vec(),
        filename,
        options,
    })
}

#[napi]
pub fn capabilities_json() -> String {
    serde_json::to_string(&bulkparse::capabilities()).expect("capabilities serialization")
}

#[napi]
pub fn inspect_pdf_json(bytes: Buffer, options: Option<String>) -> AsyncTask<ConvertTask> {
    AsyncTask::new(ConvertTask {
        inspect: true,
        bytes: bytes.to_vec(),
        filename: None,
        options,
    })
}

pub struct MarkdownFileTask {
    input: String,
    output: String,
    options: Option<String>,
}
impl Task for MarkdownFileTask {
    type Output = String;
    type JsValue = String;
    fn compute(&mut self) -> Result<String> {
        let options = self
            .options
            .as_deref()
            .map(serde_json::from_str::<bulkparse::Options>)
            .transpose()
            .map_err(|e| {
                Error::from_reason(bulkparse::error_json(&bulkparse::Error::Malformed(
                    format!("invalid options: {e}"),
                )))
            })?
            .unwrap_or_default();
        let summary = bulkparse::Parser::new(options)
            .parse_file_to_markdown(&self.input, &self.output)
            .map_err(|e| Error::from_reason(bulkparse::error_json(&e)))?;
        serde_json::to_string(&summary).map_err(|e| Error::from_reason(e.to_string()))
    }
    fn resolve(&mut self, _env: Env, output: String) -> Result<String> {
        Ok(output)
    }
}

/// Reads capped input and writes Markdown in the worker; no JS input, model or
/// Markdown buffers cross the binding. Existing destination files are preserved.
#[napi]
pub fn parse_file_to_markdown_json(
    input: String,
    output: String,
    options: Option<String>,
) -> AsyncTask<MarkdownFileTask> {
    AsyncTask::new(MarkdownFileTask {
        input,
        output,
        options,
    })
}
