#![cfg(feature = "pdf")]
use bulkparse::*;
use lopdf::{dictionary, Document, Object, Stream};
use std::cell::RefCell;

fn fixture(contents: &[&str], cmap: Option<&str>, form: bool) -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    let mut font = dictionary! {"Type"=>"Font", "Subtype"=>"Type1", "BaseFont"=>"Courier"};
    if let Some(cmap) = cmap {
        let id = doc.add_object(Stream::new(dictionary! {}, cmap.as_bytes().to_vec()));
        font.set("ToUnicode", id);
    }
    let font = doc.add_object(font);
    let image = doc.add_object(Stream::new(dictionary!{"Type"=>"XObject", "Subtype"=>"Image", "Width"=>1, "Height"=>1, "ColorSpace"=>"DeviceGray", "BitsPerComponent"=>8}, vec![255]));
    let mut xobjects = dictionary! {"Im"=>image};
    if form {
        let resource = dictionary! {"XObject"=>dictionary!{"Im"=>image}};
        let f = doc.add_object(Stream::new(dictionary!{"Type"=>"XObject", "Subtype"=>"Form", "BBox"=>vec![0.into(),0.into(),600.into(),800.into()], "Resources"=>resource}, b"q 600 0 0 800 0 0 cm /Im Do Q".to_vec()));
        xobjects.set("Form", f);
    }
    let resources = dictionary! {"Font"=>dictionary!{"F1"=>font},"XObject"=>xobjects};
    let pages = doc.new_object_id();
    let mut kids = Vec::new();
    for content in contents {
        let id = doc.add_object(Stream::new(dictionary! {}, content.as_bytes().to_vec()));
        let page = doc.add_object(dictionary! {"Type"=>"Page", "Parent"=>pages, "Contents"=>id});
        kids.push(page.into());
    }
    // Inherited boxes and resources deliberately exercise page-tree lookup.
    doc.objects.insert(pages, Object::Dictionary(dictionary!{"Type"=>"Pages", "Count"=>kids.len() as i64,"Kids"=>kids, "Resources"=>resources,"MediaBox"=>vec![0.into(),0.into(),600.into(),800.into()]}));
    let catalog = doc.add_object(dictionary! {"Type"=>"Catalog","Pages"=>pages});
    doc.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    bytes
}
const HEADER: &str = "BT /F1 12 Tf 40 740 Td (Annual results) Tj ET";
const SCAN: &str = "q 600 0 0 800 0 0 cm /Im Do Q";
fn inspect(bytes: &[u8]) -> PdfInspection {
    Parser::default().inspect_pdf(bytes).unwrap()
}

#[test]
fn comments_and_pdf_whitespace_do_not_hide_painted_content() {
    for prefix in [
        "% producer note\n\n",
        "% note\r\n  \t",
        "\0\x0c% note\r\n\n",
    ] {
        let bytes = fixture(&[&format!("{prefix}{SCAN}")], None, false);
        let page = inspect(&bytes).pages.remove(0);
        assert_eq!(page.kind, PdfPageKind::Image);
        assert_eq!(page.route, PdfRoute::Ocr);
        assert_eq!(page.image_count, 1);
    }
    let bytes=fixture(&["% note\n\nBT /F1 12 Tf 40 740 Td (Discount 25% \\(verified\\)) Tj % inline note\n \n ( retained) Tj ET"],None,false);
    let parsed = Parser::default()
        .parse_bytes(&bytes, Some("test.pdf"))
        .unwrap();
    assert!(parsed
        .to_markdown()
        .text
        .contains("Discount 25% (verified) retained"));
    assert_eq!(
        parsed.pdf_inspection.unwrap().pages[0].route,
        PdfRoute::Native
    );
}

#[test]
fn undecoded_content_is_reviewed_not_silently_skipped() {
    for prefix in ["", HEADER] {
        let bytes = fixture(&[&format!("{prefix} ? {SCAN}")], None, false);
        let page = inspect(&bytes).pages.remove(0);
        assert_eq!(page.route, PdfRoute::Review);
        assert!(page
            .reasons
            .contains(&PdfInspectionReason::ExtractionFailed));
        if !prefix.is_empty() {
            assert!(Parser::default()
                .parse_bytes(&bytes, Some("test.pdf"))
                .unwrap()
                .to_markdown()
                .text
                .contains("Annual results"));
        }
    }
}

