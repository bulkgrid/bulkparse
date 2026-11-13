#![cfg(not(target_arch = "wasm32"))]
use bulkparse::{CancellationToken, Error, Options, Parser};
use std::io::Write;

#[test]
fn default_streams_every_csv_record_without_recovery() {
    let mut csv = "value,1\n".repeat(100_001);
    csv.push_str("FINAL,2\n");
    let mut out = Vec::new();
    assert!(matches!(
        Parser::new(Options {
            limits: bulkparse::Limits::conservative(),
            ..Default::default()
        })
        .parse_to_markdown(csv.as_bytes(), Some("large.csv"), &mut out),
        Err(Error::Limit(_))
    ));
    assert!(out.is_empty());
    let options = Options::default();
    let summary = Parser::new(options)
        .parse_to_markdown(csv.as_bytes(), Some("large.csv"), &mut out)
        .unwrap();
    let text = String::from_utf8(out).unwrap();
    assert_eq!(summary.status, bulkparse::Status::Complete);
    assert!(summary.diagnostics.is_empty());
    assert_eq!(text.lines().count(), 100_004); // data plus generated header/separator
    assert!(text.ends_with("| FINAL | 2 |\n"));
    let normal = bulkparse::Limits::default();
    let large = bulkparse::Limits::large_spreadsheets();
    assert_eq!(normal.max_input_bytes, large.max_input_bytes);
    assert_eq!(normal.max_text_bytes, large.max_text_bytes);
    assert_eq!(normal.max_depth, large.max_depth);
}

#[test]
fn markdown_byte_limit_counts_markup_and_preserves_atomic_failure() {
    let bytes = b"a,b\n";
    let expected = Parser::default()
        .parse_bytes(bytes, Some("a.csv"))
        .unwrap()
        .to_markdown()
        .text;
    for cap in [0, expected.len() - 1, expected.len()] {
        let mut options = Options {
            partial_on_limit: true,
            ..Default::default()
        };
        options.limits.max_markdown_bytes = cap;
        let mut out = b"existing".to_vec();
        let result = Parser::new(options).parse_to_markdown(bytes, Some("a.csv"), &mut out);
        if cap < expected.len() {
            assert!(matches!(result, Err(Error::Limit("max_markdown_bytes"))));
            assert_eq!(out, b"existing");
        } else {
            assert_eq!(result.unwrap().bytes_written, cap as u64);
            assert_eq!(&out[8..], expected.as_bytes());
        }
    }
    // Fail after the output buffer has already spilled, not just at final flush.
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("large.csv");
    let output = dir.path().join("large.md");
    std::fs::write(&input, "a,b\n".repeat(10_000)).unwrap();
    let mut options = Options::default();
    options.limits.max_markdown_bytes = 70_000;
    assert!(matches!(
        Parser::new(options).parse_file_to_markdown(input, &output),
        Err(Error::Limit("max_markdown_bytes"))
    ));
    assert!(!output.exists());
}

#[test]
fn renderer_keeps_diagnostics_for_externally_supplied_zero_spans() {
    let mut result = Parser::default()
        .parse_bytes(b"a,b\n", Some("a.csv"))
        .unwrap();
    if let bulkparse::BlockContent::Table { rows, .. } = &mut result.document.blocks[0].content {
        rows[0][0].col_span = 0;
    }
    let output = result.to_markdown();
    assert_eq!(output.text, "|  |  |\n| --- | --- |\n| a | b |\n");
    assert_eq!(output.diagnostics[0].code, "markdown_table_spans");
    assert!(output.diagnostics[0].content_loss);
}

