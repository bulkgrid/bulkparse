use bulkparse::{BlockContent, CancellationToken, Error, Format, Options, Parser, Status};

fn parse(bytes: &[u8], name: &str) -> bulkparse::ParseResult {
    Parser::default().parse_bytes(bytes, Some(name)).unwrap()
}

#[test]
fn text_is_not_interpreted_as_markdown() {
    let result = parse(b"# literal\n\n<script>alert(1)</script>", "note.txt");
    assert_eq!(result.document.blocks.len(), 2);
    assert!(matches!(
        result.document.blocks[0].content,
        BlockContent::Paragraph { .. }
    ));
    assert!(result.to_markdown().text.contains("\\# literal"));
    assert!(!result.to_markdown().text.contains("<script>"));
}

#[test]
fn markdown_preserves_nested_structure_and_table() {
    let result = parse(
        b"# Title\n\n- First\n  - Nested\n\n> Quote\n\n| A | B |\n|---|---|\n| one | two |\n",
        "readme.md",
    );
    assert!(matches!(
        result.document.blocks[0].content,
        BlockContent::Heading { level: 1, .. }
    ));
    assert!(result
        .document
        .blocks
        .iter()
        .any(|b| matches!(b.content, BlockContent::List { .. })));
    assert!(result
        .document
        .blocks
        .iter()
        .any(|b| matches!(b.content, BlockContent::Quote { .. })));
    let table = result
        .document
        .blocks
        .iter()
        .find(|b| matches!(b.content, BlockContent::Table { .. }))
        .unwrap();
    if let BlockContent::Table { rows, .. } = &table.content {
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].len(), 2);
    }
    assert_eq!(result.status, Status::Complete);
}

#[test]
fn code_fences_cannot_be_closed_by_source_text() {
    let result = parse(b"````rust\n```\nlet x = 1;\n````\n", "code.md");
    assert!(result.to_markdown().text.starts_with("````rust"));
}

#[test]
fn dangerous_links_are_inert_but_preserved_in_json() {
    let result = parse(b"[click](javascript:alert%281%29)", "x.md");
    assert!(result.to_json().unwrap().contains("javascript:"));
    assert!(!result.to_markdown().text.contains("javascript:"));
}

#[test]
fn percent_encoded_link_destinations_are_not_double_encoded() {
    let result = parse(b"[file](https://example.test/a%20b)", "x.md");
    assert!(result
        .to_markdown()
        .text
        .contains("https://example.test/a%20b"));
    assert!(!result.to_markdown().text.contains("%2520"));
}

#[test]
fn csv_keeps_first_row_and_quoted_newlines() {
    let result = parse(b"alpha,\"line1\nline2\"\nbeta,\"x,y\"\n", "data.csv");
    let BlockContent::Table { rows, header_rows } = &result.document.blocks[0].content else {
        panic!("table expected")
    };
    assert_eq!(*header_rows, 0);
    assert_eq!(rows.len(), 2);
    let md = result.to_markdown().text;
    assert!(md.contains("alpha"));
    assert!(md.contains("line1<br>line2"));
    assert!(md.contains("x,y"));
}

#[test]
fn csv_rejects_ragged_rows_without_silent_loss() {
    assert!(matches!(
        Parser::default().parse_bytes(b"a,b\nc\n", Some("x.csv")),
        Err(Error::Malformed(_))
    ));
}

#[test]
fn tsv_is_explicit_and_utf8_is_validated() {
    let result = parse("名称\t値\n一\t二".as_bytes(), "x.tsv");
    assert!(result.to_markdown().text.contains("名称"));
    assert!(Parser::default()
        .parse_bytes(&[0xff], Some("x.txt"))
        .is_err());
    assert!(matches!(
        Parser::default().parse_bytes(b"a,b", None),
        Err(Error::Unsupported(_))
    ));
}

#[test]
fn limits_and_cancellation_apply_before_work() {
    let mut options = Options::default();
    options.limits.max_input_bytes = 2;
    assert!(matches!(
        Parser::new(options).parse_bytes(b"abc", Some("x.txt")),
        Err(Error::Limit("max_input_bytes"))
    ));
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert!(matches!(
        Parser::default().parse_with_cancel(b"a", Some("x.txt"), &cancel),
        Err(Error::Cancelled)
    ));
}

#[test]
fn cell_and_nesting_limits_are_enforced() {
    let mut options = Options::default();
    options.limits.max_table_cells = 1;
    assert!(matches!(
        Parser::new(options).parse_bytes(b"a,b", Some("x.csv")),
        Err(Error::Limit("max_table_cells"))
    ));
    let mut options = Options::default();
    options.limits.max_depth = 2;
    assert!(matches!(
        Parser::new(options).parse_bytes(b"> > > deep", Some("x.md")),
        Err(Error::Limit("max_depth"))
    ));
}

#[test]
fn notebook_does_not_execute_code_and_reports_rich_outputs() {
    let bytes = br#"{"nbformat":4,"cells":[{"cell_type":"code","source":["print('safe')"],"outputs":[{"output_type":"display_data","data":{"image/png":"unused"}}]}]}"#;
    let result = parse(bytes, "demo.ipynb");
    assert_eq!(result.status, Status::Partial);
    assert_eq!(result.document.blocks[0].source.cell, Some(1));
    assert!(result.to_markdown().text.contains("print('safe')"));
    let options = Options {
        strict: true,
        ..Default::default()
    };
    assert!(matches!(
        Parser::new(options).parse_bytes(bytes, Some("demo.ipynb")),
        Err(Error::Incomplete(_))
    ));
}