#[test]
fn inspection_and_conversion_agree_on_all_pages_without_ocr() {
    let mixed = format!("{HEADER} {SCAN}");
    let bytes = fixture(&[HEADER, SCAN, &mixed, "", "10 10 50 50 re f"], None, false);
    let report = inspect(&bytes);
    assert_eq!(report.kind, PdfPageKind::Mixed);
    assert_eq!(report.pages_needing_ocr, [2, 3, 5]);
    assert_eq!(
        report.pages.iter().map(|p| p.route).collect::<Vec<_>>(),
        [
            PdfRoute::Native,
            PdfRoute::Ocr,
            PdfRoute::Ocr,
            PdfRoute::Skip,
            PdfRoute::Ocr
        ]
    );
    let converted = Parser::default()
        .parse_bytes(&bytes, Some("file.pdf"))
        .unwrap();
    assert_eq!(converted.pdf_inspection.as_ref().unwrap(), &report);
    assert_eq!(report.pages[0].image_count, 0); // Unused image resources don't count.
    assert_eq!(report.pages[2].image_coverage, Some(1.));
    assert!(report.pages[2]
        .reasons
        .contains(&PdfInspectionReason::LargeImageWithSparseText));
    assert_eq!(report.pages[3].kind, PdfPageKind::Blank);
}

#[test]
fn tiled_images_are_combined_but_repeated_logos_do_not_inflate_coverage() {
    let tiled = format!("{HEADER} q 300 0 0 800 0 0 cm /Im Do Q q 300 0 0 800 300 0 cm /Im Do Q");
    let logo = format!("{HEADER} {}", "q 50 0 0 50 10 10 cm /Im Do Q ".repeat(200));
    let report = inspect(&fixture(&[&tiled, &logo], None, false));
    assert_eq!(report.pages[0].image_coverage, Some(1.));
    assert_eq!(report.pages[0].route, PdfRoute::Ocr);
    assert!(report.pages[1].image_coverage.unwrap() < 0.02);
    assert_eq!(report.pages[1].route, PdfRoute::Native);
}

#[test]
fn nested_form_images_use_transforms_and_bbox_clipping() {
    let full = format!("{HEADER} /Form Do");
    let tiny = format!("{HEADER} q 0.1 0 0 0.1 0 0 cm /Form Do Q");
    let report = inspect(&fixture(&[&full, &tiny], None, true));
    assert_eq!(report.pages_needing_ocr, [1]);
    assert_eq!(report.pages[0].image_coverage, Some(1.));
    assert!(report.pages[1].image_coverage.unwrap() < 0.02);
}

#[test]
fn clipping_is_not_claimed_as_blank_or_precise() {
    let report = inspect(&fixture(
        &[
            "0 0 100 100 re W n",
            &format!("{HEADER} 0 0 100 100 re W n {SCAN}"),
        ],
        None,
        false,
    ));
    assert_eq!(report.pages[0].route, PdfRoute::Review);
    assert_eq!(report.pages[1].image_coverage, None);
    assert!(report.pages[1]
        .reasons
        .contains(&PdfInspectionReason::UncertainGraphics));
}

#[test]
fn off_page_and_zero_area_images_are_not_scans() {
    let bytes = fixture(
        &[
            "q 600 0 0 800 1000 1000 cm /Im Do Q",
            "q 0 0 0 0 0 0 cm /Im Do Q",
        ],
        None,
        false,
    );
    for p in inspect(&bytes).pages {
        assert_eq!(p.route, PdfRoute::Skip);
        assert_eq!(p.image_count, 0);
    }
}

#[test]
fn usable_hidden_text_is_retained_and_render_mode_restores_after_q() {
    let prose = "This is the searchable text layer of a scanned report. ".repeat(4);
    let content = format!("{SCAN} q 3 Tr BT /F1 12 Tf ({prose}) Tj ET Q {HEADER}");
    let p = inspect(&fixture(&[&content], None, false)).pages.remove(0);
    assert_eq!(p.route, PdfRoute::Native);
    assert_eq!(p.invisible_text_operations, 1);
    assert_eq!(p.text_operations, 2);
    assert!(p.reasons.contains(&PdfInspectionReason::InvisibleTextLayer));
}

#[test]
fn bad_unicode_layer_and_missing_fonts_request_ocr_without_images() {
    let cmap = "1 begincodespacerange <00> <FF> endcodespacerange 3 beginbfchar <41> <E001> <42> <E000> <43> <0001> endbfchar";
    let bytes = fixture(
        &[
            "BT /F1 12 Tf (ABCABC) Tj ET",
            "BT /Missing 12 Tf (Unmapped) Tj ET",
        ],
        Some(cmap),
        false,
    );
    let report = inspect(&bytes);
    assert_eq!(report.pages_needing_ocr, [1, 2]);
    assert_eq!(report.pages[0].suspicious_characters, 6);
    assert!(report.pages[1]
        .reasons
        .contains(&PdfInspectionReason::UndecodableText));
}

