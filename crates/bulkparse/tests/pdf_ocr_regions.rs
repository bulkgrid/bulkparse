#![cfg(feature = "pdf")]
use bulkparse::*;

struct Provider(Vec<PdfOcrRegion>);
impl PdfOcrProvider for Provider {
    fn recognize(&self, _: PdfOcrRequest<'_>) -> Result<String, Error> {
        panic!("use regions")
    }
    fn recognize_page(&self, _: PdfOcrRequest<'_>) -> Result<PdfOcrOutput, Error> {
        Ok(output(self.0.clone()))
    }
}
fn output(regions: Vec<PdfOcrRegion>) -> PdfOcrOutput {
    PdfOcrOutput::Regions {
        image_size: [600, 800],
        regions,
    }
}
fn region(order: u32, content: PdfOcrRegionContent) -> PdfOcrRegion {
    PdfOcrRegion {
        order,
        label: "test_region".into(),
        bbox: [10., 20., 590., 100.],
        confidence: None,
        content,
    }
}
fn cell(text: &str, col_span: u32, row_span: u32) -> PdfOcrCell {
    PdfOcrCell {
        text: text.into(),
        col_span,
        row_span,
    }
}
fn regions() -> Vec<PdfOcrRegion> {
    vec![
        region(
            2,
            PdfOcrRegionContent::Formula {
                tex: r"E=mc^2".into(),
            },
        ),
        region(
            0,
            PdfOcrRegionContent::Heading {
                level: 1,
                text: "Scanned report".into(),
            },
        ),
        region(
            1,
            PdfOcrRegionContent::Table {
                header_rows: 1,
                rows: vec![
                    vec![cell("Quarter", 1, 1), cell("Revenue", 2, 1)],
                    vec![cell("Q1", 1, 2), cell("10", 1, 1), cell("20", 1, 1)],
                    vec![cell("30", 1, 1), cell("40", 1, 1)],
                ],
            },
        ),
    ]
}
fn parse(regions: Vec<PdfOcrRegion>, options: Options) -> Result<ParseResult, Error> {
    Parser::new(options).parse_with_pdf_ocr(
        include_bytes!("../../../fixtures/holdout/native-and-scan.pdf"),
        Some("mixed.pdf"),
        &CancellationToken::default(),
        &Provider(regions),
        PdfOcrBudget::default(),
    )
}

#[test]
fn semantic_ocr_preserves_tables_math_order_and_unknown_confidence() {
    let result = parse(regions(), Options::default()).unwrap();
    assert_eq!(result.status, Status::Partial);
    let blocks: Vec<_> = result
        .document
        .blocks
        .iter()
        .filter(|b| b.source.page == Some(2))
        .collect();
    assert_eq!(blocks.len(), 3);
    assert!(matches!(
        blocks[0].content,
        BlockContent::Heading { level: 1, .. }
    ));
    assert!(blocks
        .iter()
        .all(|b| b.source.pdf_region.as_ref().unwrap().confidence.is_none()));
    let BlockContent::Table { rows, header_rows } = &blocks[1].content else {
        panic!("table missing")
    };
    assert_eq!(*header_rows, 1);
    assert_eq!(rows[0][1].col_span, 2);
    assert_eq!(rows[1][0].row_span, 2);
    assert_eq!(rows[2][0].source.cell, Some(2));
    assert_eq!(rows[2][0].source.row, Some(3));
    let md = result.to_markdown().text;
    assert!(
        md.contains(
            "| Quarter | Revenue |  |\n| --- | --- | --- |\n| Q1 | 10 | 20 |\n|  | 30 | 40 |"
        ),
        "{md}"
    );
    assert!(!md.contains("<table>"));
    assert!(md.contains("$$E=mc^2$$"));
    assert!(md.find("Scanned report").unwrap() < md.find("Quarter").unwrap());
    let roundtrip: ParseResult = serde_json::from_str(&result.to_json().unwrap()).unwrap();
    assert_eq!(roundtrip.document, result.document);
    assert!(result
        .diagnostics
        .iter()
        .any(|d| d.code == "pdf_ocr_structure_inferred"));
    assert!(!result
        .diagnostics
        .iter()
        .any(|d| d.code == "pdf_ocr_layout_inferred"));
    let native = Parser::default()
        .parse_bytes(
            include_bytes!("../../../fixtures/holdout/native-and-scan.pdf"),
            Some("mixed.pdf"),
        )
        .unwrap();
    assert_eq!(
        result
            .document
            .blocks
            .iter()
            .filter(|b| b.source.page == Some(1))
            .collect::<Vec<_>>(),
        native
            .document
            .blocks
            .iter()
            .filter(|b| b.source.page == Some(1))
            .collect::<Vec<_>>()
    );
}

