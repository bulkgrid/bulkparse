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