#[test]
fn sparse_multilingual_text_numbers_and_punctuation_do_not_require_ocr() {
    let cmap = "1 begincodespacerange <00> <FF> endcodespacerange 4 beginbfchar <41> <4E2D> <42> <0633> <43> <00C5> <44> <D83DDE00> endbfchar";
    let bytes = fixture(
        &[
            "BT /F1 12 Tf (ABCD) Tj ET",
            "BT /F1 12 Tf (123.45 + = $) Tj ET",
        ],
        Some(cmap),
        false,
    );
    assert!(inspect(&bytes).pages_needing_ocr.is_empty());
}

#[test]
fn isolated_corrupt_character_does_not_replace_good_prose() {
    let cmap =
        "1 begincodespacerange <00> <FF> endcodespacerange 1 beginbfchar <7E> <E000> endbfchar";
    let content = format!("{HEADER} BT /F1 12 Tf (Some readable words with one ~ error) Tj ET");
    let p = inspect(&fixture(&[&content], Some(cmap), false))
        .pages
        .remove(0);
    assert_eq!(p.route, PdfRoute::Native);
    assert_eq!(p.suspicious_characters, 1);
    assert!(p
        .reasons
        .contains(&PdfInspectionReason::SuspectTextEncoding));
}

struct Provider {
    calls: RefCell<Vec<u32>>,
    response: Option<&'static str>,
}
impl PdfOcrProvider for Provider {
    fn recognize(&self, request: PdfOcrRequest<'_>) -> Result<String, Error> {
        self.calls.borrow_mut().push(request.page);
        self.response
            .map(str::to_owned)
            .ok_or_else(|| Error::Malformed("provider unavailable".into()))
    }
}
fn parse(bytes: &[u8], response: Option<&'static str>, pages: usize) -> (ParseResult, Vec<u32>) {
    let provider = Provider {
        calls: RefCell::new(vec![]),
        response,
    };
    let result = Parser::default()
        .parse_with_pdf_ocr(
            bytes,
            Some("x.pdf"),
            &CancellationToken::default(),
            &provider,
            PdfOcrBudget {
                max_pages: pages,
                ..Default::default()
            },
        )
        .unwrap();
    (result, provider.calls.into_inner())
}
#[test]
fn selective_ocr_replaces_mixed_page_once_and_preserves_native_pages() {
    let bytes = fixture(&[HEADER, &format!("{HEADER} {SCAN}"), ""], None, false);
    let (result, calls) = parse(&bytes, Some("Annual results\nScanned body"), 8);
    assert_eq!(calls, [2]);
    assert_eq!(
        result.to_markdown().text.matches("Annual results").count(),
        2
    );
    assert!(result.to_markdown().text.contains("Scanned body"));
    assert_eq!(
        result.pdf_inspection.unwrap().pages[1].ocr_status,
        PdfOcrStatus::Applied
    );
}
#[test]
fn failed_empty_and_budgeted_ocr_retain_native_evidence() {
    let bytes = fixture(&[&format!("{HEADER} {SCAN}")], None, false);
    for (response, budget, status) in [
        (None, 8, PdfOcrStatus::Failed),
        (Some("   "), 8, PdfOcrStatus::Empty),
        (Some("replacement"), 0, PdfOcrStatus::BudgetExhausted),
    ] {
        let (result, calls) = parse(&bytes, response, budget);
        assert_eq!(result.to_markdown().text.trim(), "Annual results");
        assert_eq!(calls.len(), usize::from(budget > 0));
        assert_eq!(result.pdf_inspection.unwrap().pages[0].ocr_status, status);
    }
}
#[test]
fn unknown_page_errors_are_reported_not_silently_skipped() {
    let bytes = fixture(&["Q", HEADER], None, false);
    let p = inspect(&bytes);
    assert_eq!(p.kind, PdfPageKind::Unknown);
    assert_eq!(p.pages[0].route, PdfRoute::Review);
    assert!(p.pages[0]
        .reasons
        .contains(&PdfInspectionReason::ExtractionFailed));
    assert_eq!(p.pages[1].route, PdfRoute::Native);
}
#[test]
fn inspection_obeys_input_page_text_node_and_cancellation_limits() {
    let bytes = fixture(&[HEADER, HEADER], None, false);
    for field in [
        "max_input_bytes",
        "max_pages",
        "max_text_bytes",
        "max_nodes",
    ] {
        let options: Options =
            serde_json::from_value(serde_json::json!({"limits":{field:1}})).unwrap();
        assert!(
            matches!(
                Parser::new(options).inspect_pdf(&bytes),
                Err(Error::Limit(_))
            ),
            "{field}"
        );
    }
    let cancelled = CancellationToken::default();
    cancelled.cancel();
    assert!(matches!(
        Parser::default().inspect_pdf_with_cancel(&bytes, &cancelled),
        Err(Error::Cancelled)
    ));
}

