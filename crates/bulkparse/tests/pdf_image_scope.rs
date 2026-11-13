#![cfg(feature = "pdf")]
use bulkparse::{Options, Parser};
const PDF: &[u8] = include_bytes!("../../../fixtures/inspection/header-over-scan.pdf");

#[test]
fn pdf_images_inform_inspection_without_becoming_exported_assets() {
    let parser = Parser::default();
    let result = parser.parse_bytes(PDF, Some("document.pdf")).unwrap();
    assert!(result.document.assets.is_empty());
    assert!(result
        .pdf_inspection
        .as_ref()
        .unwrap()
        .pages
        .iter()
        .any(|p| p.image_count > 0));
    assert!(!result.diagnostics.iter().any(|d| matches!(
        d.code.as_str(),
        "image_assets_retained" | "image_asset_unavailable"
    )));
    let without = Parser::new(Options {
        extract_images: false,
        ..Default::default()
    })
    .parse_bytes(PDF, Some("document.pdf"))
    .unwrap();
    assert_eq!(result.to_json().unwrap(), without.to_json().unwrap());
    let mut options = Options::default();
    options.limits.max_image_bytes = 0;
    assert_eq!(
        result.to_json().unwrap(),
        Parser::new(options)
            .parse_bytes(PDF, Some("document.pdf"))
            .unwrap()
            .to_json()
            .unwrap()
    );
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn pdf_file_output_creates_no_image_directory_even_when_export_is_requested() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("source.pdf");
    let output = temp.path().join("output.md");
    std::fs::write(&input, PDF).unwrap();
    let summary = Parser::new(Options {
        export_images: true,
        ..Default::default()
    })
    .parse_file_to_markdown(&input, &output)
    .unwrap();
    assert_eq!(summary.image_count, 0);
    assert!(summary.images.is_empty());
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 2);
    let expected = Parser::default()
        .parse_bytes(PDF, Some("source.pdf"))
        .unwrap()
        .to_markdown()
        .text;
    assert_eq!(std::fs::read_to_string(output).unwrap(), expected);
    assert!(summary
        .pdf_inspection
        .unwrap()
        .pages
        .iter()
        .any(|p| p.image_count > 0));
}
