use wasm_bindgen::prelude::*;

/// JSON transport keeps nulls, tagged enums and integer fields identical to native bindings.
#[wasm_bindgen(js_name = parseJson)]
pub fn parse_json(
    bytes: &[u8],
    filename: Option<String>,
    options: Option<String>,
) -> Result<String, JsValue> {
    bulkparse::parse_json(bytes, filename.as_deref(), options.as_deref())
        .map_err(|e| JsValue::from_str(&bulkparse::error_json(&e)))
}

#[wasm_bindgen(js_name = capabilitiesJson)]
pub fn capabilities_json() -> String {
    serde_json::to_string(&bulkparse::capabilities()).expect("capabilities serialization")
}

#[wasm_bindgen(js_name = inspectPdfJson)]
pub fn inspect_pdf_json(bytes: &[u8], options: Option<String>) -> Result<String, JsValue> {
    bulkparse::inspect_pdf_json(bytes, options.as_deref())
        .map_err(|e| JsValue::from_str(&bulkparse::error_json(&e)))
}