#[test]
fn corrupt_ocr_does_not_replace_readable_native_text() {
    let bytes = fixture(&[&format!("{HEADER} {SCAN}")], None, false);
    let (result, _) = parse(&bytes, Some("\u{fffd}\u{fffd}\u{fffd}"), 8);
    assert_eq!(result.to_markdown().text.trim(), "Annual results");
    assert_eq!(
        result.pdf_inspection.unwrap().pages[0].ocr_status,
        PdfOcrStatus::Failed
    );
    assert!(result
        .diagnostics
        .iter()
        .any(|d| d.code == "pdf_ocr_rejected"));
}

#[test]
fn annotations_are_reviewed_and_unpainted_paths_are_skipped() {
    let bytes = fixture(&["", "10 10 50 50 re n S"], None, false);
    let mut doc = Document::load_mem(&bytes).unwrap();
    let id = doc.get_pages()[&1];
    doc.get_object_mut(id).unwrap().as_dict_mut().unwrap().set(
        "Annots",
        vec![Object::Dictionary(dictionary! {"Subtype"=>"Text"})],
    );
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let report = inspect(&bytes);
    assert_eq!(report.pages[0].route, PdfRoute::Review);
    assert_eq!(report.pages[1].route, PdfRoute::Skip);
}

#[test]
fn crop_and_form_bounds_limit_image_area() {
    let bytes = fixture(&[&format!("{HEADER} /Form Do")], None, true);
    let mut doc = Document::load_mem(&bytes).unwrap();
    for object in doc.objects.values_mut() {
        if let Object::Stream(stream) = object {
            if stream.dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Form") {
                stream
                    .dict
                    .set("BBox", vec![0.into(), 0.into(), 50.into(), 50.into()]);
            }
        }
    }
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let report = inspect(&bytes);
    assert!(report.pages[0].image_coverage.unwrap() < 0.02);
    assert_eq!(report.pages[0].route, PdfRoute::Native);
}

#[test]
fn reversed_boxes_and_invalid_optional_geometry_preserve_native_text() {
    for bbox in [
        vec![600.into(), 800.into(), 0.into(), 0.into()],
        vec![Object::Name(b"invalid".to_vec())],
    ] {
        let bytes = fixture(&[&format!("{HEADER} /Form Do")], None, true);
        let mut doc = Document::load_mem(&bytes).unwrap();
        for object in doc.objects.values_mut() {
            if let Object::Stream(stream) = object {
                if stream.dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Form") {
                    stream.dict.set("BBox", bbox.clone());
                }
            }
        }
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        let result = Parser::default()
            .parse_bytes(&bytes, Some("x.pdf"))
            .unwrap();
        assert_eq!(result.to_markdown().text.trim(), "Annual results");
        assert_eq!(
            result.pdf_inspection.unwrap().pages[0].kind,
            PdfPageKind::Mixed
        );
    }
}

#[test]
fn sparse_native_footer_does_not_hide_lines_of_outlined_text() {
    let outlines = |rows: usize, repeat: bool, curved: bool| {
        let mut s = String::new();
        for r in 0..rows {
            for c in 0..24 {
                let x = 40 + if repeat { 0 } else { c * 10 };
                let y = 300 + r * 20;
                s += &format!("q 1 0 0 1 {x} {y} cm ");
                s += if curved {
                    "0 0 m 6 0 l 8 2 8 8 6 10 c 0 10 l h f Q "
                } else {
                    "0 0 7 10 re f Q "
                };
            }
        }
        s
    };
    let footer = "BT /F1 10 Tf 40 40 Td (page 1) Tj ET ";
    let pages = [
        format!("{footer}{}", outlines(4, false, true)),
        format!("{footer}{}", outlines(1, false, true)),
        format!("{footer}{}", outlines(100, true, true)),
        format!("{footer}{}", outlines(20, false, false)),
        format!(
            "BT /F1 10 Tf 40 700 Td ({}) Tj ET {}",
            "Readable prose ".repeat(12),
            outlines(4, false, true)
        ),
        format!("{footer} q 1 0 0 1 900 0 cm {} Q", outlines(4, false, true)),
    ];
    let data = fixture(
        &pages.iter().map(String::as_str).collect::<Vec<_>>(),
        None,
        false,
    );
    let report = inspect(&data);
    assert_eq!(report.pages_needing_ocr, vec![1]);
    assert!(report.pages[0]
        .reasons
        .contains(&PdfInspectionReason::OutlinedTextWithSparseNativeText));
    assert_eq!(
        Parser::default()
            .parse_bytes(&data, Some("drawing.pdf"))
            .unwrap()
            .pdf_inspection
            .unwrap(),
        report
    );
}