#[test]
fn region_validation_rejects_bad_boxes_order_confidence_and_spans() {
    let mut bad = vec![];
    for v in [f64::NAN, f64::INFINITY, -1., 601.] {
        let mut r = regions();
        r[0].bbox[0] = v;
        bad.push(r);
    }
    let mut r = regions();
    r[0].order = 0;
    bad.push(r);
    let mut r = regions();
    r[0].confidence = Some(f64::NAN);
    bad.push(r);
    for rows in [
        vec![vec![cell("a", 0, 1)]],
        vec![vec![cell("a", 1, 0)]],
        vec![vec![cell("a", 1, 2)]],
        vec![vec![cell("a", 2, 1)], vec![cell("b", 1, 1)]],
        vec![
            vec![cell("a", 1, 1), cell("b", 1, 2)],
            vec![cell("c", 2, 1)],
        ],
    ] {
        bad.push(vec![region(
            0,
            PdfOcrRegionContent::Table {
                rows,
                header_rows: 0,
            },
        )]);
    }
    for r in bad {
        assert!(output(r.clone()).validate(10000).is_err());
        let parsed = parse(r, Options::default()).unwrap();
        assert!(parsed.diagnostics.iter().any(|d| d.code == "pdf_ocr_error"));
        assert!(parsed
            .document
            .blocks
            .iter()
            .all(|b| b.source.page == Some(1)));
    }
}

#[test]
fn semantic_limits_charge_text_expanded_cells_and_nodes() {
    assert!(matches!(
        output(regions()).validate(5),
        Err(Error::Limit("pdf_ocr_output_bytes"))
    ));
    assert!(matches!(
        output(vec![regions()[0].clone(); 1025]).validate(1_000_000),
        Err(Error::Limit("pdf_ocr_regions"))
    ));
    let mut options = Options::default();
    options.limits.max_table_cells = 8;
    assert!(matches!(
        parse(regions(), options),
        Err(Error::Limit("max_table_cells"))
    ));
    let mut options = Options::default();
    options.limits.max_nodes = 1;
    assert!(matches!(
        parse(regions(), options),
        Err(Error::Limit("max_nodes"))
    ));
}

#[test]
fn figures_are_reported_as_incomplete_and_strict_rejects_model_output() {
    let r = vec![region(
        0,
        PdfOcrRegionContent::Figure {
            description: "".into(),
        },
    )];
    let parsed = parse(r.clone(), Options::default()).unwrap();
    assert!(parsed
        .diagnostics
        .iter()
        .any(|d| d.code == "pdf_ocr_figure_unavailable" && d.content_loss));
    assert!(parse(
        r,
        Options {
            strict: true,
            ..Default::default()
        }
    )
    .is_err());
}

#[test]
fn native_documents_do_not_call_an_advanced_provider() {
    struct Never;
    impl PdfOcrProvider for Never {
        fn recognize(&self, _: PdfOcrRequest<'_>) -> Result<String, Error> {
            panic!("native page reached OCR")
        }
    }
    let bytes = include_bytes!("../../../fixtures/holdout/prose.pdf");
    let native = Parser::default()
        .parse_bytes(bytes, Some("prose.pdf"))
        .unwrap();
    let advanced = Parser::default()
        .parse_with_pdf_ocr(
            bytes,
            Some("prose.pdf"),
            &CancellationToken::default(),
            &Never,
            PdfOcrBudget::default(),
        )
        .unwrap();
    assert_eq!(native.to_json().unwrap(), advanced.to_json().unwrap());
}
