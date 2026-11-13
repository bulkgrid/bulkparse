use bulkparse::{BlockContent, Error, Options, Parser, TextEncoding};

fn table_width(bytes: &[u8], options: Options) -> usize {
    let result = Parser::new(options)
        .parse_bytes(bytes, Some("data.csv"))
        .unwrap();
    let BlockContent::Table { rows, .. } = &result.document.blocks[0].content else {
        panic!("expected table")
    };
    assert!(rows.iter().all(|r| r.len() == rows[0].len()));
    rows[0].len()
}

#[test]
fn csv_dialects_preserve_columns_and_quoted_delimiters() {
    for delimiter in [',', ';', '\t', '|'] {
        let row = (0..12)
            .map(|i| format!("field{i}"))
            .collect::<Vec<_>>()
            .join(&delimiter.to_string());
        assert_eq!(
            table_width(format!("{row}\n{row}\n").as_bytes(), Options::default()),
            12
        );
    }
    assert_eq!(
        table_width(b"name;value\nalpha;\"a,b\nnext\"\n", Options::default()),
        2
    );
    assert_eq!(table_width(b"one\ntwo\n", Options::default()), 1);
}

#[test]
fn csv_ambiguity_requires_override_and_sampling_is_bounded() {
    let ambiguous = b"a,b;c\nx,y;z\n";
    assert!(matches!(
        Parser::default().parse_bytes(ambiguous, Some("x.csv")),
        Err(Error::Malformed(_))
    ));
    assert_eq!(
        table_width(
            ambiguous,
            Options {
                csv_delimiter: Some(';'),
                ..Default::default()
            }
        ),
        2
    );
    let huge = format!("\"{}\";tail", "x".repeat(70_000));
    assert!(matches!(
        Parser::default().parse_bytes(huge.as_bytes(), Some("x.csv")),
        Err(Error::Malformed(_))
    ));
    assert_eq!(
        table_width(
            huge.as_bytes(),
            Options {
                csv_delimiter: Some(';'),
                ..Default::default()
            }
        ),
        2
    );
    for delimiter in ['"', '\n', '\0', 'é'] {
        assert!(Parser::new(Options {
            csv_delimiter: Some(delimiter),
            ..Default::default()
        })
        .parse_bytes(b"a,b", Some("x.csv"))
        .is_err());
    }
}

