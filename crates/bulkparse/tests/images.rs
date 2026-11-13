#![cfg(all(feature = "docx", feature = "office"))]
use bulkparse::{Options, Parser};
use std::io::{Cursor, Write};

fn document() -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in [
        ("word/document.xml", &br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Preserve this text.</w:t></w:r></w:p></w:body></w:document>"#[..]),
        ("word/media/a.png", &b"\x89PNG\r\n\x1a\noriginal pixels"[..]),
        ("word/media/duplicate.png", &b"\x89PNG\r\n\x1a\noriginal pixels"[..]),
        ("word/media/vector.svg", &b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>"[..]),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
#[test]
fn originals_are_deduplicated_and_json_roundtrips_without_changing_markdown() {
    let bytes = document();
    let parsed = Parser::default()
        .parse_bytes(&bytes, Some("a.docx"))
        .unwrap();
    assert_eq!(parsed.document.assets.len(), 2);
    assert_eq!(
        &*parsed.document.assets[0].data,
        b"\x89PNG\r\n\x1a\noriginal pixels"
    );
    let json = parsed.to_json().unwrap();
    assert!(json.contains("iVBORw0K"));
    let decoded: bulkparse::ParseResult = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.document, decoded.document);
    let without = Parser::new(Options {
        extract_images: false,
        ..Default::default()
    })
    .parse_bytes(&bytes, Some("a.docx"))
    .unwrap();
    assert!(without.document.assets.is_empty());
    assert_eq!(without.to_markdown().text, parsed.to_markdown().text);
}
#[test]
fn image_limits_omit_assets_with_diagnostics_and_preserve_text() {
    let mut options = Options::default();
    options.limits.max_image_bytes = 1;
    let parsed = Parser::new(options)
        .parse_bytes(&document(), Some("a.docx"))
        .unwrap();
    assert!(parsed.document.assets.is_empty());
    assert!(parsed.to_markdown().text.contains("Preserve this text"));
    assert!(parsed
        .diagnostics
        .iter()
        .any(|d| d.code == "image_asset_unavailable"
            && d.evidence == bulkparse::DiagnosticEvidence::Detected));
    let mut options = Options::default();
    options.limits.max_images = 1;
    let parsed = Parser::new(options)
        .parse_bytes(&document(), Some("a.docx"))
        .unwrap();
    assert_eq!(parsed.document.assets.len(), 1);
}
#[test]
fn rtf_hex_binary_and_nested_metadata_preserve_original_bytes() {
    let input = b"{\\rtf1 Text {\\pict\\pngblip {\\*\\blipuid abcdef}89504e470d0a1a0a}{\\pict\\jpegblip\\bin5 {}\\ab}}";
    let parsed = Parser::default().parse_bytes(input, Some("a.rtf")).unwrap();
    assert_eq!(parsed.document.assets.len(), 2);
    assert_eq!(&*parsed.document.assets[0].data, b"\x89PNG\r\n\x1a\n");
    assert_eq!(&*parsed.document.assets[1].data, b"{}\\ab");
}
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn file_export_saves_exact_bytes_without_rewriting_markdown() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.docx");
    let output = dir.path().join("result.md");
    std::fs::write(&input, document()).unwrap();
    let parser = Parser::new(Options {
        export_images: true,
        ..Default::default()
    });
    let summary = parser.parse_file_to_markdown(&input, &output).unwrap();
    assert_eq!(summary.images.len(), 2);
    let markdown = std::fs::read_to_string(&output).unwrap();
    let parsed = parser.parse_file(&input).unwrap();
    for (saved, original) in summary.images.iter().zip(&parsed.document.assets) {
        assert!(!markdown.contains(&saved.path));
        assert_eq!(
            std::fs::read(dir.path().join(&saved.path)).unwrap(),
            &*original.data
        );
    }
    assert_eq!(markdown, parsed.to_markdown().text);
    assert!(!markdown.contains("Additional image assets"));
    let before = std::fs::read_dir(dir.path()).unwrap().count();
    assert!(parser.parse_file_to_markdown(&input, &output).is_err());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), before);
    assert_eq!(std::fs::read_to_string(&output).unwrap(), markdown);
}
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn failed_export_cleans_staged_images_and_markdown() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.docx");
    let output = dir.path().join("result.md");
    std::fs::write(&input, document()).unwrap();
    let mut options = Options {
        export_images: true,
        ..Default::default()
    };
    options.limits.max_markdown_bytes = 10;
    assert!(Parser::new(options)
        .parse_file_to_markdown(&input, &output)
        .is_err());
    assert!(!output.exists());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn repeated_docx_images_keep_reading_flow_position_including_table_cells() {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let image = r#"<w:r><w:drawing><a:blip r:embed="im"/></w:drawing></w:r>"#;
    let body = format!(
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:a="urn:a"><w:body><w:p><w:r><w:t>Before</w:t></w:r>{image}<w:r><w:t>After</w:t></w:r></w:p><w:tbl><w:tr><w:tc><w:tcPr><w:gridSpan w:val="2"/></w:tcPr><w:p>{image}</w:p></w:tc></w:tr></w:tbl></w:body></w:document>"#
    );
    for (name, bytes) in [
        ("word/document.xml", body.as_bytes()),
        ("word/_rels/document.xml.rels", &br#"<Relationships><Relationship Id="im" Type="image" Target="media/a.png"/></Relationships>"#[..]),
        ("word/media/a.png", &b"original pixels"[..]),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap(); zip.write_all(bytes).unwrap();
    }
    let bytes = zip.finish().unwrap().into_inner();
    let parsed = Parser::default()
        .parse_bytes(&bytes, Some("inline.docx"))
        .unwrap();
    assert_eq!(parsed.document.assets.len(), 1);
    assert!(parsed.to_json().unwrap().contains("asset:"));
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("inline.docx");
    let output = dir.path().join("out.md");
    std::fs::write(&input, bytes).unwrap();
    let summary = Parser::new(Options {
        export_images: true,
        ..Default::default()
    })
    .parse_file_to_markdown(&input, &output)
    .unwrap();
    let markdown = std::fs::read_to_string(output).unwrap();
    let path = &summary.images[0].path;
    assert!(!markdown.contains(path));
    assert_eq!(markdown, parsed.to_markdown().text);
    let json = parsed.to_json().unwrap();
    assert_eq!(
        json.matches(&format!("asset:{}", parsed.document.assets[0].id))
            .count(),
        2
    );
    let bulkparse::BlockContent::Paragraph { inlines } = &parsed.document.blocks[0].content else {
        panic!("missing paragraph")
    };
    assert!(matches!(&inlines[1], bulkparse::Inline::Image { .. }));
    assert!(!markdown.contains("Additional image assets"));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn rtf_export_preserves_alt_text_markdown_and_saves_asset() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("inline.rtf");
    let output = dir.path().join("out.md");
    std::fs::write(
        &input,
        br"{\rtf1 Before {\pict\pngblip 89504e470d0a1a0a} After}",
    )
    .unwrap();
    let summary = Parser::new(Options {
        export_images: true,
        ..Default::default()
    })
    .parse_file_to_markdown(&input, &output)
    .unwrap();
    let markdown = std::fs::read_to_string(output).unwrap();
    assert!(!markdown.contains(&summary.images[0].path));
    assert_eq!(
        markdown,
        Parser::default()
            .parse_file(&input)
            .unwrap()
            .to_markdown()
            .text
    );
    assert_eq!(
        std::fs::read(dir.path().join(&summary.images[0].path)).unwrap(),
        b"\x89PNG\r\n\x1a\n"
    );
    assert!(!markdown.contains("Additional image assets"));
}
