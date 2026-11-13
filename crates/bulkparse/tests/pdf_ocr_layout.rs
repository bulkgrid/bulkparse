#![cfg(feature = "pdf")]
use bulkparse::*;

struct Positioned(PdfOcrOutput);
impl PdfOcrProvider for Positioned {
    fn recognize(&self, _: PdfOcrRequest<'_>) -> Result<String, Error> {
        panic!("structured provider must be used")
    }
    fn recognize_page(&self, _: PdfOcrRequest<'_>) -> Result<PdfOcrOutput, Error> {
        Ok(self.0.clone())
    }
}
fn line(text: &str, x: f64, y: f64) -> PdfOcrLine {
    PdfOcrLine {
        text: text.into(),
        quad: [[x, y], [x + 100., y], [x + 100., y + 12.], [x, y + 12.]],
        confidence: 0.875,
    }
}
fn output(lines: Vec<PdfOcrLine>) -> PdfOcrOutput {
    PdfOcrOutput::Lines {
        image_size: [600, 800],
        lines,
    }
}
fn parse(provider: &Positioned, budget: PdfOcrBudget) -> Result<ParseResult, Error> {
    Parser::default().parse_with_pdf_ocr(
        include_bytes!("../../../fixtures/holdout/scan-only.pdf"),
        Some("unrelated-name.pdf"),
        &CancellationToken::default(),
        provider,
        budget,
    )
}
#[test]
fn geometry_survives_ordering_json_and_markdown() {
    let a = line("Left one", 20., 40.);
    let provider = Positioned(output(vec![
        a.clone(),
        line("Right one", 350., 40.),
        line("Left two", 20., 80.),
        line("Right two", 350., 80.),
    ]));
    let result = parse(&provider, PdfOcrBudget::default()).unwrap();
    assert_eq!(result.document.schema_version, "0.8");
    assert_eq!(result.status, Status::Partial);
    assert_eq!(result.document.blocks.len(), 4);
    let evidence = result.document.blocks[0].source.ocr.as_ref().unwrap();
    assert_eq!(evidence.quad, a.quad);
    assert_eq!(evidence.confidence, 0.875);
    assert_eq!(evidence.image_size, [600, 800]);
    assert!(result
        .document
        .blocks
        .iter()
        .all(|b| b.source.page == Some(1)));
    let roundtrip: ParseResult = serde_json::from_str(&result.to_json().unwrap()).unwrap();
    assert_eq!(roundtrip.document, result.document);
    let md = result.to_markdown().text;
    assert!(md.find("Left two").unwrap() < md.find("Right one").unwrap());
    assert_eq!(md.matches("Left one").count(), 1);
    assert!(result
        .diagnostics
        .iter()
        .any(|d| d.code == "pdf_ocr_layout_inferred"));
    let schema = json_schema().to_string();
    assert!(schema.contains("image_size") && schema.contains("confidence"));
}
#[test]
fn malformed_evidence_is_rejected_without_discarding_native_pages() {
    let mut invalid = vec![];
    for value in [f64::NAN, f64::INFINITY, -1., 601.] {
        let mut l = line("invalid", 10., 20.);
        l.quad[0][0] = value;
        invalid.push(l);
    }
    for value in [f64::NAN, -0.1, 1.1] {
        let mut l = line("invalid", 10., 20.);
        l.confidence = value;
        invalid.push(l);
    }
    let mut crossed = line("invalid", 10., 20.);
    crossed.quad.swap(1, 2);
    invalid.push(crossed);
    let mut flat = line("invalid", 10., 20.);
    flat.quad[2] = flat.quad[1];
    invalid.push(flat);
    invalid.push(line("multiple\nlines", 10., 20.));
    invalid.push(line(" ", 10., 20.));
    for l in invalid {
        let result = Parser::default()
            .parse_with_pdf_ocr(
                include_bytes!("../../../fixtures/holdout/native-and-scan.pdf"),
                Some("x.pdf"),
                &CancellationToken::default(),
                &Positioned(output(vec![l])),
                PdfOcrBudget::default(),
            )
            .unwrap();
        assert!(!result.document.blocks.is_empty());
        assert!(result
            .document
            .blocks
            .iter()
            .all(|b| b.source.page == Some(1) && b.source.ocr.is_none()));
        assert!(result
            .diagnostics
            .iter()
            .any(|d| d.code == "pdf_ocr_error" && d.source.page == Some(2)));
        assert!(!result
            .diagnostics
            .iter()
            .any(|d| d.code == "pdf_ocr_applied"));
    }
}
#[test]
fn structured_text_geometry_and_node_budgets_are_enforced() {
    let provider = Positioned(output(vec![line("a", 10., 20.), line("b", 10., 60.)]));
    assert!(matches!(
        parse(
            &provider,
            PdfOcrBudget {
                max_pages: 1,
                max_output_bytes: 2
            }
        ),
        Err(Error::Limit("pdf_ocr_output_bytes"))
    ));
    assert!(parse(
        &provider,
        PdfOcrBudget {
            max_pages: 1,
            max_output_bytes: 3
        }
    )
    .is_ok());
    let mut options = Options::default();
    options.limits.max_nodes = 1;
    assert!(matches!(
        Parser::new(options).parse_with_pdf_ocr(
            include_bytes!("../../../fixtures/holdout/scan-only.pdf"),
            Some("x.pdf"),
            &CancellationToken::default(),
            &provider,
            PdfOcrBudget::default()
        ),
        Err(Error::Limit("max_nodes"))
    ));
    assert!(matches!(
        output(vec![line("a", 10., 20.); 4097]).validate(100_000),
        Err(Error::Limit("pdf_ocr_lines"))
    ));
    assert!(PdfOcrOutput::Lines {
        image_size: [0, 800],
        lines: vec![]
    }
    .validate(100)
    .is_err());
}
