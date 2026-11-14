#![cfg(feature = "pdf")]
use bulkparse::*;
use serde_json::{json, Value};
fn evidence() -> Value {
    json!({"page_count":2,"pages":[
        {"page":1,"output":{"type":"text","value":"First frame"}},
        {"page":2,"output":{"type":"text","value":"Second frame"}}
    ]})
}
fn parse(evidence: Value, options: Value, budget: Value) -> Result<Conversion, Error> {
    let json = parse_with_ocr_json(
        b"II\x2a\0fake-host-decoded-image",
        Some("image.tiff"),
        Some(&options.to_string()),
        &evidence.to_string(),
        Some(&budget.to_string()),
    )?;
    Ok(serde_json::from_str(&json).unwrap())
}
#[test]
fn image_evidence_preserves_frame_order_and_coverage() {
    let result = parse(evidence(), json!({}), json!({})).unwrap();
    assert_eq!(result.result.document.format, Format::Image);
    assert_eq!(result.result.coverage.len(), 2);
    assert_eq!(result.result.document.blocks[1].source.page, Some(2));
    assert!(result.markdown.find("First").unwrap() < result.markdown.find("Second").unwrap());
}
#[test]
fn skipped_frames_are_partial_and_strict_mode_rejects_them() {
    let result = parse(evidence(), json!({}), json!({"max_pages":1})).unwrap();
    assert_eq!(result.result.status, Status::Partial);
    assert_eq!(result.result.coverage[1].status, Status::Partial);
    assert!(!result.markdown.contains("Second"));
    assert!(matches!(
        parse(evidence(), json!({"strict":true}), json!({"max_pages":1})),
        Err(Error::Incomplete(_))
    ));
}
#[test]
fn duplicate_missing_invalid_and_oversized_evidence_is_rejected() {
    let mut value = evidence();
    value["pages"][1]["page"] = json!(1);
    assert!(parse(value, json!({}), json!({})).is_err());
    let mut value = evidence();
    value["pages"].as_array_mut().unwrap().pop();
    assert!(parse(value, json!({}), json!({})).is_err());
    assert!(matches!(
        parse(evidence(), json!({"limits":{"max_pages":1}}), json!({})),
        Err(Error::Limit(_))
    ));
    assert!(matches!(
        parse(evidence(), json!({}), json!({"max_output_bytes":1})),
        Err(Error::Limit(_))
    ));
    assert!(matches!(
        parse(evidence(), json!({}), json!({"max_pages":0})),
        Err(Error::Limit(_))
    ));
    assert!(parse(evidence(), json!({"format":"pdf"}), json!({})).is_err());
}
#[test]
fn native_images_never_return_empty_success_without_ocr() {
    assert!(matches!(
        Parser::default().parse_bytes(b"\x89PNG\r\n\x1a\n", Some("a.png")),
        Err(Error::Unsupported(_))
    ));
}
#[test]
fn force_ocr_keeps_inspection_evidence_and_replaces_native_text() {
    let bytes = include_bytes!("../../../fixtures/native-and-empty.pdf");
    let result = parse_with_ocr_json(
        bytes,
        Some("a.pdf"),
        Some(r#"{"pdf_force_ocr":true}"#),
        &evidence().to_string(),
        None,
    )
    .unwrap();
    let result: Conversion = serde_json::from_str(&result).unwrap();
    assert!(result.markdown.contains("First frame"));
    assert_eq!(
        result.result.pdf_inspection.unwrap().pages[0].ocr_status,
        PdfOcrStatus::Applied
    );
}