fn same(bytes: &[u8], name: &str, options: Options) {
    let parser = Parser::new(options);
    let result = parser.parse_bytes(bytes, Some(name)).unwrap();
    let rendered = result.to_markdown();
    let mut output = Vec::new();
    let summary = parser
        .parse_to_markdown(bytes, Some(name), &mut output)
        .unwrap();
    assert_eq!(output, rendered.text.as_bytes());
    assert_eq!(summary.bytes_written, output.len() as u64);
    assert_eq!(summary.status, result.status);
    assert_eq!(summary.format, result.document.format);
    assert_eq!(
        serde_json::to_value(summary.diagnostics).unwrap(),
        serde_json::to_value(result.diagnostics).unwrap()
    );
    assert_eq!(
        serde_json::to_value(summary.coverage).unwrap(),
        serde_json::to_value(result.coverage).unwrap()
    );
    assert_eq!(
        serde_json::to_value(summary.render_diagnostics).unwrap(),
        serde_json::to_value(rendered.diagnostics).unwrap()
    );
}

#[test]
fn csv_record_stream_preserves_encoding_headers_escaping_and_multiline_values() {
    for headers in [false, true] {
        let options = Options {
            csv_has_headers: headers,
            ..Default::default()
        };
        same(
            "Name;Value\n\"東京 | *x*\";\"first\nsecond\"\nempty;\n".as_bytes(),
            "test.csv",
            options.clone(),
        );
        same(b"name,value\nprice,\x80 12\n", "test.csv", options.clone());
        same(b"a\tb\n1\t2\n", "test.tsv", options.clone());
        let utf16: Vec<u8> = [0xff, 0xfe]
            .into_iter()
            .chain("a,b\n東京,2\n".encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        same(&utf16, "test.csv", options.clone());
        same(b"", "empty.csv", options);
    }
}

#[test]
fn large_success_spills_output_without_changing_bytes() {
    let csv = "one,two,three\n".repeat(8_000);
    same(csv.as_bytes(), "large.csv", Options::default());
}

#[test]
fn recovery_discards_the_first_attempt_and_keeps_exact_resource_boundaries() {
    for headers in [false, true] {
        let mut options = Options {
            partial_on_limit: true,
            csv_has_headers: headers,
            ..Default::default()
        };
        options.limits.max_nodes = 5;
        same(b"first\nsecond\nthird\nfourth\n", "test.csv", options);
    }
}

#[test]
fn parse_limit_and_strict_failures_do_not_publish_prefixes() {
    let mut options = Options::default();
    options.limits.max_nodes = 4;
    for options in [
        options.clone(),
        Options {
            strict: true,
            partial_on_limit: true,
            ..options
        },
    ] {
        let mut out = b"existing".to_vec();
        assert!(matches!(
            Parser::new(options).parse_to_markdown(b"a\nb\nc\n", Some("a.csv"), &mut out),
            Err(Error::Limit(_))
        ));
        assert_eq!(out, b"existing");
    }
    let mut out = Vec::new();
    assert!(Parser::default()
        .parse_to_markdown(b"a,b\n1,2\n3,4,5\n", Some("bad.csv"), &mut out)
        .is_err());
    assert!(out.is_empty());
}

#[test]
fn cancellation_and_writer_failures_propagate() {
    let cancel = CancellationToken::default();
    cancel.cancel();
    let mut out = Vec::new();
    assert!(matches!(
        Parser::default().parse_to_markdown_with_cancel(b"a,b\n", Some("a.csv"), &mut out, &cancel),
        Err(Error::Cancelled)
    ));
    assert!(out.is_empty());
    struct Broken;
    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    assert!(matches!(
        Parser::default().parse_to_markdown(b"a,b\n", Some("a.csv"), &mut Broken),
        Err(Error::Io(_))
    ));
}

#[test]
fn file_output_is_atomic_and_does_not_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.csv");
    let output = dir.path().join("output.md");
    std::fs::write(&input, "a,b\n1,2\n").unwrap();
    let parser = Parser::default();
    parser.parse_file_to_markdown(&input, &output).unwrap();
    let expected = std::fs::read(&output).unwrap();
    assert!(parser.parse_file_to_markdown(&input, &output).is_err());
    assert_eq!(std::fs::read(&output).unwrap(), expected);
    std::fs::write(&input, "a,b\n1,2,3\n").unwrap();
    let failed = dir.path().join("failed.md");
    assert!(parser.parse_file_to_markdown(&input, &failed).is_err());
    assert!(!failed.exists());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
}