#[test]
fn json_roundtrip_and_binding_contract_match() {
    let result = parse(b"# Test", "x.md");
    let restored: bulkparse::ParseResult =
        serde_json::from_str(&result.to_json().unwrap()).unwrap();
    assert_eq!(result.document, restored.document);
    let wire = bulkparse::parse_json(b"# Test", Some("x.md"), None).unwrap();
    let value: serde_json::Value = serde_json::from_str(&wire).unwrap();
    assert_eq!(
        value["document"],
        serde_json::to_value(result.document).unwrap()
    );
    assert!(value["markdown"].as_str().unwrap().contains("# Test"));
    assert!(bulkparse::parse_json(b"x", Some("x.txt"), Some(r#"{"strcit":true}"#)).is_err());
}

#[test]
fn explicit_binary_format_requires_valid_signature() {
    let options = Options {
        format: Some(Format::Pdf),
        ..Default::default()
    };
    assert!(matches!(
        Parser::new(options).parse_bytes(b"not a pdf", None),
        Err(Error::Malformed(_))
    ));
}

#[cfg(feature = "docx")]
fn docx(main: &str, extra: &[(&str, &str)]) -> Vec<u8> {
    use std::io::{Cursor, Write};
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("word/document.xml", options).unwrap();
    zip.write_all(main.as_bytes()).unwrap();
    for (path, text) in extra {
        zip.start_file(*path, options).unwrap();
        zip.write_all(text.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

#[cfg(feature = "docx")]
#[test]
fn docx_uses_namespaces_headings_tables_and_entities() {
    let xml = r#"<z:document xmlns:z="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><z:body><z:p><z:pPr><z:pStyle z:val="Heading2"/></z:pPr><z:r><z:t>A &amp; B</z:t></z:r></z:p><z:tbl><z:tr><z:tc><z:p><z:r><z:t>Value</z:t></z:r></z:p></z:tc></z:tr></z:tbl></z:body></z:document>"#;
    let result = parse(&docx(xml, &[]), "misnamed.txt");
    assert_eq!(result.document.format, Format::Docx);
    assert!(matches!(
        result.document.blocks[0].content,
        BlockContent::Heading { level: 2, .. }
    ));
    assert!(result.to_markdown().text.contains("## A & B"));
    assert!(result.to_markdown().text.contains("Value"));
    assert_eq!(
        result.document.blocks[0].source.part.as_deref(),
        Some("word/document.xml")
    );
}

#[cfg(feature = "docx")]
#[test]
fn docx_detects_content_gaps_and_expansion_limits() {
    let xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Text</w:t><w:drawing/></w:r></w:p></w:body></w:document>"#;
    let bytes = docx(xml, &[]);
    assert_eq!(parse(&bytes, "x.docx").status, Status::Partial);
    let mut options = Options::default();
    options.limits.max_expanded_bytes = 5;
    assert!(matches!(
        Parser::new(options).parse_bytes(&bytes, Some("x.docx")),
        Err(Error::Limit("max_expanded_bytes"))
    ));
}

#[cfg(feature = "docx")]
#[test]
fn docx_depth_limit_tracks_ancestors_not_previous_siblings() {
    let paragraphs = "<w:p><w:r><w:t>Text</w:t></w:r></w:p>".repeat(32);
    let xml = format!(
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{paragraphs}</w:body></w:document>"#
    );
    let bytes = docx(&xml, &[]);
    let mut options = Options::default();
    // Root, document, body, paragraph, run, text element, text node.
    options.limits.max_depth = 7;
    assert_eq!(
        Parser::new(options.clone())
            .parse_bytes(&bytes, Some("x.docx"))
            .unwrap()
            .document
            .blocks
            .len(),
        32
    );
    options.limits.max_depth = 6;
    assert!(matches!(
        Parser::new(options).parse_bytes(&bytes, Some("x.docx")),
        Err(Error::Limit("max_depth"))
    ));
}

#[cfg(feature = "docx")]
#[test]
fn docx_rejects_dtd_and_relationship_traversal() {
    let xml = r#"<!DOCTYPE x [<!ENTITY x "secret">]><x/>"#;
    assert!(Parser::default()
        .parse_bytes(&docx(xml, &[]), Some("x.docx"))
        .is_err());
    let rels = r#"<Relationships><Relationship Id="r1" Type="http://example/officeDocument" Target="../../escape.xml"/></Relationships>"#;
    assert!(Parser::default()
        .parse_bytes(&docx("<x/>", &[("_rels/.rels", rels)]), Some("x.docx"))
        .is_err());
}

#[cfg(feature = "pdf")]
#[test]
fn pdf_pages_share_the_model_and_never_claim_full_fidelity() {
    let bytes = include_bytes!("../../../fixtures/native-and-empty.pdf");
    let result = parse(bytes, "x.pdf");
    assert_eq!(result.status, Status::Partial);
    assert_eq!(result.coverage.len(), 2);
    assert!(result.to_markdown().text.contains("Hello BulkParse"));
    assert!(result
        .diagnostics
        .iter()
        .any(|d| d.code == "pdf_no_text" && d.source.page == Some(2)));
}