#[test]
fn csv_unicode_encodings_are_lossless_and_strict() {
    let text = "name;value\n名称;😀\n";
    for (encoding, bom) in [
        (TextEncoding::Utf16Le, [0xff, 0xfe]),
        (TextEncoding::Utf16Be, [0xfe, 0xff]),
    ] {
        let body: Vec<u8> = text
            .encode_utf16()
            .flat_map(|u| {
                if encoding == TextEncoding::Utf16Le {
                    u.to_le_bytes()
                } else {
                    u.to_be_bytes()
                }
            })
            .collect();
        let mut bytes = bom.to_vec();
        bytes.extend(&body);
        assert_eq!(table_width(&bytes, Options::default()), 2);
        assert_eq!(
            table_width(
                &body,
                Options {
                    csv_encoding: Some(encoding),
                    ..Default::default()
                }
            ),
            2
        );
        let result = Parser::default()
            .parse_bytes(&bytes, Some("x.csv"))
            .unwrap();
        assert!(result.to_markdown().text.contains("😀"));
        assert!(Parser::new(Options {
            csv_encoding: Some(TextEncoding::Utf8),
            ..Default::default()
        })
        .parse_bytes(&bytes, Some("x.csv"))
        .is_err());
    }
    for bytes in [&[0xff, 0xfe, 0][..], &[0xff, 0xfe, 0, 0xd8][..]] {
        assert!(Parser::default().parse_bytes(bytes, Some("x.csv")).is_err());
    }
    let mut limited = Options::default();
    limited.limits.max_text_bytes = 1;
    assert!(matches!(
        Parser::new(limited).parse_bytes(&[0xff, 0xfe, 0x2d, 0x4e], Some("x.csv")),
        Err(Error::Limit("max_text_bytes"))
    ));
    let options: Options =
        serde_json::from_str(r#"{"csv_delimiter":";","csv_encoding":"utf16_le"}"#).unwrap();
    assert_eq!(options.csv_encoding, Some(TextEncoding::Utf16Le));
}

#[test]
fn csv_legacy_exports_preserve_currency_and_punctuation_with_diagnostics() {
    let bytes = b"name,amount\n\x93caf\xe9\x94,\xa310\n\x80,\x96\n";
    let r = Parser::default()
        .parse_bytes(bytes, Some("export.csv"))
        .unwrap();
    let md = r.to_markdown().text;
    assert!(
        md.contains("“café”") && md.contains("£10") && md.contains('€') && md.contains('–'),
        "{md}"
    );
    assert!(r
        .diagnostics
        .iter()
        .any(|d| d.code == "csv_encoding_inferred"));
    let options: Options = serde_json::from_str(r#"{"csv_encoding":"windows1252"}"#).unwrap();
    let explicit = Parser::new(options)
        .parse_bytes(bytes, Some("export.tsv"))
        .unwrap();
    assert!(!explicit
        .diagnostics
        .iter()
        .any(|d| d.code == "csv_encoding_inferred"));
    assert!(Parser::new(Options {
        csv_encoding: Some(TextEncoding::Utf8),
        ..Default::default()
    })
    .parse_bytes(bytes, Some("export.csv"))
    .is_err());
    let utf8 = Parser::default()
        .parse_bytes("name,value\n名称,é€😀\n".as_bytes(), Some("a.csv"))
        .unwrap();
    assert!(utf8.to_markdown().text.contains("é€😀"));
    assert!(!utf8
        .diagnostics
        .iter()
        .any(|d| d.code == "csv_encoding_inferred"));
}

#[test]
fn csv_encoding_fallback_respects_bom_invalid_bytes_and_limits() {
    for b in [
        b"a,b\nx,\x81\n".as_slice(),
        b"a,b\nx,\xa3\0\n",
        b"\xef\xbb\xbfa,b\nx,\xa3\n",
    ] {
        assert!(Parser::default().parse_bytes(b, Some("bad.csv")).is_err());
    }
    let mut options = Options::default();
    options.limits.max_text_bytes = 6;
    assert!(matches!(
        Parser::new(options).parse_bytes(b"\x80,\x80\n", Some("limited.csv")),
        Err(Error::Limit("max_text_bytes"))
    ));
    let conflict = Options {
        csv_encoding: Some(TextEncoding::Windows1252),
        ..Default::default()
    };
    assert!(Parser::new(conflict)
        .parse_bytes(b"\xef\xbb\xbfa,b", Some("bom.csv"))
        .is_err());
}

#[cfg(feature = "docx")]
mod docx {
    use super::*;
    use bulkparse::Status;
    use std::io::{Cursor, Write};
    const NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
    fn package(body: &str, styles: &str, numbering: &str) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (path, xml) in [
            (
                "word/document.xml",
                format!("<w:document xmlns:w=\"{NS}\"><w:body>{body}</w:body></w:document>"),
            ),
            (
                "word/styles.xml",
                format!("<w:styles xmlns:w=\"{NS}\">{styles}</w:styles>"),
            ),
            (
                "word/numbering.xml",
                format!("<w:numbering xmlns:w=\"{NS}\">{numbering}</w:numbering>"),
            ),
        ] {
            zip.start_file(path, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(xml.as_bytes()).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }
    fn p(text: &str) -> String {
        format!("<w:p><w:r><w:t>{text}</w:t></w:r></w:p>")
    }
    fn sdt(text: &str) -> String {
        format!("<w:sdt><w:sdtPr/><w:sdtContent>{text}</w:sdtContent></w:sdt>")
    }
    fn parse(bytes: &[u8]) -> bulkparse::ParseResult {
        Parser::default()
            .parse_bytes(bytes, Some("x.docx"))
            .unwrap()
    }
    fn strict(bytes: &[u8]) -> Result<bulkparse::ParseResult, Error> {
        Parser::new(Options {
            strict: true,
            ..Default::default()
        })
        .parse_bytes(bytes, Some("x.docx"))
    }
    #[test]
    fn content_controls_keep_body_rows_cells_and_nested_tables() {
        let nested = format!(
            "<w:tbl><w:tr><w:tc>{}</w:tc></w:tr></w:tbl>",
            p("Nested value")
        );
        let row = format!(
            "<w:tr><w:tc>{}</w:tc>{}</w:tr>",
            p("Left"),
            sdt(&format!("<w:tc>{}{nested}</w:tc>", p("Right")))
        );
        let body = format!("{}<w:tbl>{}</w:tbl>", sdt(&p("Cover value")), sdt(&row));
        let result = strict(&package(&body, "", "")).unwrap();
        assert_eq!(result.status, Status::Complete);
        let json = result.to_json().unwrap();
        for value in ["Cover value", "Left", "Right", "Nested value"] {
            assert_eq!(json.matches(value).count(), 1);
        }
        let BlockContent::Table { rows, .. } = &result.document.blocks[1].content else {
            panic!("table")
        };
        assert_eq!(rows[0].len(), 2);
        assert!(matches!(
            rows[0][1].blocks[1].content,
            BlockContent::Table { .. }
        ));
    }
    #[test]
    fn unsupported_table_wrappers_cannot_claim_complete() {
        let body = format!(
            "<w:tbl><w:unknown><w:tr><w:tc>{}</w:tc></w:tr></w:unknown></w:tbl>",
            p("Unknown")
        );
        let bytes = package(&body, "", "");
        assert_eq!(parse(&bytes).status, Status::Partial);
        assert!(matches!(strict(&bytes), Err(Error::Incomplete(_))));
    }
    const STYLES: &str = r#"<w:style w:type="paragraph" w:styleId="Base"><w:pPr><w:numPr><w:numId w:val="7"/></w:numPr></w:pPr></w:style><w:style w:type="paragraph" w:styleId="Child"><w:basedOn w:val="Base"/></w:style>"#;
    const NUMBERING: &str = r#"<w:abstractNum w:abstractNumId="1"><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/></w:lvl><w:lvl w:ilvl="1"><w:start w:val="1"/><w:numFmt w:val="bullet"/></w:lvl></w:abstractNum><w:num w:numId="7"><w:abstractNumId w:val="1"/><w:lvlOverride w:ilvl="0"><w:startOverride w:val="4"/></w:lvlOverride></w:num>"#;
    fn item(text: &str, extra: &str) -> String {
        format!("<w:p><w:pPr><w:pStyle w:val=\"Child\"/>{extra}</w:pPr><w:r><w:t>{text}</w:t></w:r></w:p>")
    }
    #[test]
    fn inherited_lists_continue_after_tables_and_honor_overrides() {
        let body = format!(
            "{}{}<w:tbl><w:tr><w:tc>{}</w:tc></w:tr></w:tbl>{}{}",
            item("First", ""),
            item("Nested", r#"<w:numPr><w:ilvl w:val="1"/></w:numPr>"#),
            p("Cell"),
            item("Second", ""),
            item("Plain", r#"<w:numPr><w:numId w:val="0"/></w:numPr>"#)
        );
        let result = strict(&package(&body, STYLES, NUMBERING)).unwrap();
        let BlockContent::List { start, items } = &result.document.blocks[0].content else {
            panic!("list")
        };
        assert_eq!(*start, Some(4));
        assert!(matches!(
            items[0][1].content,
            BlockContent::List { start: None, .. }
        ));
        assert!(matches!(
            result.document.blocks[2].content,
            BlockContent::List { start: Some(5), .. }
        ));
        assert!(matches!(
            result.document.blocks[3].content,
            BlockContent::Paragraph { .. }
        ));
        assert!(result.to_markdown().text.contains("5. Second"));
    }
    #[test]
    fn missing_or_custom_numbering_reports_partial_and_cycles_fail() {
        for numbering in [String::new(), NUMBERING.replace("decimal", "lowerRoman")] {
            let bytes = package(&item("Retained", ""), STYLES, &numbering);
            let result = parse(&bytes);
            assert_eq!(result.status, Status::Partial);
            assert!(result.to_markdown().text.contains("Retained"));
            assert!(matches!(strict(&bytes), Err(Error::Incomplete(_))));
        }
        let cycle = r#"<w:style w:styleId="Child"><w:basedOn w:val="Child"/></w:style>"#;
        assert!(matches!(
            strict(&package(&item("Cycle", ""), cycle, "")),
            Err(Error::Malformed(_))
        ));
    }
    #[test]
    fn styles_preserve_code_quotes_and_run_overrides() {
        let styles = r#"<w:style w:styleId="Code"><w:name w:val="Source Code"/></w:style><w:style w:styleId="Quote"><w:name w:val="Quotation"/><w:rPr><w:i/></w:rPr></w:style><w:style w:styleId="Base"><w:rPr><w:b/></w:rPr></w:style><w:style w:styleId="Child"><w:basedOn w:val="Base"/></w:style>"#;
        let body = r#"<w:p><w:pPr><w:pStyle w:val="Code"/></w:pPr><w:r><w:t>x = 1;</w:t></w:r></w:p><w:p><w:pPr><w:pStyle w:val="Quote"/></w:pPr><w:r><w:t>Quoted</w:t></w:r></w:p><w:p><w:pPr><w:pStyle w:val="Child"/></w:pPr><w:r><w:t>Bold</w:t></w:r><w:r><w:rPr><w:b w:val="0"/></w:rPr><w:t>Plain</w:t></w:r></w:p>"#;
        let result = strict(&package(body, styles, "")).unwrap();
        assert!(matches!(
            result.document.blocks[0].content,
            BlockContent::Code { .. }
        ));
        assert!(matches!(
            result.document.blocks[1].content,
            BlockContent::Quote { .. }
        ));
        let markdown = result.to_markdown().text;
        assert!(markdown.contains("> *Quoted*"));
        assert!(markdown.contains("**Bold**Plain"));
    }
    #[test]
    fn corrupt_optional_styles_recover_body_but_strict_rejects() {
        let bytes = package(&p("Keep this"), "<broken>", "");
        let result = parse(&bytes);
        assert_eq!(result.status, Status::Partial);
        assert!(result.to_markdown().text.contains("Keep this"));
        assert!(matches!(strict(&bytes), Err(Error::Incomplete(_))));
    }
}

#[cfg(feature = "pdf")]
#[test]
fn pdf_spacing_handles_real_adjustments_and_adjacent_arrays() {
    use lopdf::{dictionary, Object, Stream};
    let mut doc = lopdf::Document::with_version("1.5");
    let pages = doc.new_object_id();
    let font = doc.add_object(
        dictionary! {"Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica"},
    );
    let stream = b"BT /F1 12 Tf 1 0 0 1 40 300 Tm 14 TL [(Hello) -250.5 (world)] TJ [(con)] TJ [(tinued)] TJ T* (Next) Tj (Quoted) ' 0 0 (Double) \" ET";
    let contents = doc.add_object(Stream::new(dictionary! {}, stream.to_vec()));
    let page = doc.add_object(dictionary! {"Type" => "Page", "Parent" => pages, "MediaBox" => vec![0.into(),0.into(),400.into(),400.into()], "Resources" => dictionary! {"Font" => dictionary! {"F1" => font}}, "Contents" => contents});
    doc.objects.insert(
        pages,
        Object::Dictionary(
            dictionary! {"Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1},
        ),
    );
    let catalog = doc.add_object(dictionary! {"Type" => "Catalog", "Pages" => pages});
    doc.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let result = Parser::default()
        .parse_bytes(&bytes, Some("x.pdf"))
        .unwrap();
    let markdown = result.to_markdown().text;
    assert!(markdown.contains("Hello worldcontinued"));
    assert!(markdown.contains("Next\nQuoted\nDouble"));
    assert_eq!(result.status, bulkparse::Status::Partial);
    let mut options = Options::default();
    options.limits.max_nodes = 2;
    assert!(matches!(
        Parser::new(options).parse_bytes(&bytes, Some("x.pdf")),
        Err(Error::Limit("max_nodes"))
    ));
}

#[test]
fn csv_bounded_recovery_keeps_complete_records_and_strict_mode_errors() {
    let data = b"name,value\nalpha,one\nbeta,two\ngamma,three\n";
    let mut o = Options::default();
    o.limits.max_table_cells = 5;
    assert!(matches!(
        Parser::new(o.clone()).parse_bytes(data, Some("rows.csv")),
        Err(Error::Limit(_))
    ));
    o.partial_on_limit = true;
    let r = Parser::new(o.clone())
        .parse_bytes(data, Some("rows.csv"))
        .unwrap();
    assert_eq!(r.status, bulkparse::Status::Partial);
    let BlockContent::Table { rows, .. } = &r.document.blocks[0].content else {
        panic!()
    };
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|r| r.len() == 2));
    assert!(r.to_markdown().text.contains("alpha") && !r.to_markdown().text.contains("beta"));
    assert!(r
        .diagnostics
        .iter()
        .any(|d| d.code == "csv_rows_truncated" && d.content_loss));
    o.strict = true;
    assert!(matches!(
        Parser::new(o).parse_bytes(data, Some("rows.csv")),
        Err(Error::Limit(_))
    ));
    let options: Options = serde_json::from_str(r#"{"partial_on_limit":true}"#).unwrap();
    assert!(options.partial_on_limit);
}