#[cfg(feature = "office")]
fn zip(parts: &[(&str, &str)]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, text) in parts {
        zip.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(text.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

#[cfg(feature = "office")]
#[test]
fn worksheet_provenance_is_shared_without_changing_json() {
    let bytes = zip(&[
        ("xl/workbook.xml", r#"<workbook xmlns:r="urn:r"><sheets><sheet name="S" r:id="a"/></sheets></workbook>"#),
        ("xl/_rels/workbook.xml.rels", r#"<Relationships><Relationship Id="a" Type="worksheet" Target="a.xml"/></Relationships>"#),
        ("xl/a.xml", "<worksheet><sheetData><row><c r=\"A1\"><v>1</v></c><c r=\"B1\"><v>2</v></c></row></sheetData></worksheet>"),
    ]);
    let result = Parser::default()
        .parse_bytes(&bytes, Some("s.xlsx"))
        .unwrap();
    let bulkparse::BlockContent::Table { rows, .. } = &result.document.blocks[1].content else {
        panic!("table")
    };
    let part = rows[0][0].source.part.as_ref().unwrap();
    for cell in &rows[0] {
        assert!(std::sync::Arc::ptr_eq(
            part,
            cell.source.part.as_ref().unwrap()
        ));
        assert!(std::sync::Arc::ptr_eq(
            part,
            cell.blocks[0].source.part.as_ref().unwrap()
        ));
    }
    let json = serde_json::to_value(&result).unwrap();
    assert_eq!(
        json["document"]["blocks"][1]["rows"][0][0]["source"]["part"],
        "xl/a.xml"
    );
    same(&bytes, "s.xlsx", Options::default());
}

#[cfg(feature = "office")]
#[test]
fn repetition_budget_is_cumulative_and_never_automatically_raised() {
    let row = "<table-row><table-cell number-columns-repeated=\"3\" value=\"1\"/></table-row>";
    let source = format!("<document-content><body><spreadsheet><table name=\"A\">{row}</table><table name=\"B\">{row}</table></spreadsheet></body></document-content>");
    let bytes = zip(&[
        ("mimetype", "application/vnd.oasis.opendocument.spreadsheet"),
        ("content.xml", &source),
    ]);
    let mut options = Options::default();
    options.limits.max_cell_repetitions = 3;
    let parser = Parser::new(options.clone());
    assert!(matches!(
        parser.parse_bytes(&bytes, Some("s.ods")),
        Err(Error::Limit("max_cell_repetitions"))
    ));
    let mut output = Vec::new();
    assert!(matches!(
        parser.parse_to_markdown(&bytes, Some("s.ods"), &mut output),
        Err(Error::Limit("max_cell_repetitions"))
    ));
    assert!(output.is_empty());
    options.limits.max_cell_repetitions = 4;
    same(&bytes, "s.ods", options);
}

#[cfg(feature = "office")]
#[test]
fn workbook_stream_preserves_merges_styles_sheet_order_and_late_recovery() {
    let workbook = r#"<workbook xmlns:r="urn:r"><sheets><sheet name="First" r:id="a"/><sheet name="Second" r:id="b"/></sheets></workbook>"#;
    let rels = r#"<Relationships><Relationship Id="a" Type="worksheet" Target="a.xml"/><Relationship Id="b" Type="worksheet" Target="b.xml"/></Relationships>"#;
    let sheet = r#"<worksheet><sheetData><row><c r="A1" t="inlineStr"><is><r><rPr><b/></rPr><t>Heading</t></r></is></c></row><row r="2"><c r="B2"><v>12</v></c></row></sheetData><mergeCells><mergeCell ref="A1:B1"/></mergeCells></worksheet>"#;
    let second = format!(
        "<worksheet><sheetData>{}</sheetData></worksheet>",
        (1..80)
            .map(|i| format!("<row r=\"{i}\"><c r=\"A{i}\"><v>{i}</v></c></row>"))
            .collect::<String>()
    );
    let bytes = zip(&[
        ("xl/workbook.xml", workbook),
        ("xl/_rels/workbook.xml.rels", rels),
        ("xl/a.xml", sheet),
        ("xl/b.xml", &second),
    ]);
    same(&bytes, "two.xlsx", Options::default());
    let mut options = Options {
        partial_on_limit: true,
        ..Default::default()
    };
    options.limits.max_nodes = 200;
    same(&bytes, "two.xlsx", options);
}

#[cfg(feature = "office")]
#[test]
fn xlsx_stream_decorates_original_cells_and_preserves_limit_order() {
    let workbook = r#"<workbook xmlns:r="urn:r"><sheets><sheet name="Decorated" r:id="a"/></sheets></workbook>"#;
    let rels =
        r#"<Relationships><Relationship Id="a" Type="worksheet" Target="a.xml"/></Relationships>"#;
    let sheet = r#"<worksheet xmlns:r="urn:r"><sheetData><row r="1"><c r="A1" t="inlineStr"><is><r><rPr><b/></rPr><t>Anchor &amp; |</t></r></is></c><c r="B1" t="inlineStr"><is><r><rPr><i/></rPr><t>Covered one</t></r></is></c></row><row r="2"><c r="A2"><v>12.5</v></c><c r="B2" t="inlineStr"><is><r><rPr><b/></rPr><t>東京 two</t></r><r><t> tail</t></r></is></c></row><row r="3"><c r="A3" t="inlineStr"><is><t>Final</t></is></c></row><row r="4" hidden="1"><c r="A4" t="inlineStr"><is><t>SECRET</t></is></c></row></sheetData><mergeCells><mergeCell ref="A1:B2"/></mergeCells><hyperlinks><hyperlink ref="B2" r:id="specific"/><hyperlink ref="A1:B2" r:id="general"/><hyperlink ref="A3" location="target"/><hyperlink ref="D3" r:id="missing"/></hyperlinks></worksheet>"#;
    let links = r#"<Relationships><Relationship Id="specific" Type="hyperlink" Target="https://example.test/covered" TargetMode="External"/><Relationship Id="general" Type="hyperlink" Target="https://example.test/anchor" TargetMode="External"/></Relationships>"#;
    for merged in [true, false] {
        let sheet = if merged {
            sheet.to_owned()
        } else {
            sheet.replace("<mergeCells><mergeCell ref=\"A1:B2\"/></mergeCells>", "")
        };
        let bytes = zip(&[
            ("xl/workbook.xml", workbook),
            ("xl/_rels/workbook.xml.rels", rels),
            ("xl/a.xml", &sheet),
            ("xl/_rels/a.xml.rels", links),
        ]);
        same(&bytes, "decorated.xlsx", Options::default());
        let md = Parser::default()
            .parse_bytes(&bytes, Some("decorated.xlsx"))
            .unwrap()
            .to_markdown()
            .text;
        assert!(
            md.contains("https://example.test/covered")
                && md.contains("https://example.test/anchor")
        );
        assert!(!md.contains("SECRET"));
        for kind in ["nodes", "cells", "text", "nodes_and_links"] {
            for limit in 0..180 {
                for recovery in [false, true] {
                    let mut options = Options {
                        partial_on_limit: recovery,
                        ..Default::default()
                    };
                    match kind {
                        "nodes" => options.limits.max_nodes = limit,
                        "cells" => options.limits.max_table_cells = limit,
                        "text" => options.limits.max_text_bytes = limit,
                        _ => {
                            options.limits.max_nodes = limit;
                            options.limits.max_text_bytes = 90;
                        }
                    }
                    let parser = Parser::new(options);
                    let normal = parser.parse_bytes(&bytes, Some("decorated.xlsm"));
                    let mut output = Vec::new();
                    let streamed =
                        parser.parse_to_markdown(&bytes, Some("decorated.xlsm"), &mut output);
                    match (normal, streamed) {
                        (Err(a), Err(b)) => {
                            assert_eq!(
                                a.to_string(),
                                b.to_string(),
                                "{merged} {kind} {limit} {recovery}"
                            );
                            assert!(output.is_empty());
                        }
                        (Ok(a), Ok(b)) => {
                            let rendered = a.to_markdown();
                            assert_eq!(output, rendered.text.as_bytes());
                            assert_eq!(a.status, b.status);
                            assert_eq!(
                                serde_json::to_value(a.diagnostics).unwrap(),
                                serde_json::to_value(b.diagnostics).unwrap()
                            );
                            assert_eq!(
                                serde_json::to_value(rendered.diagnostics).unwrap(),
                                serde_json::to_value(b.render_diagnostics).unwrap()
                            );
                        }
                        _ => panic!("different outcome at {merged} {kind} {limit} {recovery}"),
                    }
                }
            }
        }
    }
}

#[cfg(feature = "office")]
#[test]
fn ods_repeated_rows_multiple_headers_and_nested_cells_match() {
    let content = r#"<office:document-content xmlns:office="urn:o" xmlns:table="urn:t" xmlns:text="urn:x"><office:body><office:spreadsheet><table:table table:name="First"><table:table-header-rows><table:table-row table:number-rows-repeated="2"><table:table-cell><text:p>Header</text:p></table:table-cell></table:table-row></table:table-header-rows><table:table-row table:number-rows-repeated="3"><table:table-cell><text:p>A</text:p><text:p>B</text:p></table:table-cell></table:table-row></table:table><table:table table:name="Second"><table:table-row><table:table-cell table:number-columns-spanned="2"><text:p>tail</text:p></table:table-cell><table:covered-table-cell/></table:table-row></table:table></office:spreadsheet></office:body></office:document-content>"#;
    let bytes = zip(&[
        ("mimetype", "application/vnd.oasis.opendocument.spreadsheet"),
        ("content.xml", content),
    ]);
    same(&bytes, "two.ods", Options::default());
}

#[cfg(feature = "office")]
#[test]
fn streamed_merged_table_preserves_budget_errors_and_atomic_output() {
    let content = r#"<office:document-content xmlns:office="urn:o" xmlns:table="urn:t" xmlns:text="urn:x"><office:body><office:spreadsheet><table:table table:name="Grid"><table:table-header-rows><table:table-row table:number-rows-repeated="2"><table:table-cell><text:p>Header</text:p></table:table-cell></table:table-row></table:table-header-rows><table:table-row><table:table-cell table:number-columns-spanned="2"><text:p>Anchor &amp; |</text:p></table:table-cell><table:covered-table-cell><text:p>Covered</text:p></table:covered-table-cell></table:table-row><table:table-row><table:table-cell><text:list><text:list-item><text:p>Nested</text:p></text:list-item></text:list></table:table-cell></table:table-row></table:table></office:spreadsheet></office:body></office:document-content>"#;
    let bytes = zip(&[
        ("mimetype", "application/vnd.oasis.opendocument.spreadsheet"),
        ("content.xml", content),
    ]);
    same(&bytes, "grid.ods", Options::default());
    for kind in ["nodes", "cells", "text"] {
        for limit in 0..90 {
            let mut options = Options::default();
            match kind {
                "nodes" => options.limits.max_nodes = limit,
                "cells" => options.limits.max_table_cells = limit,
                _ => options.limits.max_text_bytes = limit,
            }
            let parser = Parser::new(options.clone());
            let normal = parser.parse_bytes(&bytes, Some("grid.ods"));
            let mut output = Vec::new();
            let streamed = parser.parse_to_markdown(&bytes, Some("grid.ods"), &mut output);
            match (normal, streamed) {
                (Err(a), Err(b)) => {
                    assert_eq!(a.to_string(), b.to_string(), "{kind} {limit}");
                    assert!(output.is_empty(), "{kind} {limit}");
                }
                (Ok(a), Ok(b)) => {
                    assert_eq!(output, a.to_markdown().text.as_bytes());
                    assert_eq!(
                        serde_json::to_value(a.diagnostics).unwrap(),
                        serde_json::to_value(b.diagnostics).unwrap()
                    );
                }
                _ => panic!("different outcome at {kind} {limit}"),
            }
        }
    }
}

#[cfg(feature = "office")]
#[test]
fn spreadsheet_preview_stream_matches_multiple_parts_and_exact_cutoffs() {
    let content = format!(
        r#"<office:document-content xmlns:office="urn:o" xmlns:table="urn:t" xmlns:text="urn:x"><office:body><office:spreadsheet><table:table table:name="First"><table:table-row><table:table-cell><text:p>A | B</text:p></table:table-cell></table:table-row></table:table><table:table table:name="Second">{}</table:table></office:spreadsheet></office:body></office:document-content>"#,
        "<table:table-row><table:table-cell><text:p>C &amp; D</text:p></table:table-cell></table:table-row>".repeat(150)
    );
    let bytes = zip(&[
        ("mimetype", "application/vnd.oasis.opendocument.spreadsheet"),
        ("content.xml", &content),
    ]);
    for nodes in [90, 150, 300] {
        let mut options = Options {
            partial_on_limit: true,
            ..Default::default()
        };
        options.limits.max_nodes = nodes;
        same(&bytes, "preview.ods", options);
    }
}

#[cfg(feature = "office")]
#[test]
fn incremental_xml_matches_the_small_dom_path_for_rich_shared_strings_and_ods() {
    fn compare(small: &[u8], large: &[u8], name: &str) {
        let parser = Parser::default();
        let a = parser.parse_bytes(small, Some(name)).unwrap();
        let b = parser.parse_bytes(large, Some(name)).unwrap();
        assert_eq!(
            serde_json::to_value(&a.document).unwrap(),
            serde_json::to_value(&b.document).unwrap()
        );
        assert_eq!(
            serde_json::to_value(&a.diagnostics).unwrap(),
            serde_json::to_value(&b.diagnostics).unwrap()
        );
        assert_eq!(a.to_markdown().text, b.to_markdown().text);
        same(large, name, Options::default());
    }
    let workbook =
        r#"<workbook xmlns:r="urn:r"><sheets><sheet name="First" r:id="a"/></sheets></workbook>"#;
    let rels =
        r#"<Relationships><Relationship Id="a" Type="worksheet" Target="a.xml"/></Relationships>"#;
    let sheet = r#"<worksheet><cols><col min="3" max="3" hidden="1"/></cols><sheetData><row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c><c r="C1" t="s"><v>0</v></c></row><row r="2"><c r="B2" t="s"><v>0</v></c></row></sheetData><mergeCells><mergeCell ref="A2:B2"/></mergeCells><hyperlinks><hyperlink ref="A1" location="target"/></hyperlinks></worksheet>"#;
    let strings = r#"<sst xmlns="urn:s"><si><r><rPr><b/><i val="0"/></rPr><t> A&amp;B </t></r><rPh><t>phonetic</t></rPh><r><rPr><i/></rPr><t>東京</t></r></si><si><t>second</t></si></sst>"#;
    let large_sheet = sheet.replace(
        "<sheetData>",
        &format!("<sheetData>{}", " ".repeat(270_000)),
    );
    let large_strings = strings.replace("</sst>", &format!("{}</sst>", " ".repeat(270_000)));
    let small = zip(&[
        ("xl/workbook.xml", workbook),
        ("xl/_rels/workbook.xml.rels", rels),
        ("xl/a.xml", sheet),
        ("xl/sharedStrings.xml", strings),
    ]);
    let large = zip(&[
        ("xl/workbook.xml", workbook),
        ("xl/_rels/workbook.xml.rels", rels),
        ("xl/a.xml", &large_sheet),
        ("xl/sharedStrings.xml", &large_strings),
    ]);
    compare(&small, &large, "test.xlsx");
    let ods = r#"<office:document-content xmlns:office="urn:o" xmlns:table="urn:t" xmlns:text="urn:x" xmlns:style="urn:s" xmlns:fo="urn:f"><office:automatic-styles><style:style style:name="B" style:family="text"><style:text-properties fo:font-weight="bold"/></style:style></office:automatic-styles><office:body><office:spreadsheet><table:table table:name="Sheet"><table:table-header-rows><table:table-row table:number-rows-repeated="2"><table:table-cell><text:p>Header</text:p></table:table-cell></table:table-row></table:table-header-rows><table:table-row table:number-rows-repeated="3"><table:table-cell table:number-columns-repeated="2"><text:p><text:span text:style-name="B">A&amp;B</text:span></text:p></table:table-cell></table:table-row></table:table><table:table table:name="Second"><table:table-row><table:table-cell><text:p>tail</text:p></table:table-cell></table:table-row></table:table></office:spreadsheet></office:body></office:document-content>"#;
    let large_ods = ods.replace(
        "<office:body>",
        &format!("<office:body>{}", " ".repeat(270_000)),
    );
    compare(
        &zip(&[
            ("mimetype", "application/vnd.oasis.opendocument.spreadsheet"),
            ("content.xml", ods),
        ]),
        &zip(&[
            ("mimetype", "application/vnd.oasis.opendocument.spreadsheet"),
            ("content.xml", &large_ods),
        ]),
        "test.ods",
    );
}

#[test]
fn plain_table_fast_path_matches_general_inline_rendering() {
    let values = [
        "",
        "  space  ",
        "\\| | \\\\|",
        "a\nb\r\nc",
        "東京 😀\u{a0}",
        "\\`*_{}[]<>#!|~+-=.",
    ];
    for value in values {
        let csv = format!("\"{}\"\n", value.replace('"', "\"\""));
        let mut result = Parser::default()
            .parse_bytes(csv.as_bytes(), Some("cell.csv"))
            .unwrap();
        let fast = result.to_markdown();
        for block in &mut result.document.blocks {
            if let bulkparse::BlockContent::Table { rows, .. } = &mut block.content {
                for cell in rows.iter_mut().flatten() {
                    for block in &mut cell.blocks {
                        if let bulkparse::BlockContent::Paragraph { inlines } = &mut block.content {
                            inlines.push(bulkparse::Inline::Text {
                                text: String::new(),
                            });
                        }
                    }
                }
            }
        }
        let general = result.to_markdown();
        assert_eq!(fast.text, general.text, "{value:?}");
        assert_eq!(
            serde_json::to_value(fast.diagnostics).unwrap(),
            serde_json::to_value(general.diagnostics).unwrap()
        );
    }
}

#[test]
fn csv_budget_probe_matches_streaming_limit_order() {
    for data in [
        b"a,b\ncc,dd\n".as_slice(),
        b"\"line\nbreak\",x\ny,z\n",
        b"a,b\nc,d,e\n",
    ] {
        for cap in 0..14 {
            for budget in ["nodes", "cells", "text"] {
                let mut options = Options::default();
                match budget {
                    "nodes" => options.limits.max_nodes = cap,
                    "cells" => options.limits.max_table_cells = cap,
                    _ => options.limits.max_text_bytes = cap,
                }
                let parser = Parser::new(options);
                let structured = parser.parse_bytes(data, Some("budget.csv"));
                let mut output = vec![];
                let streaming = parser.parse_to_markdown(data, Some("budget.csv"), &mut output);
                match (structured, streaming) {
                    (Ok(result), Ok(_)) => assert_eq!(result.to_markdown().text.as_bytes(), output),
                    (Err(a), Err(b)) => {
                        assert_eq!(a.to_string(), b.to_string(), "{budget}={cap}");
                        assert!(output.is_empty());
                    }
                    other => panic!("budget paths disagree at {budget}={cap}: {other:?}"),
                }
            }
        }
    }
}
