use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

#[pyfunction]
#[pyo3(signature = (bytes, filename=None, options=None))]
fn parse_json(
    py: Python<'_>,
    bytes: Vec<u8>,
    filename: Option<String>,
    options: Option<String>,
) -> PyResult<String> {
    py.detach(move || bulkparse::parse_json(&bytes, filename.as_deref(), options.as_deref()))
        .map_err(|e| PyValueError::new_err(bulkparse::error_json(&e)))
}

#[pyfunction]
fn capabilities_json() -> String {
    serde_json::to_string(&bulkparse::capabilities()).expect("capabilities serialization")
}

#[pymodule]
fn _native(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(parse_json, module)?)?;
    module.add_function(wrap_pyfunction!(parse_with_ocr_json, module)?)?;
    module.add_class::<NativeOcrParser>()?;
    module.add_function(wrap_pyfunction!(parse_file_to_markdown_json, module)?)?;
    module.add_function(wrap_pyfunction!(inspect_pdf_json, module)?)?;
    module.add_function(wrap_pyfunction!(capabilities_json, module)?)?;
    Ok(())
}

#[pyfunction]
#[pyo3(signature = (bytes, options=None))]
fn inspect_pdf_json(py: Python<'_>, bytes: Vec<u8>, options: Option<String>) -> PyResult<String> {
    py.detach(move || bulkparse::inspect_pdf_json(&bytes, options.as_deref()))
        .map_err(|e| PyValueError::new_err(bulkparse::error_json(&e)))
}

#[pyfunction]
#[pyo3(signature = (input, output, options=None))]
fn parse_file_to_markdown_json(
    py: Python<'_>,
    input: String,
    output: String,
    options: Option<String>,
) -> PyResult<String> {
    py.detach(move || {
        let options = options
            .as_deref()
            .map(serde_json::from_str::<bulkparse::Options>)
            .transpose()
            .map_err(|e| bulkparse::Error::Malformed(format!("invalid options: {e}")))?
            .unwrap_or_default();
        let summary = bulkparse::Parser::new(options).parse_file_to_markdown(input, output)?;
        serde_json::to_string(&summary).map_err(|e| bulkparse::Error::Malformed(e.to_string()))
    })
    .map_err(|e| PyValueError::new_err(bulkparse::error_json(&e)))
}

#[pyfunction]
#[pyo3(signature = (bytes, evidence, filename=None, options=None, budget=None))]
fn parse_with_ocr_json(
    py: Python<'_>,
    bytes: Vec<u8>,
    evidence: String,
    filename: Option<String>,
    options: Option<String>,
    budget: Option<String>,
) -> PyResult<String> {
    py.detach(move || {
        bulkparse::parse_with_ocr_json(
            &bytes,
            filename.as_deref(),
            options.as_deref(),
            &evidence,
            budget.as_deref(),
        )
    })
    .map_err(|e| PyValueError::new_err(bulkparse::error_json(&e)))
}

#[pyclass]
struct NativeOcrParser {
    provider: std::sync::Mutex<Option<std::sync::Arc<bulkparse_ocr_native::NativeOcr>>>,
}
#[pymethods]
impl NativeOcrParser {
    #[new]
    fn new(config: String) -> PyResult<Self> {
        let provider = bulkparse_ocr_native::NativeOcr::from_json(config.as_bytes())
            .map_err(|e| PyValueError::new_err(bulkparse::error_json(&e)))?;
        Ok(Self {
            provider: std::sync::Mutex::new(Some(std::sync::Arc::new(provider))),
        })
    }
    #[pyo3(signature = (bytes, filename=None, options=None, budget=None))]
    fn parse_json(
        &self,
        py: Python<'_>,
        bytes: Vec<u8>,
        filename: Option<String>,
        options: Option<String>,
        budget: Option<String>,
    ) -> PyResult<String> {
        let provider = self
            .provider
            .lock()
            .map_err(|_| PyValueError::new_err("OCR parser lock poisoned"))?
            .clone()
            .ok_or_else(|| {
                PyValueError::new_err(bulkparse::error_json(&bulkparse::Error::Malformed(
                    "OCR parser is closed".into(),
                )))
            })?;
        py.detach(move || {
            provider.parse_json(
                &bytes,
                filename.as_deref(),
                options.as_deref(),
                budget.as_deref(),
            )
        })
        .map_err(|e| PyValueError::new_err(bulkparse::error_json(&e)))
    }
    fn close(&self) -> PyResult<()> {
        *self
            .provider
            .lock()
            .map_err(|_| PyValueError::new_err("OCR parser lock poisoned"))? = None;
        Ok(())
    }
}
