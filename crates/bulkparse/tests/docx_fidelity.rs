#![cfg(feature = "docx")]
use bulkparse::{BlockContent, Error, Options, Parser};
use std::io::{Cursor, Write};

fn package(body: &str, parts: &[(&str, &str)]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    zip.start_file("word/document.xml", options).unwrap();
    write!(zip, r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><w:body>{body}</w:body></w:document>"#).unwrap();
    for (path, xml) in parts {
        zip.start_file(*path, options).unwrap();
        zip.write_all(xml.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
fn paragraph(text: &str) -> String {
    format!("<w:p><w:r><w:t>{text}</w:t></w:r></w:p>")
}
fn cell(props: &str, text: &str) -> String {
    format!("<w:tc><w:tcPr>{props}</w:tcPr>{}</w:tc>", paragraph(text))
}

#[test]
fn merges_follow_grid_boundaries_and_retain_covered_content() {
    let body = format!(
        r#"<w:tbl><w:tr><w:trPr><w:tblHeader w:val="false"/><w:gridBefore w:val="1"/></w:trPr>{}{}</w:tr><w:tr><w:trPr><w:gridBefore w:val="1"/></w:trPr>{}{}</w:tr></w:tbl>"#,
        cell(
            r#"<w:gridSpan w:val="2"/><w:vMerge w:val="restart"/>"#,
            "Group"
        ),
        cell("", "First"),
        cell(
            r#"<w:gridSpan w:val="2"/><w:vMerge/>"#,
            "Continuation detail"
        ),
        cell("", "Second")
    );
    let result = Parser::default()
        .parse_bytes(&package(&body, &[]), Some("form.docx"))
        .unwrap();
    let BlockContent::Table { rows, header_rows } = &result.document.blocks[0].content else {
        panic!()
    };
    assert_eq!(*header_rows, 0);
    assert_eq!((rows[0][1].col_span, rows[0][1].row_span), (2, 2));
    assert_eq!(rows[1].len(), 2); // one grid-before gap, one ordinary cell
    let md = result.to_markdown().text;
    assert!(md.contains("rowspan=\"2\""));
    assert!(md.contains("Continuation detail"));
    assert!(!result.diagnostics.iter().any(|d| d.code.contains("merge")));
}

#[test]
fn orphan_merges_do_not_consume_unrelated_cells_and_spans_obey_limits() {
    let body = format!(
        "<w:tbl><w:tr>{}</w:tr><w:tr>{}</w:tr></w:tbl>",
        cell("", "Plain"),
        cell("<w:vMerge/>", "Keep")
    );
    let result = Parser::default()
        .parse_bytes(&package(&body, &[]), Some("form.docx"))
        .unwrap();
    assert!(result
        .diagnostics
        .iter()
        .any(|d| d.code == "docx_merge_orphan"));
    let BlockContent::Table { rows, .. } = &result.document.blocks[0].content else {
        panic!()
    };
    assert_eq!(rows[0][0].row_span, 1);
    assert_eq!(rows[1].len(), 1);
    let body = format!(
        "<w:tbl><w:tr>{}</w:tr></w:tbl>",
        cell(r#"<w:gridSpan w:val="100"/>"#, "Bounded")
    );
    let mut opts = Options::default();
    opts.limits.max_table_cells = 10;
    assert!(matches!(
        Parser::new(opts).parse_bytes(&package(&body, &[]), Some("x.docx")),
        Err(Error::Limit("max_table_cells"))
    ));
}

#[test]
fn horizontal_merges_and_mismatched_vertical_spans_are_explicit() {
    let body = format!(
        "<w:tbl><w:tr>{}{}{}</w:tr><w:tr>{}{}</w:tr></w:tbl>",
        cell(
            r#"<w:hMerge w:val="restart"/><w:vMerge w:val="restart"/>"#,
            "A"
        ),
        cell("<w:hMerge/>", "B"),
        cell("", "C"),
        cell("<w:vMerge/>", "D"),
        cell("", "E")
    );
    let r = Parser::default()
        .parse_bytes(&package(&body, &[]), Some("x.docx"))
        .unwrap();
    let BlockContent::Table { rows, .. } = &r.document.blocks[0].content else {
        panic!()
    };
    assert_eq!(rows[0][0].col_span, 2);
    assert_eq!(rows[0][0].row_span, 1);
    assert!(r.diagnostics.iter().any(|d| d.code == "docx_merge_orphan"));
    for t in ["A", "B", "C", "D", "E"] {
        assert!(r.to_markdown().text.contains(t));
    }
}

#[test]
fn text_boxes_choose_one_branch_keep_blocks_and_current_revisions() {
    let body = r#"<w:p><w:r><w:t>Intro</w:t><mc:AlternateContent><mc:Choice Requires="wps"><w:drawing><w:txbxContent><w:p><w:r><w:t>Box label</w:t></w:r></w:p><w:tbl><w:tr><w:tc><w:p><w:r><w:t>Box value</w:t></w:r></w:p></w:tc></w:tr></w:tbl></w:txbxContent></w:drawing></mc:Choice><mc:Fallback><w:pict><w:txbxContent><w:p><w:r><w:t>Duplicate fallback</w:t></w:r></w:p></w:txbxContent></w:pict></mc:Fallback></mc:AlternateContent></w:r><w:del><w:r><w:delText>Deleted</w:delText></w:r></w:del><w:moveTo><w:r><w:t>Current</w:t></w:r></w:moveTo></w:p>"#;
    let r = Parser::default()
        .parse_bytes(&package(body, &[]), Some("x.docx"))
        .unwrap();
    let md = r.to_markdown().text;
    assert_eq!(md.matches("Box label").count(), 1);
    assert!(md.contains("Box value"));
    assert!(md.contains("Current"));
    assert!(!md.contains("Deleted"));
    assert!(!md.contains("Duplicate fallback"));
    assert!(r
        .document
        .blocks
        .iter()
        .any(|b| matches!(b.content, BlockContent::Table { .. })));
}

#[test]
fn referenced_headers_and_notes_keep_part_specific_links_without_unused_stories() {
    let body = r#"<w:p><w:r><w:t>Main</w:t><w:footnoteReference w:id="4"/></w:r></w:p><w:sectPr><w:headerReference r:id="h"/></w:sectPr><w:sectPr><w:headerReference r:id="h"/></w:sectPr>"#;
    let rels = r#"<Relationships><Relationship Id="h" Type="urn:rel/header" Target="header.xml"/><Relationship Id="u" Type="urn:rel/header" Target="unused.xml"/><Relationship Id="f" Type="urn:rel/footnotes" Target="notes.xml"/></Relationships>"#;
    let header = r#"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:p><w:hyperlink r:id="link"><w:r><w:t>Header link</w:t></w:r></w:hyperlink></w:p></w:hdr>"#;
    let notes = r#"<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:footnote w:type="separator" w:id="-1"><w:p/></w:footnote><w:footnote w:id="4"><w:p><w:r><w:footnoteRef/><w:t>Actual note</w:t></w:r></w:p></w:footnote><w:footnote w:id="9"><w:p><w:r><w:t>Unused note</w:t></w:r></w:p></w:footnote></w:footnotes>"#;
    let hr = r#"<Relationships><Relationship Id="link" Type="urn:rel/hyperlink" TargetMode="External" Target="https://example.test/header"/></Relationships>"#;
    let r = Parser::default()
        .parse_bytes(
            &package(
                body,
                &[
                    ("word/_rels/document.xml.rels", rels),
                    ("word/header.xml", header),
                    ("word/_rels/header.xml.rels", hr),
                    ("word/notes.xml", notes),
                ],
            ),
            Some("x.docx"),
        )
        .unwrap();
    let md = r.to_markdown().text;
    assert_eq!(md.matches("Header link").count(), 1);
    assert!(md.contains("https://example.test/header"));
    assert!(md.contains("Actual note"));
    assert!(md.contains("docx-footnote-4"));
    assert!(!md.contains("Unused note"));
    assert!(!r.diagnostics.iter().any(|d| d.code == "docx_note_missing"));
    assert!(r
        .document
        .blocks
        .iter()
        .any(|b| b.source.part.as_deref() == Some("word/notes.xml")));
}

#[cfg(feature = "office")]
#[test]
fn rtf_headers_are_separate_from_body_and_do_not_leak_formatting() {
    let r=Parser::default().parse_bytes(br"{\rtf1\ansi Body {\headerf\b First header\par}{\footer\i Footer \u233?\par}continues\par}",Some("x.rtf")).unwrap();
    let md = r.to_markdown().text;
    assert!(md.starts_with("Body continues"), "{md}");
    assert!(md.contains("**First header**"));
    assert!(md.contains("Footer"));
    assert!(md.contains('é'));
    assert!(r
        .diagnostics
        .iter()
        .any(|d| d.code == "rtf_auxiliary_layout"));
}

#[cfg(feature = "office")]
#[test]
fn malformed_rtf_auxiliary_does_not_discard_the_body() {
    let r = Parser::default()
        .parse_bytes(br"{\rtf1 Body before{\header Header after", Some("x.rtf"))
        .unwrap();
    let md = r.to_markdown().text;
    assert!(
        md.contains("Body before") && md.contains("Header after"),
        "{md}"
    );
    assert!(r.diagnostics.iter().any(|d| d.code == "rtf_unbalanced"));
}
