#![cfg(feature = "pdf")]
use bulkparse::{
    BlockContent, CancellationToken, Error, Inline, Options, Parser, PdfOcrBudget, PdfOcrProvider,
    PdfOcrRequest, Status,
};
use lopdf::{dictionary, Document, Object, Stream};
use std::cell::RefCell;

fn finish(mut doc: Document, resources: Object, contents: Vec<Vec<u8>>) -> Vec<u8> {
    let pages = doc.new_object_id();
    let mut kids = Vec::new();
    for bytes in contents {
        let content = doc.add_object(Stream::new(dictionary! {}, bytes));
        let page=doc.add_object(dictionary!{"Type"=>"Page","Parent"=>pages,"MediaBox"=>vec![0.into(),0.into(),600.into(),800.into()],"Resources"=>resources.clone(),"Contents"=>content});
        kids.push(page.into());
    }
    doc.objects.insert(
        pages,
        Object::Dictionary(dictionary! {"Type"=>"Pages","Count"=>kids.len() as i64,"Kids"=>kids}),
    );
    let catalog = doc.add_object(dictionary! {"Type"=>"Catalog","Pages"=>pages});
    doc.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    bytes
}
fn simple(content: &str) -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    let font =
        doc.add_object(dictionary! {"Type"=>"Font","Subtype"=>"Type1","BaseFont"=>"Courier"});
    finish(
        doc,
        dictionary! {"Font"=>dictionary!{"F1"=>font}}.into(),
        vec![content.as_bytes().to_vec()],
    )
}
fn styled(content: &str) -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    let normal =
        doc.add_object(dictionary! {"Type"=>"Font","Subtype"=>"Type1","BaseFont"=>"Courier"});
    let bold =
        doc.add_object(dictionary! {"Type"=>"Font","Subtype"=>"Type1","BaseFont"=>"Courier-Bold"});
    finish(
        doc,
        dictionary! {"Font"=>dictionary!{"F1"=>normal,"F2"=>bold}}.into(),
        vec![content.as_bytes().to_vec()],
    )
}
fn mapped(content: &str, cmap: &str) -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    let cmap = doc.add_object(Stream::new(dictionary! {}, cmap.as_bytes().to_vec()));
    let cid = doc.add_object(dictionary! {"Type"=>"Font","Subtype"=>"CIDFontType2","DW"=>500});
    let font=doc.add_object(dictionary!{"Type"=>"Font","Subtype"=>"Type0","Encoding"=>"Identity-H","DescendantFonts"=>vec![cid.into()],"ToUnicode"=>cmap});
    finish(
        doc,
        dictionary! {"Font"=>dictionary!{"F1"=>font}}.into(),
        vec![content.as_bytes().to_vec()],
    )
}
fn parse(bytes: &[u8]) -> bulkparse::ParseResult {
    Parser::default().parse_bytes(bytes, Some("x.pdf")).unwrap()
}
fn text(bytes: &[u8]) -> String {
    parse(bytes).to_markdown().text
}

#[test]
fn font_widths_text_matrices_and_graphics_state_recover_spaces() {
    let bytes=simple("q 2 0 0 2 10 20 cm BT /F1 10 Tf 1 0 0 1 20 200 Tm (Hello) Tj 33 0 Td (world) Tj ET Q BT /F1 10 Tf 1 0 0 1 50 380 Tm (Second line) Tj ET");
    let out = text(&bytes);
    assert!(out.contains("Hello world"), "{out}");
    assert!(out.contains("Second line"));
    assert!(out.find("Hello").unwrap() < out.find("Second").unwrap());
}
#[test]
fn columns_read_down_before_across_despite_interleaved_stream() {
    let bytes=simple("BT /F1 12 Tf 1 0 0 1 40 700 Tm (Left one) Tj 1 0 0 1 320 700 Tm (Right one) Tj 1 0 0 1 40 685 Tm (Left two) Tj 1 0 0 1 320 685 Tm (Right two) Tj ET");
    let out = text(&bytes);
    let a = out.find("Left one").unwrap();
    let b = out.find("Left two").unwrap();
    let c = out.find("Right one").unwrap();
    let d = out.find("Right two").unwrap();
    assert!(a < b && b < c && c < d, "{out}");
}
#[test]
fn compact_cmaps_ranges_ligatures_and_supplementary_characters() {
    let cmap="/CIDInit/ProcSet findresource begin 1 begincodespacerange<0000><FFFF>endcodespacerange 2 beginbfchar<0001><00660069><0002><D83DDE00>endbfchar 1 beginbfrange<0003><0005><0041>endbfrange end";
    assert_eq!(
        text(&mapped(
            "BT /F1 12 Tf 1 0 0 1 40 700 Tm <00010002000300040005> Tj ET",
            cmap
        ))
        .trim(),
        "fi😀ABC"
    );
}
#[test]
fn four_byte_cmap_ranges_are_not_expanded() {
    let cmap="1 begincodespacerange<00000000><FFFFFFFF>endcodespacerange 1 beginbfrange<00000000><FFFFFFFF><0041>endbfrange";
    let bytes = mapped("BT /F1 12 Tf <00000000> Tj ET", cmap);
    let mut options = Options::default();
    options.limits.max_nodes = 100;
    let result = Parser::new(options)
        .parse_bytes(&bytes, Some("x.pdf"))
        .unwrap();
    assert_eq!(result.to_markdown().text.trim(), "A");
}
#[test]
fn simple_encoding_fills_unmapped_tounicode_entries() {
    let mut doc = Document::with_version("1.5");
    let cmap = doc.add_object(Stream::new(
        dictionary! {},
        b"1 begincodespacerange<0000><FFFF>endcodespacerange 1 beginbfchar<80><20AC>endbfchar"
            .to_vec(),
    ));
    let font=doc.add_object(dictionary!{"Type"=>"Font","Subtype"=>"Type1","BaseFont"=>"Courier","Encoding"=>"WinAnsiEncoding","ToUnicode"=>cmap});
    let bytes = finish(
        doc,
        dictionary! {"Font"=>dictionary!{"F1"=>font}}.into(),
        vec![b"BT /F1 12 Tf (Price ) Tj <80> Tj ET".to_vec()],
    );
    assert_eq!(text(&bytes).trim(), "Price €");
}
const ARABIC:&str="1 begincodespacerange<0000><FFFF>endcodespacerange 4 beginbfchar<0001><0633><0002><0644><0003><0627><0004><0645>endbfchar";
#[test]
fn rtl_uses_positions_for_visual_and_logical_paint_order() {
    let visual = mapped(
        "BT /F1 12 Tf 1 0 0 1 40 700 Tm <0004000300020001> Tj ET",
        ARABIC,
    );
    let logical = mapped(
        "BT /F1 12 Tf 1 0 0 1 58 700 Tm [<0001> 1000 <0002> 1000 <0003> 1000 <0004>] TJ ET",
        ARABIC,
    );
    assert_eq!(text(&visual).trim(), "سلام");
    assert_eq!(text(&logical).trim(), "سلام");
}
#[test]
fn rtl_runs_preserve_latin_digits_and_glyph_sequences() {
    let cmap="1 begincodespacerange<0000><FFFF>endcodespacerange 8 beginbfchar<0001><05E9><0002><05DC><0003><05D5><0004><05DD><0005><0020><0006><0031><0007><0032><0008><0033>endbfchar";
    let bytes = mapped(
        "BT /F1 12 Tf 1 0 0 1 40 700 Tm <00060007000800050004000300020001> Tj ET",
        cmap,
    );
    assert_eq!(text(&bytes).trim(), "שלום 123");
}
#[test]
fn actualtext_is_authoritative_and_not_reversed() {
    let bytes=simple("BT /F1 12 Tf 1 0 0 1 40 700 Tm /Span <</ActualText <FEFF0633064406270645>>> BDC (placeholder) Tj EMC ET");
    assert_eq!(text(&bytes).trim(), "سلام");
}
#[test]
fn rotated_text_keeps_word_gaps_and_lines() {
    let bytes=simple("BT /F1 12 Tf 0 1 -1 0 50 50 Tm (First) Tj 39.6 0 Td (line) Tj -39.6 -15 Td (Second line) Tj ET");
    let out = text(&bytes);
    assert!(out.contains("First line"), "{out}");
    assert!(out.contains("Second line"));
    assert!(parse(&bytes)
        .diagnostics
        .iter()
        .any(|d| d.code == "pdf_rotated_text"));
}
#[test]
fn form_xobjects_use_local_resources_and_transform() {
    let mut doc = Document::with_version("1.5");
    let font =
        doc.add_object(dictionary! {"Type"=>"Font","Subtype"=>"Type1","BaseFont"=>"Courier"});
    let form=doc.add_object(Stream::new(dictionary!{"Type"=>"XObject","Subtype"=>"Form","BBox"=>vec![0.into(),0.into(),100.into(),100.into()],"Resources"=>dictionary!{"Font"=>dictionary!{"F1"=>font}},"Matrix"=>vec![1.into(),0.into(),0.into(),1.into(),40.into(),700.into()]},b"BT /F1 12 Tf (Inside form) Tj ET".to_vec()));
    let bytes = finish(
        doc,
        dictionary! {"XObject"=>dictionary!{"Fm"=>form}}.into(),
        vec![b"/Fm Do".to_vec()],
    );
    assert_eq!(text(&bytes).trim(), "Inside form");
}
#[test]
fn cyclic_form_and_malformed_cmap_are_diagnosed() {
    let mut doc = Document::with_version("1.5");
    let id = doc.new_object_id();
    doc.objects.insert(id,Object::Stream(Stream::new(dictionary!{"Type"=>"XObject","Subtype"=>"Form","Resources"=>dictionary!{"XObject"=>dictionary!{"Loop"=>id}}},b"/Loop Do".to_vec())));
    let bytes = finish(
        doc,
        dictionary! {"XObject"=>dictionary!{"Loop"=>id}}.into(),
        vec![b"/Loop Do".to_vec()],
    );
    assert!(parse(&bytes)
        .diagnostics
        .iter()
        .any(|d| d.code == "pdf_page_error"));
    let invalid = mapped(
        "BT /F1 12 Tf <0001> Tj ET",
        "1 beginbfrange<0001><0003>[<0041>]endbfrange",
    );
    assert!(parse(&invalid)
        .diagnostics
        .iter()
        .any(|d| d.code == "pdf_font_error"));
}
fn scanned() -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    let image=doc.add_object(Stream::new(dictionary!{"Type"=>"XObject","Subtype"=>"Image","Width"=>1,"Height"=>1,"ColorSpace"=>"DeviceGray","BitsPerComponent"=>8},vec![0]));
    let font =
        doc.add_object(dictionary! {"Type"=>"Font","Subtype"=>"Type1","BaseFont"=>"Courier"});
    finish(
        doc,
        dictionary! {"XObject"=>dictionary!{"Im"=>image},"Font"=>dictionary!{"F1"=>font}}.into(),
        vec![
            b"BT /F1 12 Tf (Native) Tj ET /Im Do".to_vec(),
            b"/Im Do".to_vec(),
            Vec::new(),
            b"/Im Do".to_vec(),
        ],
    )
}
struct Ocr {
    calls: RefCell<Vec<u32>>,
    fail: bool,
}
impl PdfOcrProvider for Ocr {
    fn recognize(&self, request: PdfOcrRequest<'_>) -> Result<String, Error> {
        assert!(request.document.starts_with(b"%PDF"));
        self.calls.borrow_mut().push(request.page);
        if self.fail {
            Err(Error::Malformed("provider unavailable".into()))
        } else {
            Ok("Recognized scan".into())
        }
    }
}
#[test]
fn ocr_is_opt_in_and_only_visits_image_only_pages_within_budget() {
    let bytes = scanned();
    assert!(parse(&bytes)
        .diagnostics
        .iter()
        .any(|d| d.code == "pdf_ocr_needed"));
    let ocr = Ocr {
        calls: RefCell::new(Vec::new()),
        fail: false,
    };
    let result = Parser::default()
        .parse_with_pdf_ocr(
            &bytes,
            Some("x.pdf"),
            &CancellationToken::default(),
            &ocr,
            PdfOcrBudget {
                max_pages: 1,
                max_output_bytes: 100,
            },
        )
        .unwrap();
    assert_eq!(*ocr.calls.borrow(), vec![2]);
    assert!(result.to_markdown().text.contains("Native"));
    assert!(result.to_markdown().text.contains("Recognized scan"));
    assert!(result
        .diagnostics
        .iter()
        .any(|d| d.code == "pdf_ocr_budget" && d.source.page == Some(4)));
    assert_eq!(result.status, Status::Partial);
}

#[test]
fn empty_ocr_output_is_not_reported_as_recovered_text() {
    struct Empty;
    impl PdfOcrProvider for Empty {
        fn recognize(&self, _: PdfOcrRequest<'_>) -> Result<String, Error> {
            Ok(" \n".into())
        }
    }
    let result = Parser::default()
        .parse_with_pdf_ocr(
            &scanned(),
            Some("scan.pdf"),
            &CancellationToken::default(),
            &Empty,
            PdfOcrBudget::default(),
        )
        .unwrap();
    assert!(result.diagnostics.iter().any(|d| d.code == "pdf_ocr_empty"));
    assert!(!result
        .diagnostics
        .iter()
        .any(|d| d.code == "pdf_ocr_applied"));
    assert_eq!(result.status, Status::Partial);
}
#[test]
fn ocr_errors_limits_and_cancellation_do_not_claim_success() {
    let bytes = scanned();
    let ocr = Ocr {
        calls: RefCell::new(Vec::new()),
        fail: true,
    };
    let result = Parser::default()
        .parse_with_pdf_ocr(
            &bytes,
            Some("x.pdf"),
            &CancellationToken::default(),
            &ocr,
            PdfOcrBudget::default(),
        )
        .unwrap();
    assert!(result.diagnostics.iter().any(|d| d.code == "pdf_ocr_error"));
    assert!(result.to_markdown().text.contains("Native"));
    let ocr = Ocr {
        calls: RefCell::new(Vec::new()),
        fail: false,
    };
    assert!(matches!(
        Parser::default().parse_with_pdf_ocr(
            &bytes,
            Some("x.pdf"),
            &CancellationToken::default(),
            &ocr,
            PdfOcrBudget {
                max_pages: 1,
                max_output_bytes: 2
            }
        ),
        Err(Error::Limit("pdf_ocr_output_bytes"))
    ));
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert!(matches!(
        Parser::default().parse_with_pdf_ocr(
            &bytes,
            Some("x.pdf"),
            &cancel,
            &ocr,
            PdfOcrBudget::default()
        ),
        Err(Error::Cancelled)
    ));
}

fn table_content(paint: &str, scale: f64, blank: bool) -> String {
    let mut s = format!("q {scale} 0 0 {scale} 10 20 cm ");
    for y in [720, 690, 660, 630] {
        s.push_str(&format!("40 {y} m 400 {y} l {paint} "));
    }
    s.push_str("BT /F1 12 Tf ");
    for (row, items) in [
        ["Metal", "Count", "Cost"],
        ["Tin", "19", "240"],
        ["Lead", "28", "350"],
    ]
    .iter()
    .enumerate()
    {
        for (col, t) in items.iter().enumerate() {
            if blank && row == 1 && col == 1 {
                continue;
            }
            s.push_str(&format!(
                "1 0 0 1 {} {} Tm ({t}) Tj ",
                50 + col * 130,
                700 - row * 30
            ));
        }
    }
    s.push_str("ET Q");
    s
}
fn table_values(result: &bulkparse::ParseResult) -> Vec<Vec<Vec<String>>> {
    result
        .document
        .blocks
        .iter()
        .filter_map(|b| {
            if let BlockContent::Table { rows, header_rows } = &b.content {
                assert_eq!(*header_rows, 0);
                Some(
                    rows.iter()
                        .map(|r| {
                            r.iter()
                                .map(|c| {
                                    c.blocks
                                        .iter()
                                        .map(|b| {
                                            if let BlockContent::Paragraph { inlines } = &b.content
                                            {
                                                inlines
                                                    .iter()
                                                    .map(|i| {
                                                        if let Inline::Text { text } = i {
                                                            text.as_str()
                                                        } else {
                                                            ""
                                                        }
                                                    })
                                                    .collect::<String>()
                                            } else {
                                                String::new()
                                            }
                                        })
                                        .collect::<Vec<_>>()
                                        .join("\n")
                                })
                                .collect()
                        })
                        .collect(),
                )
            } else {
                None
            }
        })
        .collect()
}
#[test]
fn ruled_tables_preserve_cells_provenance_and_surrounding_text() {
    for scale in [0.75, 1., 1.5] {
        let s=format!("BT /F1 12 Tf 1 0 0 1 50 1150 Tm (Introduction) Tj ET {} BT /F1 12 Tf 1 0 0 1 50 100 Tm (Conclusion) Tj ET",table_content("S",scale,false));
        // This scaled fixture intentionally reaches y=1150: give it a page
        // that contains the drawing instead of relying on off-page extraction.
        let mut doc = Document::load_mem(&simple(&s)).unwrap();
        for id in doc.get_pages().values() {
            doc.get_dictionary_mut(*id).unwrap().set(
                "MediaBox",
                vec![0.into(), 0.into(), 1000.into(), 1200.into()],
            );
        }
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        let result = parse(&bytes);
        let values = table_values(&result);
        assert_eq!(
            values,
            vec![vec![
                vec!["Metal", "Count", "Cost"],
                vec!["Tin", "19", "240"],
                vec!["Lead", "28", "350"]
            ]]
        );
        assert!(matches!(
            result.document.blocks[0].content,
            BlockContent::Paragraph { .. }
        ));
        let table = result
            .document
            .blocks
            .iter()
            .find(|b| matches!(b.content, BlockContent::Table { .. }))
            .unwrap();
        if let BlockContent::Table { rows, .. } = &table.content {
            assert_eq!(rows[1][2].source.page, Some(1));
            assert_eq!(rows[1][2].source.row, Some(2));
            assert_eq!(rows[1][2].source.cell, Some(3));
        }
        let md = result.to_markdown().text;
        assert!(md.find("Introduction").unwrap() < md.find("Metal").unwrap());
        assert!(md.find("350").unwrap() < md.find("Conclusion").unwrap());
        assert_eq!(md.matches("Metal").count(), 1);
    }
}
#[test]
fn table_empty_cells_are_not_shifted_and_limits_apply() {
    let bytes = simple(&table_content("S", 1., true));
    assert_eq!(table_values(&parse(&bytes))[0][1], vec!["Tin", "", "240"]);
    let mut options = Options::default();
    options.limits.max_table_cells = 8;
    assert!(matches!(
        Parser::new(options).parse_bytes(&bytes, Some("x.pdf")),
        Err(Error::Limit("max_table_cells"))
    ));
}
#[test]
fn unpainted_lines_and_aligned_prose_do_not_become_tables() {
    assert!(table_values(&parse(&simple(&table_content("n", 1., false)))).is_empty());
    let s="40 720 m 400 720 l S 40 690 m 400 690 l S 40 660 m 400 660 l S BT /F1 12 Tf 1 0 0 1 50 700 Tm (A normal sentence continues here.) Tj 1 0 0 1 50 670 Tm (Another ordinary paragraph follows.) Tj ET";
    assert!(table_values(&parse(&simple(s))).is_empty());
}
#[test]
fn filled_rule_rectangles_and_wrapped_cells_are_supported() {
    let s=table_content("n",1.,false)+" 50 740 360 1 re f 50 710 360 1 re f 50 680 360 1 re f 50 650 360 1 re f BT /F1 8 Tf 1 0 0 1 60 703 Tm (extra) Tj ET";
    let values = table_values(&parse(&simple(&s)));
    assert_eq!(values.len(), 1);
    assert!(values[0][1][0].contains("Tin"));
    assert!(values[0][1][0].contains("extra"));
}

#[test]
fn vertical_borders_keep_tall_wrapped_cells_in_one_row() {
    let mut content = String::new();
    for y in [730, 700, 400, 370] {
        content += &format!("40 {y} m 360 {y} l S ");
    }
    for x in [40, 200, 360] {
        content += &format!("{x} 730 m {x} 370 l S ");
    }
    content += "BT /F1 10 Tf ";
    for (x, y, text) in [
        (48, 715, "Label"),
        (208, 715, "Value"),
        (48, 680, "First line"),
        (48, 660, "Second line"),
        (48, 640, "Third line"),
        (208, 600, "42"),
        (48, 385, "Last row"),
        (208, 385, "99"),
    ] {
        content += &format!("1 0 0 1 {x} {y} Tm ({text}) Tj ");
    }
    content += "ET";
    let tables = table_values(&parse(&simple(&content)));
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].len(), 3);
    assert_eq!(
        tables[0][1],
        vec!["First line\nSecond line\nThird line", "42"]
    );
}

#[test]
fn shaded_cell_rectangles_join_fragmented_edges_and_keep_empty_columns() {
    let mut content = String::new();
    for row in 0..3 {
        for col in 0..3 {
            content += &format!("{} {} 140 40 re f ", 40 + col * 140, 730 - (row + 1) * 40);
        }
    }
    content += "BT /F1 10 Tf ";
    for (row, label, value) in [
        (0, "Item", "Count"),
        (1, "Linen", "12"),
        (2, "Cotton", "34"),
    ] {
        let y = 715 - row * 40;
        content += &format!("1 0 0 1 48 {y} Tm ({label}) Tj 1 0 0 1 188 {y} Tm ({value}) Tj ");
    }
    content += "ET";
    assert_eq!(
        table_values(&parse(&simple(&content))),
        vec![vec![
            vec!["Item", "Count", ""],
            vec!["Linen", "12", ""],
            vec!["Cotton", "34", ""]
        ]]
    );
    // Paths that were never painted must not provide table boundaries.
    assert!(table_values(&parse(&simple(&content.replace(" re f ", " re n ")))).is_empty());
}

fn numeric_grid(sparse_rules: bool) -> String {
    let mut content = if sparse_rules {
        "40 730 m 460 730 l S 40 700 m 460 700 l S 40 600 m 460 600 l S ".to_owned()
    } else {
        String::new()
    };
    content += "BT /F1 10 Tf ";
    for (x, label) in [
        (48, "District"),
        (178, "East"),
        (278, "West"),
        (378, "North"),
    ] {
        content += &format!("1 0 0 1 {x} 715 Tm ({label}) Tj ");
    }
    for row in 0..5 {
        let y = 685 - row * 15;
        content += &format!("1 0 0 1 48 {y} Tm (Zone {}) Tj ", row + 1);
        for (col, x) in [178, 278, 378].into_iter().enumerate() {
            if row == 4 && col == 1 {
                continue;
            }
            content += &format!("1 0 0 1 {x} {y} Tm ({}) Tj ", 10 + row * 3 + col);
        }
    }
    content += "ET";
    content
}

#[test]
fn sparse_rules_split_repeated_numeric_rows_without_merging_them() {
    let result = parse(&simple(&numeric_grid(true)));
    let tables = table_values(&result);
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].len(), 6);
    assert_eq!(tables[0][1], vec!["Zone 1", "10", "11", "12"]);
    assert_eq!(tables[0][5], vec!["Zone 5", "22", "", "24"]);
}

#[test]
fn borderless_numeric_tables_include_aligned_headers_and_keep_sparse_cells() {
    let content=numeric_grid(false)+" BT /F1 10 Tf 1 0 0 1 40 570 Tm (A following paragraph remains outside the table.) Tj ET";
    let bytes = simple(&content);
    let result = parse(&bytes);
    let tables = table_values(&result);
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].len(), 6);
    assert_eq!(tables[0][5], vec!["Zone 5", "22", "", "24"]);
    assert!(
        result.to_markdown().text.find("</table>").unwrap()
            < result
                .to_markdown()
                .text
                .find("following paragraph")
                .unwrap()
    );
    let mut options = Options::default();
    options.limits.max_table_cells = 23;
    assert!(matches!(
        Parser::new(options).parse_bytes(&bytes, Some("grid.pdf")),
        Err(Error::Limit("max_table_cells"))
    ));
}

#[test]
fn native_tables_emit_exact_rows_and_escape_source_html() {
    let result = parse(&simple(
        &table_content("S", 1., false).replace("Metal", "<x>&"),
    ));
    let markdown = result.to_markdown().text;
    assert_eq!(markdown.matches("<tr>").count(), 3);
    assert_eq!(markdown.matches("<td>").count(), 9);
    assert!(markdown.contains("<td>&lt;x&gt;&amp;</td>"), "{markdown}");
    assert!(!markdown.contains("<x>"));
    assert!(!markdown.contains("| ---"));
}

#[test]
fn table_extraction_keeps_body_font_evidence_for_the_page_title() {
    let content =
        "BT /F1 20 Tf 1 0 0 1 40 780 Tm (Quarterly report) Tj ET ".to_owned() + &numeric_grid(true);
    let result = parse(&simple(&content));
    assert!(result
        .to_markdown()
        .text
        .starts_with("# Quarterly report\n"));
    assert_eq!(table_values(&result)[0].len(), 6);
}
#[test]
fn unicode_expansion_defaults_to_selective_and_keeps_actualtext() {
    let cmap="1 begincodespacerange<0000><FFFF>endcodespacerange 4 beginbfchar<0001><FEFB><0002><FB03><0003><2460><0004><FF21>endbfchar";
    let bytes = mapped(
        "BT /F1 12 Tf 1 0 0 1 40 700 Tm <0001> Tj 0 -20 Td <0002> Tj 0 -20 Td <0003> Tj 0 -20 Td <0004> Tj ET",
        cmap,
    );
    let raw = Parser::new(Options {
        pdf_normalize_unicode: false,
        ..Default::default()
    })
    .parse_bytes(&bytes, Some("x.pdf"))
    .unwrap()
    .to_markdown()
    .text;
    assert!(raw.contains('ﻻ'));
    assert!(raw.contains('ﬃ'));
    let options: Options = serde_json::from_str("{}").unwrap();
    assert!(options.pdf_normalize_unicode);
    let result = Parser::new(options.clone())
        .parse_bytes(&bytes, Some("x.pdf"))
        .unwrap();
    let out = result.to_markdown().text;
    assert!(out.contains("لا"));
    assert!(out.contains("ffi"));
    assert!(out.contains('①'));
    assert!(out.contains('Ａ'));
    assert!(result
        .diagnostics
        .iter()
        .any(|d| d.code == "pdf_unicode_normalized" && !d.content_loss));
    let bytes = simple("BT /F1 12 Tf /Span <</ActualText <FEFFFB03>>> BDC (x) Tj EMC ET");
    assert_eq!(
        Parser::new(options)
            .parse_bytes(&bytes, Some("x.pdf"))
            .unwrap()
            .to_markdown()
            .text
            .trim(),
        "ﬃ"
    );
}
#[test]
fn unicode_expansion_obeys_text_budget() {
    let bytes = mapped(
        "BT /F1 12 Tf <0001> Tj ET",
        "1 begincodespacerange<0000><FFFF>endcodespacerange 1 beginbfchar<0001><FDFA>endbfchar",
    );
    let mut options = Options {
        pdf_normalize_unicode: true,
        ..Default::default()
    };
    options.limits.max_text_bytes = 10;
    assert!(matches!(
        Parser::new(options).parse_bytes(&bytes, Some("x.pdf")),
        Err(Error::Limit("max_text_bytes"))
    ));
}

#[test]
fn short_rule_fragments_must_not_cut_words_into_cells() {
    let mut s = String::from(
        "55 720 m 400 720 l S 55 690 m 400 690 l S 55 660 m 400 660 l S BT /F1 12 Tf ",
    );
    for y in [700, 670] {
        s.push_str(&format!(
            "1 0 0 1 40 {y} Tm (Unbroken) Tj 1 0 0 1 240 {y} Tm (Value) Tj "
        ));
    }
    s.push_str("ET");
    let result = parse(&simple(&s));
    assert!(table_values(&result).is_empty());
    assert_eq!(result.to_markdown().text.matches("Unbroken").count(), 2);
}

#[test]
fn larger_isolated_lines_become_headings_without_promoting_uniform_text() {
    let body = "BT /F1 10 Tf 1 0 0 1 40 660 Tm (This paragraph provides sufficient ordinary body text.) Tj 1 0 0 1 40 645 Tm (Another line of prose establishes the body font size.) Tj ET";
    let bytes = simple(&("BT /F1 20 Tf 1 0 0 1 40 740 Tm (Harbour report) Tj /F1 15 Tf 1 0 0 1 40 700 Tm (Annual summary) Tj ET ".to_owned() + body));
    let result = parse(&bytes);
    let levels: Vec<_> = result
        .document
        .blocks
        .iter()
        .filter_map(|b| match b.content {
            BlockContent::Heading { level, .. } => Some(level),
            _ => None,
        })
        .collect();
    assert_eq!(levels, [1, 2]);
    assert!(result.to_markdown().text.starts_with("# Harbour report"));
    let mut limited = Options {
        pdf_normalize_unicode: false,
        ..Default::default()
    };
    limited.limits.max_text_bytes = 120;
    assert!(matches!(
        Parser::new(limited).parse_bytes(&bytes, Some("x.pdf")),
        Err(Error::Limit("max_text_bytes"))
    ));
    let uniform = parse(&simple("BT /F1 18 Tf 1 0 0 1 40 700 Tm (Short line) Tj 1 0 0 1 40 670 Tm (Another line) Tj 1 0 0 1 40 640 Tm (Final line) Tj ET"));
    assert!(uniform
        .document
        .blocks
        .iter()
        .all(|b| !matches!(b.content, BlockContent::Heading { .. })));
    let dropcap = parse(&simple(
        &("BT /F1 25 Tf 1 0 0 1 40 700 Tm (A) Tj /F1 10 Tf (n ordinary opening line) Tj ET "
            .to_owned()
            + body),
    ));
    assert!(dropcap
        .document
        .blocks
        .iter()
        .all(|b| !matches!(b.content, BlockContent::Heading { .. })));
    for label in ["A", "A hyphenated continu-", "A complete sentence."] {
        let result = parse(&simple(
            &(format!("BT /F1 20 Tf 1 0 0 1 40 700 Tm ({label}) Tj ET ") + body),
        ));
        assert!(result
            .document
            .blocks
            .iter()
            .all(|b| !matches!(b.content, BlockContent::Heading { .. })));
    }
}

#[test]
fn repeated_numeric_rows_form_a_headerless_table_but_prose_stays_column_ordered() {
    let mut content = String::from("BT /F1 10 Tf ");
    for (row, label) in ["Orchard", "Meadow", "Forest", "Valley"].iter().enumerate() {
        let y = 700 - row * 15;
        content += &format!("1 0 0 1 40 {y} Tm ({label}) Tj ");
        for (col, x) in [180, 250, 320].iter().enumerate() {
            content += &format!("1 0 0 1 {x} {y} Tm ({}) Tj ", 101 + row * 10 + col);
        }
    }
    content += "ET";
    let result = parse(&simple(&content));
    let out = result.to_markdown().text;
    let tables = table_values(&result);
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].len(), 4);
    assert_eq!(tables[0][0], vec!["Orchard", "101", "102", "103"]);
    assert_eq!(tables[0][1], vec!["Meadow", "111", "112", "113"]);
    assert_eq!(out.matches("<tr>").count(), 4);
    // A short aligned numeric list is not enough evidence without a header.
    let short = content
        .replace("1 0 0 1 40 655 Tm (Valley) Tj ", "")
        .replace("1 0 0 1 180 655 Tm (131) Tj ", "")
        .replace("1 0 0 1 250 655 Tm (132) Tj ", "")
        .replace("1 0 0 1 320 655 Tm (133) Tj ", "");
    assert!(table_values(&parse(&simple(&short))).is_empty());
    // Two parallel prose columns containing numbers remain column ordered.
    let prose = text(&simple("BT /F1 10 Tf 1 0 0 1 40 700 Tm (Revenue rose 12 percent) Tj 1 0 0 1 320 700 Tm (Costs fell 10 percent) Tj 1 0 0 1 40 685 Tm (Sales rose 15 percent) Tj 1 0 0 1 320 685 Tm (Staff grew 20 percent) Tj 1 0 0 1 40 670 Tm (Profit rose 30 percent) Tj 1 0 0 1 320 670 Tm (Rent fell 12 percent) Tj ET"));
    assert!(
        prose.find("Profit").unwrap() < prose.find("Costs").unwrap(),
        "{prose}"
    );
}

#[test]
fn footnote_heavy_pages_do_not_turn_body_prose_into_headings() {
    let mut content = String::from("BT /F1 10 Tf ");
    for i in 0..4 {
        content += &format!(
            "1 0 0 1 40 {} Tm (Ordinary body text continues on this line) Tj ",
            700 - i * 14
        );
    }
    content += "/F1 7 Tf ";
    for i in 0..12 {
        content += &format!("1 0 0 1 40 {} Tm (Footnote text uses a smaller font and occupies most of this page) Tj ", 500 - i * 10);
    }
    content += "ET";
    let result = parse(&simple(&content));
    assert!(result
        .document
        .blocks
        .iter()
        .all(|b| !matches!(b.content, BlockContent::Heading { .. })));
}

#[test]
fn sparse_numeric_columns_do_not_force_row_order() {
    // Only two numeric cells per row are ambiguous parallel columns.
    let out = text(&simple("BT /F1 10 Tf 1 0 0 1 40 700 Tm (111) Tj 1 0 0 1 320 700 Tm (211) Tj 1 0 0 1 40 685 Tm (112) Tj 1 0 0 1 320 685 Tm (212) Tj 1 0 0 1 40 670 Tm (113) Tj 1 0 0 1 320 670 Tm (213) Tj ET"));
    assert!(out.find("113").unwrap() < out.find("211").unwrap(), "{out}");
}

fn merged_grid() -> String {
    let mut s = String::from("40 560 360 160 re S 160 560 m 160 720 l S 280 560 m 280 680 l S 160 680 m 400 680 l S 40 640 m 400 640 l S 40 600 m 400 600 l S BT /F1 10 Tf ");
    for (x, y, t) in [
        (50, 690, "Group"),
        (170, 700, "Shared heading"),
        (170, 660, "A"),
        (290, 660, "B"),
        (50, 620, "First"),
        (170, 620, "10"),
        (290, 620, "20"),
        (50, 580, "Second"),
        (170, 580, "30"),
        (290, 580, "40"),
    ] {
        s.push_str(&format!("1 0 0 1 {x} {y} Tm ({t}) Tj "));
    }
    s.push_str("ET");
    s
}

#[test]
fn explicit_grid_recovers_row_and_column_spans_with_logical_provenance() {
    let result = parse(&simple(&merged_grid()));
    let tables: Vec<_> = result
        .document
        .blocks
        .iter()
        .filter_map(|b| {
            if let BlockContent::Table { rows, .. } = &b.content {
                Some(rows)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(tables.len(), 1);
    let rows = tables[0];
    assert_eq!(rows.len(), 4);
    assert_eq!((rows[0][0].row_span, rows[0][0].col_span), (2, 1));
    assert_eq!((rows[0][1].row_span, rows[0][1].col_span), (1, 2));
    assert_eq!(rows[1].len(), 2);
    assert_eq!(rows[1][0].source.cell, Some(2));
    assert_eq!(rows[1][1].source.cell, Some(3));
    assert_eq!(rows[1][0].source.row, Some(2));
    let md = result.to_markdown().text;
    assert!(md.contains("<td rowspan=\"2\">Group</td>"), "{md}");
    assert!(md.contains("<td colspan=\"2\">Shared heading</td>"), "{md}");
    let json = serde_json::to_value(result).unwrap();
    assert!(json.to_string().contains("\"row_span\":2"));
    let mut options = Options::default();
    options.limits.max_table_cells = 11;
    assert!(matches!(
        Parser::new(options).parse_bytes(&simple(&merged_grid()), Some("merged.pdf")),
        Err(Error::Limit("max_table_cells"))
    ));
}

#[test]
fn closed_grid_retains_empty_worksheet_cells() {
    let s="40 600 360 120 re S 160 600 m 160 720 l S 280 600 m 280 720 l S 40 690 m 400 690 l S 40 660 m 400 660 l S 40 630 m 400 630 l S BT /F1 10 Tf 1 0 0 1 50 700 Tm (Item) Tj 1 0 0 1 170 700 Tm (Observed) Tj 1 0 0 1 290 700 Tm (Expected) Tj 1 0 0 1 50 670 Tm (First) Tj 1 0 0 1 50 640 Tm (Second) Tj 1 0 0 1 50 610 Tm (Third) Tj ET";
    let result = parse(&simple(s));
    let tables = table_values(&result);
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].len(), 4);
    assert_eq!(tables[0][2], vec!["Second", "", ""]);
}

#[test]
fn mitred_filled_borders_and_outline_rings_define_cells() {
    let mut s=String::from("40 600 m 40 720 l 400 720 l 400 600 l 40 600 l 41 601 l 399 601 l 399 719 l 41 719 l 41 601 l h f ");
    for y in [680, 640] {
        s.push_str(&format!(
            "40 {y} m 400 {y} l 399 {} l 41 {} l h f ",
            y - 1,
            y - 1
        ));
    }
    s.push_str("220 720 m 220 600 l 219 601 l 219 719 l h f BT /F1 10 Tf ");
    for (r, y) in [700, 660, 620].into_iter().enumerate() {
        for (c, x) in [50, 230].into_iter().enumerate() {
            s.push_str(&format!("1 0 0 1 {x} {y} Tm (R{r}C{c}) Tj "));
        }
    }
    s.push_str("ET");
    let result = parse(&simple(&s));
    assert_eq!(
        table_values(&result),
        vec![vec![
            vec!["R0C0", "R0C1"],
            vec!["R1C0", "R1C1"],
            vec!["R2C0", "R2C1"]
        ]]
    );
}

#[test]
fn non_rectangular_missing_edges_do_not_create_merged_cells() {
    let s="40 600 360 120 re S 160 600 m 160 660 l S 280 600 m 280 720 l S 160 660 m 400 660 l S BT /F1 10 Tf 1 0 0 1 50 700 Tm (A) Tj 1 0 0 1 170 700 Tm (B) Tj 1 0 0 1 290 700 Tm (C) Tj 1 0 0 1 50 630 Tm (D) Tj 1 0 0 1 170 630 Tm (E) Tj 1 0 0 1 290 630 Tm (F) Tj ET";
    let result = parse(&simple(s));
    for b in &result.document.blocks {
        if let BlockContent::Table { rows, .. } = &b.content {
            assert!(rows
                .iter()
                .flatten()
                .all(|c| c.row_span == 1 && c.col_span == 1));
        }
    }
    let md = result.to_markdown().text;
    for word in ["A", "B", "C", "D", "E", "F"] {
        assert!(md.contains(word), "{md}");
    }
}

#[test]
fn open_worksheet_recovers_separate_headers_and_blank_answers() {
    let mut s = String::from("40 600 m 40 720 l S 160 600 m 160 680 l S 280 600 m 280 680 l S ");
    for y in [720, 680, 640, 600] {
        s.push_str(&format!("40 {y} m 400 {y} l S "));
    }
    s.push_str("BT /F1 10 Tf 1 0 0 1 50 700 Tm (Prompt) Tj 1 0 0 1 170 700 Tm (First) Tj 1 0 0 1 290 700 Tm (Second) Tj 1 0 0 1 50 660 Tm (Before) Tj 1 0 0 1 50 620 Tm (After) Tj 1 0 0 1 170 660 Tm ( ) Tj 1 0 0 1 290 620 Tm ( ) Tj ET");
    assert_eq!(
        table_values(&parse(&simple(&s))),
        vec![vec![
            vec!["Prompt", "First", "Second"],
            vec!["Before", "", ""],
            vec!["After", "", ""]
        ]]
    );
}

#[test]
fn nested_unboxed_headers_keep_both_axes_of_spans() {
    let mut s=String::from("40 730 m 500 730 l S 40 590 m 500 590 l S 140 700 m 500 700 l S 140 680 m 300 680 l S 320 680 m 500 680 l S BT /F1 10 Tf ");
    for (x, y, t) in [
        (50, 710, "Measure"),
        (280, 710, "Samples"),
        (180, 690, "Group A"),
        (360, 690, "Group B"),
    ] {
        s.push_str(&format!("1 0 0 1 {x} {y} Tm ({t}) Tj "));
    }
    for (c, x) in [140, 200, 260, 320, 380, 440].iter().enumerate() {
        s.push_str(&format!("1 0 0 1 {x} 670 Tm (C{c}) Tj "));
    }
    for (r, y) in [650, 630, 610].iter().enumerate() {
        s.push_str(&format!("1 0 0 1 50 {y} Tm (Item{r}) Tj "));
        for x in [140, 200, 260, 320, 380, 440] {
            s.push_str(&format!("1 0 0 1 {x} {y} Tm (12) Tj "));
        }
    }
    s.push_str("ET");
    let result = parse(&simple(&s));
    let rows = result
        .document
        .blocks
        .iter()
        .find_map(|b| {
            if let BlockContent::Table { rows, .. } = &b.content {
                Some(rows)
            } else {
                None
            }
        })
        .expect("nested header table");
    assert_eq!(rows.len(), 6);
    assert_eq!((rows[0][0].row_span, rows[0][1].col_span), (3, 6));
    assert_eq!(
        rows[1].iter().map(|c| c.col_span).collect::<Vec<_>>(),
        [3, 3]
    );
    assert_eq!(rows[2].len(), 6);
    assert_eq!(rows[5].len(), 7);
    let mut options = Options::default();
    options.limits.max_table_cells = 20;
    assert!(matches!(
        Parser::new(options).parse_bytes(&simple(&s), Some("nested.pdf")),
        Err(Error::Limit("max_table_cells"))
    ));
}

#[test]
fn styled_prose_rows_retain_wrapped_text_and_bottom_aligned_group_labels() {
    let mut s = String::from("0.9 g 40 430 500 300 re f 0 g BT ");
    for (r, y) in [710, 660, 610, 560, 510, 460].iter().enumerate() {
        for (c, x) in [50, 170, 290, 410].iter().enumerate() {
            if c == 0 && (r == 2 || r == 3) {
                continue;
            }
            let font = if c < 2 || r == 0 { "F2" } else { "F1" };
            s.push_str(&format!(
                "/{font} 10 Tf 1 0 0 1 {x} {y} Tm (Row{r}Col{c}) Tj "
            ));
        }
    }
    s.push_str(
        "/F1 10 Tf 1 0 0 1 290 595 Tm (Continued) Tj 1 0 0 1 290 575 Tm (Final words) Tj ET",
    );
    let result = parse(&styled(&s));
    let values = table_values(&result);
    assert_eq!(values.len(), 1, "{}", result.to_markdown().text);
    assert_eq!(values[0][2][1], "Row2Col1");
    assert_eq!(values[0][2][2], "Row2Col2\nContinued\nFinal words");
    assert_eq!(values[0][3][1], "Row3Col2");
    let rows = result
        .document
        .blocks
        .iter()
        .find_map(|b| {
            if let BlockContent::Table { rows, .. } = &b.content {
                Some(rows)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(rows[2][0].row_span, 3);
}

#[test]
fn same_size_bold_headings_survive_but_captions_and_sentence_leads_do_not() {
    let s="BT /F2 10 Tf 1 0 0 1 40 750 Tm (2.) Tj 1 0 0 1 65 750 Tm (Methods) Tj /F1 10 Tf 1 0 0 1 40 720 Tm (Ordinary body prose provides plenty of evidence for the body size.) Tj 1 0 0 1 40 705 Tm (Another sentence explains the example in enough detail.) Tj /F2 10 Tf 1 0 0 1 40 670 Tm (Table 2. Counts) Tj 1 0 0 1 40 640 Tm (Comparison to other methods.) Tj /F1 10 Tf 1 0 0 1 40 625 Tm (This sentence continues that paragraph after its bold lead.) Tj ET";
    let out = text(&styled(s));
    assert!(out.starts_with("# 2. Methods"), "{out}");
    assert_eq!(
        out.lines().filter(|l| l.starts_with('#')).count(),
        1,
        "{out}"
    );
}

#[test]
fn centered_wrapped_titles_form_one_heading() {
    let s="BT /F2 20 Tf 1 0 0 1 100 750 Tm (A Centred Title) Tj 1 0 0 1 124 726 Tm (Second Line) Tj /F1 10 Tf 1 0 0 1 40 670 Tm (Ordinary prose gives this page sufficient body text evidence.) Tj 1 0 0 1 40 655 Tm (Another body line completes the authored example.) Tj ET";
    let out = text(&styled(s));
    assert!(out.starts_with("# A Centred Title Second Line"), "{out}");
    assert_eq!(out.lines().filter(|l| l.starts_with('#')).count(), 1);
}

#[test]
fn separately_numbered_headings_do_not_merge_even_when_close_and_same_style() {
    for (first, second) in [
        ("IV.8 Delivery options", "IV.8.1 Schedule changes"),
        ("2. Controls", "3. Records"),
        ("2 Controls", "3 Records"),
        ("A.1 Sampling", "A.2 Analysis"),
        ("IV. Scope", "V. Process"),
        ("(a) Scope", "(b) Process"),
        ("Overview", "2. Requirements"),
    ] {
        // Aligned lines exercise the pre-order continuation merge; the offset
        // also exercises the later overlapping-heading merge independently.
        for x in [40, 90] {
            let escape = |s: &str| s.replace('(', "\\(").replace(')', "\\)");
            let stream = format!(
                "BT /F2 14 Tf 1 0 0 1 40 750 Tm ({}) Tj 1 0 0 1 {x} 732 Tm ({}) Tj /F1 10 Tf 1 0 0 1 40 680 Tm (Ordinary body prose establishes a consistent smaller text size.) Tj 1 0 0 1 40 665 Tm (Further explanation belongs to the body of this authored example.) Tj ET",
                escape(first), escape(second)
            );
            let out = text(&styled(&stream));
            assert_eq!(
                out.lines().filter(|l| l.starts_with('#')).count(),
                2,
                "{first} / {second}, x={x}: {out}"
            );
        }
    }
}

#[test]
fn numbered_titles_still_join_unmarked_continuations_and_display_years() {
    for (first, second, joins) in [
        ("2. Measurements", "and observations", true),
        ("2. Imaging with", "3D reconstruction", true),
        // A numeric-only line already stays plain rather than becoming a heading.
        ("Annual review", "2028", false),
        ("Research overview for", "2037 Release", true),
        ("Counting methods for", "250 Samples", true),
        ("III.", "Manufacturing methods", true),
    ] {
        let stream = format!(
            "BT /F2 14 Tf 1 0 0 1 40 750 Tm ({first}) Tj 1 0 0 1 40 732 Tm ({second}) Tj /F1 10 Tf 1 0 0 1 40 680 Tm (Ordinary body prose establishes a consistent smaller text size.) Tj 1 0 0 1 40 665 Tm (Further explanation belongs to the body of this authored example.) Tj ET"
        );
        let out = text(&styled(&stream));
        assert_eq!(
            out.lines().filter(|l| l.starts_with('#')).count(),
            1,
            "{out}"
        );
        assert!(out.contains(second), "{out}");
        assert_eq!(
            out.lines()
                .find(|l| l.starts_with('#'))
                .unwrap()
                .contains(second),
            joins,
            "{out}"
        );
    }
}

fn header_field_example(
    numbered: bool,
    rows: usize,
    transform: (f64, f64),
    value_dx: f64,
    value_dy: f64,
    value_size: f64,
    colon: bool,
) -> String {
    let (dx, scale) = transform;
    let mut stream = String::from("BT ");
    for (i, (name, value, value_x)) in [
        ("Record", "AX 582-C", 410.),
        ("Issued", "May 4, 2035", 403.),
    ]
    .into_iter()
    .take(rows)
    .enumerate()
    {
        let label = format!(
            "{}{}{}",
            if numbered {
                format!("\\({}\\) ", i + 7)
            } else {
                String::new()
            },
            name,
            if colon { ":" } else { "" }
        );
        let y = 600. - i as f64 * 20.;
        let size = if i == 0 { value_size } else { value_size * 0.8 };
        stream.push_str(&format!(
            "/F2 {} Tf 1 0 0 1 {} {} Tm ({label}) Tj /F2 {} Tf 1 0 0 1 {} {} Tm ({value}) Tj ",
            12. * scale,
            (310. + dx) * scale,
            y * scale,
            size * scale,
            (value_x + dx + value_dx) * scale,
            (y + value_dy) * scale
        ));
    }
    stream.push_str(&format!("/F1 {} Tf 1 0 0 1 40 {} Tm (Ordinary body prose provides enough evidence for the smaller regular font.) Tj 1 0 0 1 40 {} Tm (This additional explanatory sentence belongs to the body of the document.) Tj ET",10.*scale,530.*scale,515.*scale));
    text(&styled(&stream))
}

#[test]
fn repeated_heading_fields_join_labels_with_same_row_values() {
    for numbered in [false, true] {
        for (dx, scale) in [(0., 1.), (-200., 1.), (0., 0.8)] {
            let out = header_field_example(numbered, 2, (dx, scale), 0., 0., 15., true);
            assert!(
                out.lines()
                    .any(|l| l.starts_with('#') && l.contains("Record: AX 582")),
                "{out}"
            );
            assert!(
                out.lines()
                    .any(|l| l.starts_with('#') && l.contains("Issued: May 4")),
                "{out}"
            );
            assert_eq!(
                out.lines().filter(|l| l.starts_with('#')).count(),
                2,
                "{out}"
            );
        }
    }
}

#[test]
fn heading_fields_require_repetition_colons_close_baselines_and_styled_values() {
    for (rows, gap, dy, size, colon) in [
        (1, 0., 0., 15., true),  // An isolated pair is ambiguous.
        (2, 70., 0., 15., true), // A large gap is a separate column.
        (2, 0., 8., 15., true),  // A different baseline is not the same field.
        (2, 0., 0., 10., true),  // Body text is not promoted into a heading.
        (2, 0., 0., 15., false), // No label punctuation.
    ] {
        let out = header_field_example(true, rows, (0., 1.), gap, dy, size, colon);
        let joined = if colon {
            "Record: AX 582"
        } else {
            "Record AX 582"
        };
        assert!(
            !out.lines()
                .any(|l| l.starts_with('#') && l.contains(joined)),
            "{out}"
        );
    }
}

#[test]
fn small_repeated_sidebar_headings_need_independent_spatial_support() {
    let mut s = String::from("BT ");
    for i in 0..16 {
        let y = 740 - i * 24;
        s.push_str(&format!(
            "/F1 12 Tf 1 0 0 1 40 {y} Tm (Ordinary main body prose has a consistent style.) Tj "
        ));
    }
    for (y, t) in [
        (710, "Cellular Cycle"),
        (697, "and Replication"),
        (470, "Mitosis and"),
        (457, "Meiosis"),
    ] {
        s.push_str(&format!("/F2 9 Tf 1 0 0 1 430 {y} Tm ({t}) Tj "));
    }
    s.push_str("ET");
    let out = text(&styled(&s));
    assert!(out.contains("# Cellular Cycle and Replication"), "{out}");
    assert!(out.contains("# Mitosis and Meiosis"), "{out}");
    let rejected = s
        .replace("430 710", "100 710")
        .replace("430 697", "100 697")
        .replace("430 470", "100 470")
        .replace("430 457", "100 457");
    assert!(!text(&styled(&rejected)).lines().any(|l| l.starts_with('#')));
}

#[test]
fn sparse_rules_keep_spanning_headers_and_short_text_rows() {
    let mut s=String::from("40 720 m 360 720 l S 40 690 m 360 690 l S 40 570 m 360 570 l S BT /F1 10 Tf 1 0 0 1 50 700 Tm (One heading across both columns) Tj ");
    for (y, a, b) in [
        (670, "Small oak", "Quercus"),
        (640, "Tall pine", "Pinus"),
        (610, "Red birch", "Betula"),
        (580, "Ash", "Fraxinus"),
    ] {
        s.push_str(&format!(
            "1 0 0 1 50 {y} Tm ({a}) Tj 1 0 0 1 200 {y} Tm ({b}) Tj "
        ));
    }
    s.push_str("ET");
    let result = parse(&simple(&s));
    let md = result.to_markdown().text;
    assert!(
        md.contains("<td colspan=\"2\">One heading across both columns</td>"),
        "{md}"
    );
    let table = table_values(&result);
    assert_eq!(table[0].len(), 5);
    assert_eq!(table[0][4], vec!["Ash", "Fraxinus"]);
}

#[test]
fn sparse_worksheet_retains_blank_answer_column() {
    let mut s=String::from("40 720 m 360 720 l S 40 570 m 360 570 l S BT /F1 10 Tf 1 0 0 1 50 700 Tm (Sample) Tj 1 0 0 1 200 700 Tm (Observation) Tj ");
    for (y, t) in [(670, "A"), (640, "B"), (610, "C"), (580, "D")] {
        s.push_str(&format!("1 0 0 1 50 {y} Tm ({t}) Tj "));
    }
    s.push_str("ET");
    let table = table_values(&parse(&simple(&s)));
    assert_eq!(table.len(), 1);
    assert_eq!(table[0].len(), 5);
    assert_eq!(table[0][4], vec!["D", ""]);
}

#[test]
fn measurement_columns_recover_wider_headers_without_painted_borders() {
    let mut s=String::from("BT /F1 10 Tf 1 0 0 1 40 700 Tm (Identifier) Tj 1 0 0 1 104 700 Tm (Water) Tj 1 0 0 1 180 700 Tm (Mass) Tj 1 0 0 1 260 700 Tm (Time) Tj ");
    for (r, y) in [680, 660, 640].into_iter().enumerate() {
        for (x, t) in [
            (40, format!("{}", r + 1)),
            (104, format!("{} ml", r + 12)),
            (180, format!("{} kg", r + 3)),
            (260, format!("{} ms", r + 7)),
        ] {
            s.push_str(&format!("1 0 0 1 {x} {y} Tm ({t}) Tj "));
        }
    }
    s.push_str("ET");
    let table = table_values(&parse(&simple(&s)));
    assert_eq!(table.len(), 1);
    assert_eq!(table[0].len(), 4);
    assert_eq!(table[0][0], vec!["Identifier", "Water", "Mass", "Time"]);
    assert_eq!(table[0][3], vec!["3", "14 ml", "5 kg", "9 ms"]);
}

#[test]
fn offpage_text_is_excluded_without_clipping_overlapping_glyphs() {
    let s="BT /F1 10 Tf 1 0 0 1 -200 700 Tm (OutsideLeft) Tj 1 0 0 1 650 700 Tm (OutsideRight) Tj 1 0 0 1 40 900 Tm (OutsideTop) Tj 1 0 0 1 40 -30 Tm (OutsideBottom) Tj 1 0 0 1 40 700 Tm (Visible text) Tj 1 0 0 1 597 650 Tm (X) Tj ET";
    let result = parse(&simple(s));
    let out = result.to_markdown().text;
    assert!(!out.contains("Outside"), "{out}");
    assert!(out.contains("Visible text") && out.contains('X'));
    assert!(result
        .diagnostics
        .iter()
        .any(|d| d.code == "pdf_offpage_text"));
    let mut doc = Document::load_mem(&simple(s)).unwrap();
    for id in doc.get_pages().values() {
        doc.get_dictionary_mut(*id).unwrap().remove(b"MediaBox");
    }
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    assert!(text(&bytes).contains("OutsideLeft"));
}

#[test]
fn painted_spaces_across_gutters_do_not_interleave_prose_columns() {
    let mut s = String::from("BT /F1 10 Tf ");
    for r in 0..10 {
        let left = format!("Left paragraph line {r} has some words.");
        let gap = 300. - 40. - left.len() as f64 * 6. - 6.;
        s.push_str(&format!("0 Tw 1 0 0 1 40 {} Tm ({left}) Tj {gap} Tw ( ) Tj 0 Tw (Right paragraph line {r} has other words.) Tj ",700-r*15));
    }
    s.push_str("ET");
    let out = text(&simple(&s));
    assert!(
        out.find("Left paragraph line 9").unwrap() < out.find("Right paragraph line 0").unwrap(),
        "{out}"
    );
    for r in 0..10 {
        assert_eq!(out.matches(&format!("paragraph line {r}")).count(), 2);
    }
}

#[test]
fn contents_entries_and_indented_caption_continuations_are_not_headings() {
    let toc="BT /F2 18 Tf 1 0 0 1 40 750 Tm (Table of Contents) Tj /F2 12 Tf 1 0 0 1 40 700 Tm (Executive Summary) Tj /F1 12 Tf 1 0 0 1 400 700 Tm (4) Tj /F2 12 Tf 1 0 0 1 40 670 Tm (Methods and Materials) Tj /F1 12 Tf 1 0 0 1 400 670 Tm (9) Tj ET";
    let out = text(&styled(toc));
    assert_eq!(
        out.lines().filter(|l| l.starts_with('#')).count(),
        1,
        "{out}"
    );
    let caption="BT /F2 12 Tf 1 0 0 1 70 750 Tm (Table 1: Annual measurements of) Tj 1 0 0 1 40 735 Tm (Samples From Several Locations) Tj /F1 10 Tf 1 0 0 1 40 700 Tm (Ordinary body prose provides sufficient evidence for this page.) Tj 1 0 0 1 40 685 Tm (A second ordinary body line makes the font size unambiguous.) Tj ET";
    let out = text(&styled(caption));
    assert!(!out.lines().any(|l| l.starts_with('#')), "{out}");
}

#[test]
fn differently_sized_section_marker_keeps_all_wrapped_title_lines() {
    let s="BT /F2 24 Tf 1 0 0 1 40 750 Tm (III.) Tj /F2 22 Tf 1 0 0 1 40 720 Tm (Regulatory) Tj 1 0 0 1 40 690 Tm (requirements) Tj /F1 10 Tf 1 0 0 1 40 640 Tm (Ordinary body prose supplies enough text to establish its size.) Tj 1 0 0 1 40 625 Tm (Another explanatory line follows in the regular body font.) Tj ET";
    let out = text(&styled(s));
    assert!(out.contains("III. Regulatory requirements"), "{out}");
    assert_eq!(
        out.lines().filter(|l| l.starts_with('#')).count(),
        1,
        "{out}"
    );
}

#[test]
fn body_size_emphasized_question_is_a_heading() {
    let s="BT /F2 10 Tf 1 0 0 1 40 750 Tm (WHY DOES THIS HAPPEN?) Tj /F1 10 Tf 1 0 0 1 40 720 Tm (This paragraph provides substantial evidence of ordinary body prose.) Tj 1 0 0 1 40 705 Tm (A second sentence in the same regular font supplies context.) Tj ET";
    assert!(text(&styled(s)).starts_with("# WHY DOES THIS HAPPEN?"));
}

#[test]
fn separated_list_markers_stay_with_items_and_wrapped_emphasis_is_not_a_heading() {
    let mut s = String::from("BT ");
    for (y, t) in [
        (710, "First ordinary item"),
        (690, "Second ordinary item"),
        (670, "Third ordinary item"),
    ] {
        s.push_str(&format!(
            "/F1 8 Tf 1 0 0 1 40 {y} Tm (-) Tj /F1 10 Tf 1 0 0 1 64 {y} Tm ({t}) Tj "
        ));
    }
    s.push_str("/F1 8 Tf 1 0 0 1 40 630 Tm (-) Tj /F2 10 Tf 1 0 0 1 64 630 Tm (An emphasized list introduction with) Tj 1 0 0 1 64 615 Tm (a continued emphasized explanation) Tj /F1 10 Tf 1 0 0 1 64 600 Tm (Ordinary supporting prose continues the list item.) Tj ET");
    let out = text(&styled(&s));
    for t in [
        "First ordinary item",
        "Second ordinary item",
        "Third ordinary item",
    ] {
        assert!(out.contains(&format!("\\- {t}")), "{out}");
    }
    assert!(!out.lines().any(|l| l.starts_with('#')), "{out}");
}

#[test]
fn isolated_split_running_header_stays_a_single_plain_row() {
    for (left, right, scale) in [
        ("NORTHERN INSTITUTE", "ZX204", 1.),
        ("WESTERN ACADEMY", "AB92", 0.8),
    ] {
        let mut s = format!("q {scale} 0 0 {scale} 0 0 cm BT /F1 10.5 Tf 1 0 0 1 40 750 Tm ({left}) Tj /F1 10 Tf 1 0 0 1 460 750 Tm ({right}) Tj /F2 18 Tf 1 0 0 1 160 710 Tm (A Display Title) Tj ");
        for i in 0..10 {
            s.push_str(&format!("/F1 10 Tf 1 0 0 1 40 {} Tm (Ordinary body prose supplies enough evidence for the usual font.) Tj ", 650-i*15));
        }
        s.push_str("ET Q");
        let out = text(&styled(&s));
        assert!(out.starts_with(&format!("{left} {right}")), "{out}");
        assert!(out.contains("# A Display Title"), "{out}");
        assert_eq!(
            out.lines().filter(|l| l.starts_with('#')).count(),
            1,
            "{out}"
        );
    }
}

#[test]
fn wrapped_captions_and_long_bold_paragraphs_stay_plain_but_short_headings_survive() {
    let mut s = String::from("BT /F2 10 Tf 1 0 0 1 40 750 Tm (Figure 4: An authored caption that) Tj 1 0 0 1 40 735 Tm (continues over a second line and) Tj 1 0 0 1 40 720 Tm (ends on the third line.) Tj ");
    for (i, t) in [
        "This is a bold paragraph with sufficient continuous prose",
        "to distinguish it from a brief heading or display title",
        "while keeping every word in the output of this example",
        "and retaining the usual order of the paragraph lines.",
    ]
    .iter()
    .enumerate()
    {
        s.push_str(&format!("1 0 0 1 40 {} Tm ({t}) Tj ", 660 - i * 15));
    }
    s.push_str("/F2 16 Tf 1 0 0 1 40 550 Tm (Real Section) Tj /F1 10 Tf 1 0 0 1 40 520 Tm (Ordinary body text explains the section in a familiar font.) Tj 1 0 0 1 40 505 Tm (Another ordinary sentence completes this independent example.) Tj ET");
    let out = text(&styled(&s));
    assert!(out.contains("# Real Section"), "{out}");
    assert_eq!(
        out.lines().filter(|l| l.starts_with('#')).count(),
        1,
        "{out}"
    );
    assert!(out.contains("ends on the third line"));
    assert!(out.contains("usual order of the paragraph lines"));
}

#[test]
fn superscript_reference_does_not_interrupt_a_wrapped_table_caption() {
    let s = "BT /F2 10 Tf 1 0 0 1 40 700 Tm (Table: A long illustrative caption) Tj /F2 6 Tf 1 0 0 1 210 691 Tm (7) Tj /F2 10 Tf 1 0 0 1 40 686 Tm (with a second descriptive line) Tj /F1 10 Tf 1 0 0 1 40 630 Tm (Ordinary prose gives this page enough body font evidence.) Tj 1 0 0 1 40 615 Tm (Another sentence ensures the caption has a fair comparison.) Tj ET";
    let out = text(&styled(s));
    assert!(
        out.contains("Table: A long illustrative caption with a second descriptive line"),
        "{out}"
    );
    assert!(!out.lines().any(|l| l.starts_with('#')), "{out}");
    assert!(out.contains('7'));
}

#[test]
fn parallel_headings_need_data_evidence_to_become_chart_labels() {
    let mut base = String::from("BT ");
    for (x, t) in [(40, "ALPHA"), (220, "BETA"), (400, "GAMMA")] {
        base.push_str(&format!("/F2 10 Tf 1 0 0 1 {x} 650 Tm ({t}) Tj "));
        for i in 0..5 {
            base.push_str(&format!(
                "/F1 10 Tf 1 0 0 1 {x} {} Tm (Ordinary column prose.) Tj ",
                620 - i * 15
            ));
        }
    }
    let out = text(&styled(&(base.clone() + "ET")));
    for t in ["ALPHA", "BETA", "GAMMA"] {
        assert!(out.contains(&format!("# {t}")), "{out}");
    }
    base.push_str("/F1 8 Tf 1 0 0 1 40 680 Tm (25%) Tj ET");
    let out = text(&styled(&base));
    assert!(!out.lines().any(|l| l.starts_with('#')), "{out}");
    for t in ["ALPHA", "BETA", "GAMMA"] {
        assert!(out.contains(t));
    }
}

#[test]
fn sparse_photo_page_running_head_requires_image_and_body_size_evidence() {
    for (image, size, heading) in [(true, 10, false), (true, 18, true), (false, 10, true)] {
        let mut doc = Document::with_version("1.5");
        let im = doc.add_object(Stream::new(dictionary!{"Type"=>"XObject","Subtype"=>"Image","Width"=>1,"Height"=>1,"ColorSpace"=>"DeviceGray","BitsPerComponent"=>8}, vec![0]));
        let font =
            doc.add_object(dictionary! {"Type"=>"Font","Subtype"=>"Type1","BaseFont"=>"Courier"});
        let content = format!("{} BT /F1 {size} Tf 1 0 0 1 200 760 Tm (Coastal Views) Tj /F1 10 Tf 1 0 0 1 70 110 Tm (Figure 3. An illustrative scene beside the water,) Tj 1 0 0 1 70 96 Tm (with a second descriptive caption line beneath it.) Tj 1 0 0 1 70 82 Tm (Photographed during the early spring season.) Tj ET", if image { "q 520 0 0 560 40 130 cm /Im Do Q" } else { "" });
        let bytes = finish(
            doc,
            dictionary! {"Font"=>dictionary!{"F1"=>font},"XObject"=>dictionary!{"Im"=>im}}.into(),
            vec![content.into_bytes()],
        );
        let out = text(&bytes);
        assert_eq!(out.starts_with("# Coastal Views"), heading, "{out}");
        assert!(out.contains("Coastal Views"));
    }
}

#[test]
fn vertical_chart_interval_labels_need_caption_and_repeated_alignment() {
    for caption in [true, false] {
        let mut s = String::from("BT /F1 10 Tf 1 0 0 1 40 750 Tm (Ordinary explanatory prose establishes the body font.) Tj 1 0 0 1 40 735 Tm (Another descriptive sentence continues the introduction.) Tj ");
        if caption {
            s.push_str("/F2 10 Tf 1 0 0 1 180 660 Tm (Figure 2: Distribution by group) Tj ");
        }
        s.push_str("/F2 10 Tf 1 0 0 1 40 620 Tm (ABOVE 80) Tj ");
        for (y, t) in [(580, "60-79"), (540, "40-59"), (500, "20-39")] {
            s.push_str(&format!("1 0 0 1 58 {y} Tm ({t}) Tj "));
        }
        s.push_str("ET");
        let out = text(&styled(&s));
        assert_eq!(out.contains("# ABOVE 80"), !caption, "{out}");
        assert!(out.contains("ABOVE 80"));
    }
}

#[test]
fn lettered_wrapped_paragraph_lead_requires_mixed_style_continuation() {
    for mixed in [true, false] {
        let s = format!("BT /F1 10 Tf 1 0 0 1 40 750 Tm (Ordinary text supplies sufficient evidence of the body font.) Tj 1 0 0 1 40 735 Tm (Another sentence adds context before the following item.) Tj /F2 10 Tf 1 0 0 1 40 680 Tm (d.) Tj 1 0 0 1 65 680 Tm (Storage and handling of) Tj /{} 10 Tf 1 0 0 1 65 665 Tm (materials.) Tj /F1 10 Tf ( Ordinary prose starts on this same line.) Tj ET", if mixed {"F2"} else {"F1"});
        let out = text(&styled(&s));
        assert_eq!(out.contains("# Storage and handling of"), !mixed, "{out}");
        assert!(out.contains("Storage and handling of"));
        assert!(out.contains("materials"));
    }
}

#[test]
fn continued_outline_joins_page_references_without_matching_topic_words() {
    for increasing in [true, false] {
        let mut s = String::from("BT ");
        for group in 0..2 {
            let y = 750 - group * 220;
            s.push_str(&format!(
                "/F1 12 Tf 1 0 0 1 40 {y} Tm (Volume {}: An Illustrative Collection) Tj ",
                group + 1
            ));
            for i in 0..4 {
                let page = if increasing {
                    12 + group * 30 + i * 3
                } else {
                    85 - group * 30 - i * 3
                };
                let label = [
                    "An introduction",
                    "A longer explanation of the examples",
                    "Important background",
                    "Further exercises and applications",
                ][i as usize];
                s.push_str(&format!("/F1 10 Tf 1 0 0 1 40 {} Tm (Unit {}.{}: {label}) Tj 1 0 0 1 550 {} Tm ({page}) Tj ", y-30-i*32, group+1, i+1, y-30-i*32));
            }
        }
        s.push_str("ET");
        let out = text(&styled(&s));
        if increasing {
            assert!(!out.lines().any(|l| l.starts_with('#')), "{out}");
            assert!(out.contains("Unit 1.1: An introduction 12"), "{out}");
            assert!(
                out.contains("Unit 2.4: Further exercises and applications 51"),
                "{out}"
            );
        } else {
            assert!(out.contains("# Volume"), "{out}");
        }
    }
}

#[test]
fn standard_font_metrics_recover_spaces_and_column_geometry() {
    let mut doc = Document::with_version("1.5");
    let font = doc.add_object(dictionary! {"Type"=>"Font","Subtype"=>"Type1","BaseFont"=>"Times-Roman","Encoding"=>"WinAnsiEncoding"});
    let bytes = finish(doc, dictionary! {"Font"=>dictionary!{"F1"=>font}}.into(), vec![b"BT /F1 10 Tf 1 0 0 1 40 700 Tm (ill) Tj 1 0 0 1 50.84 700 Tm (wide) Tj 1 0 0 1 320 700 Tm (Right first) Tj 1 0 0 1 40 685 Tm (Left second) Tj 1 0 0 1 320 685 Tm (Right second) Tj ET".to_vec()]);
    let out = text(&bytes);
    assert!(out.contains("ill wide"), "{out}");
    assert!(
        out.find("Left second").unwrap() < out.find("Right first").unwrap(),
        "{out}"
    );
}

#[test]
fn explicit_widths_override_standard_font_fallback() {
    let mut doc = Document::with_version("1.5");
    let font = doc.add_object(dictionary! {"Type"=>"Font","Subtype"=>"Type1","BaseFont"=>"Times-Roman","FirstChar"=>0,"Widths"=>vec![Object::Integer(1000);256]});
    let bytes = finish(
        doc,
        dictionary! {"Font"=>dictionary!{"F1"=>font}}.into(),
        vec![b"BT /F1 10 Tf 1 0 0 1 40 700 Tm (ill) Tj 1 0 0 1 72.5 700 Tm (wide) Tj ET".to_vec()],
    );
    assert!(text(&bytes).contains("ill wide"));
}

#[test]
fn regular_outline_marker_keeps_bold_title_and_repetitive_noise_stays_plain() {
    let bytes = styled(
        r"BT /F1 12 Tf 40 700 Td (\(7\) ) Tj /F2 12 Tf (OVERVIEW) Tj ET BT /F1 12 Tf 40 675 Td (Ordinary body text explains the section in a normal face.) Tj 0 -16 Td (Another body line provides evidence for the usual style.) Tj ET BT /F2 20 Tf 40 750 Td (I1 111111111111 llll 11111111111111111) Tj ET",
    );
    let result = parse(&bytes);
    let out = result.to_markdown().text;
    assert!(result.document.blocks.iter().any(|b| matches!(&b.content, BlockContent::Heading { inlines, .. } if inlines.iter().any(|i| matches!(i, Inline::Text { text } if text.contains("OVERVIEW"))))), "{out}");
    assert!(
        !out.lines()
            .any(|l| l.starts_with('#') && l.contains("111111")),
        "{out}"
    );
    assert!(
        out.contains("11111111111111111"),
        "noise must remain available: {out}"
    );
}

#[test]
fn parallel_numbered_sections_are_not_numeric_data_labels() {
    let out = text(&styled(
        r"BT /F1 12 Tf 40 700 Td (\(1\)) Tj 1 0 0 1 320 700 Tm (\(2\)) Tj /F2 12 Tf 1 0 0 1 100 700 Tm (Overview) Tj 1 0 0 1 380 700 Tm (Summary) Tj /F1 12 Tf 1 0 0 1 40 675 Tm (Ordinary text is written in this part.) Tj 0 -16 Td (More text follows the previous line.) Tj 1 0 0 1 320 675 Tm (Different text in the second column.) Tj 0 -16 Td (A second body line in this column.) Tj ET",
    ));
    for title in ["Overview", "Summary"] {
        assert!(
            out.lines().any(|l| l.starts_with('#') && l.contains(title)),
            "{out}"
        );
    }
}

#[test]
fn double_borders_do_not_create_empty_rows_or_columns_and_keep_spans() {
    let mut s = String::new();
    for (y, left) in [
        (720., 39.25),
        (680., 160.),
        (640., 39.25),
        (600., 39.25),
        (560., 39.25),
    ] {
        for offset in [-0.75, 0.75] {
            s.push_str(&format!(
                "{left} {} m 400.75 {} l S ",
                y + offset,
                y + offset
            ));
        }
    }
    for (x, top) in [
        (40., 720.75),
        (160., 720.75),
        (280., 680.75),
        (400., 720.75),
    ] {
        for offset in [-0.75, 0.75] {
            s.push_str(&format!(
                "{} 559.25 m {} {top} l S ",
                x + offset,
                x + offset
            ));
        }
    }
    s.push_str("BT /F1 10 Tf ");
    for (x, y, t) in [
        (50, 690, "Group"),
        (170, 700, "Shared heading"),
        (170, 660, "A"),
        (290, 660, "B"),
        (50, 620, "First"),
        (170, 620, "10"),
        (290, 620, "20"),
        (50, 580, "Second"),
        (170, 580, "30"),
        (290, 580, "40"),
    ] {
        s.push_str(&format!("1 0 0 1 {x} {y} Tm ({t}) Tj "));
    }
    s.push_str("ET");
    let result = parse(&simple(&s));
    let rows = result
        .document
        .blocks
        .iter()
        .find_map(|b| {
            if let BlockContent::Table { rows, .. } = &b.content {
                Some(rows)
            } else {
                None
            }
        })
        .expect("double grid");
    assert_eq!(rows.len(), 4);
    assert_eq!((rows[0][0].row_span, rows[0][0].col_span), (2, 1));
    assert_eq!((rows[0][1].row_span, rows[0][1].col_span), (1, 2));
    assert_eq!(table_values(&result)[0][3], vec!["Second", "30", "40"]);
}

#[test]
fn narrow_real_blank_rows_survive_decorated_border_repair() {
    let s="40 600 360 120 re S 220 600 m 220 720 l S 40 680 m 400 680 l S 40 676 m 400 676 l S 40 640 m 400 640 l S BT /F1 10 Tf 1 0 0 1 50 700 Tm (Name) Tj 1 0 0 1 230 700 Tm (Value) Tj 1 0 0 1 50 656 Tm (First) Tj 1 0 0 1 230 656 Tm (10) Tj 1 0 0 1 50 620 Tm (Second) Tj 1 0 0 1 230 620 Tm (20) Tj ET";
    assert_eq!(
        table_values(&parse(&simple(s)))[0],
        vec![
            vec!["Name", "Value"],
            vec!["", ""],
            vec!["First", "10"],
            vec!["Second", "20"]
        ]
    );
}

fn boxed_table_example(title: &str, note: &str) -> Vec<u8> {
    let s=format!("40 540 360 180 re S 220 580 m 220 680 l S 40 680 m 400 680 l S 40 650 m 400 650 l S 40 615 m 400 615 l S 40 580 m 400 580 l S BT /F1 10 Tf 1 0 0 1 50 700 Tm ({title}) Tj 1 0 0 1 50 660 Tm (Category) Tj 1 0 0 1 230 660 Tm (Count) Tj 1 0 0 1 50 630 Tm (First) Tj 1 0 0 1 230 630 Tm (10) Tj 1 0 0 1 50 595 Tm (Second) Tj 1 0 0 1 230 595 Tm (20) Tj 1 0 0 1 50 555 Tm ({note}) Tj ET");
    simple(&s)
}

#[test]
fn boxed_captions_and_notes_stay_in_text_outside_the_data_grid() {
    for title in [
        "Table 8. Regional totals",
        "Exhibit IV: Regional totals",
        "Chart 23: Regional totals",
        "Exhibit B.4: Regional totals",
    ] {
        for note in [
            "Source: Independent records.",
            "Note: Counts include late entries.",
            "Exhibit reads: Ten entries are first.",
        ] {
            let result = parse(&boxed_table_example(title, note));
            assert_eq!(
                table_values(&result),
                vec![vec![
                    vec!["Category", "Count"],
                    vec!["First", "10"],
                    vec!["Second", "20"]
                ]]
            );
            let md = result.to_markdown().text.replace("\\.", ".");
            for t in [title, note] {
                assert_eq!(md.matches(t).count(), 1, "{md}");
            }
            assert!(
                md.find(title).unwrap() < md.find("Category").unwrap(),
                "{md}"
            );
            assert!(md.find(note).unwrap() > md.find("Second").unwrap(), "{md}");
        }
    }
}

#[test]
fn unlabeled_spanning_titles_and_full_width_data_are_not_removed() {
    let result = parse(&boxed_table_example("Regional totals", "Combined result"));
    let values = table_values(&result);
    assert_eq!(values[0].len(), 5);
    assert_eq!(values[0][0], vec!["Regional totals"]);
    assert_eq!(values[0][4], vec!["Combined result"]);
}

#[test]
fn close_rules_with_native_ink_define_a_real_row() {
    let s="40 600 360 120 re S 220 600 m 220 720 l S 40 680 m 400 680 l S 40 678.5 m 400 678.5 l S 40 640 m 400 640 l S BT /F1 10 Tf 1 0 0 1 50 700 Tm (Name) Tj 1 0 0 1 230 700 Tm (Value) Tj 1 0 0 1 50 656 Tm (First) Tj 1 0 0 1 230 656 Tm (10) Tj 1 0 0 1 50 620 Tm (Second) Tj 1 0 0 1 230 620 Tm (20) Tj /F1 1 Tf 1 0 0 1 50 679 Tm (Tiny) Tj 1 0 0 1 230 679 Tm (1) Tj ET";
    assert_eq!(
        table_values(&parse(&simple(s)))[0],
        vec![
            vec!["Name", "Value"],
            vec!["Tiny", "1"],
            vec!["First", "10"],
            vec!["Second", "20"]
        ]
    );
}

#[test]
fn painted_dividers_separate_tightly_padded_cells() {
    let borders =
        "40 600 360 120 re S 220 600 m 220 720 l S 40 680 m 400 680 l S 40 640 m 400 640 l S";
    let content = format!("{borders} BT /F1 10 Tf 1 0 0 1 190 700 Tm (Left) Tj 1 0 0 1 222 700 Tm (Right) Tj 1 0 0 1 190 660 Tm (ABCD) Tj 1 0 0 1 222 660 Tm (EFGH) Tj 1 0 0 1 190 620 Tm (IJKL) Tj 1 0 0 1 222 620 Tm (MNOP) Tj ET");
    assert_eq!(
        table_values(&parse(&simple(&content)))[0],
        vec![
            vec!["Left", "Right"],
            vec!["ABCD", "EFGH"],
            vec!["IJKL", "MNOP"]
        ]
    );
    let crossing = content.replace("222", "214");
    let result = parse(&simple(&crossing));
    // Adjacent runs forming one unspaced word must not be split at a rule.
    assert!(!table_values(&result)
        .iter()
        .flatten()
        .any(|r| r == &vec!["ABCD", "EFGH"]));
}

#[test]
fn boxed_multiline_notes_are_not_redetected_as_tables() {
    let content = "40 480 360 240 re S 220 600 m 220 680 l S 40 680 m 400 680 l S 40 650 m 400 650 l S 40 625 m 400 625 l S 40 600 m 400 600 l S BT /F1 10 Tf 1 0 0 1 50 700 Tm (Exhibit B.4: Counts) Tj 1 0 0 1 50 660 Tm (Category) Tj 1 0 0 1 230 660 Tm (Count) Tj 1 0 0 1 50 635 Tm (First) Tj 1 0 0 1 230 635 Tm (10) Tj 1 0 0 1 50 610 Tm (Second) Tj 1 0 0 1 230 610 Tm (20) Tj 1 0 0 1 50 585 Tm (Note: Records include all regions.) Tj 0 -20 Td (a. Northern entries were revised.) Tj 0 -20 Td (b. Southern entries arrived late.) Tj 0 -20 Td (c. Western entries were checked.) Tj 0 -20 Td (Source: Regional records.) Tj ET";
    let result = parse(&simple(content));
    let values = table_values(&result);
    assert_eq!(values.len(), 1);
    assert_eq!(values[0].len(), 3);
    let md = result.to_markdown().text;
    for phrase in [
        "Northern entries",
        "Southern entries",
        "Western entries",
        "Source:",
    ] {
        assert_eq!(md.matches(phrase).count(), 1, "{md}");
    }
}

#[test]
fn wide_double_outer_frame_does_not_create_blank_columns() {
    let content = "40 600 360 120 re S 42.4 602.4 355.2 115.2 re S 220 602.4 m 220 717.6 l S 42.4 680 m 397.6 680 l S 42.4 640 m 397.6 640 l S BT /F1 12 Tf 1 0 0 1 50 700 Tm (Name) Tj 1 0 0 1 230 700 Tm (Value) Tj 1 0 0 1 50 660 Tm (First) Tj 1 0 0 1 230 660 Tm (10) Tj 1 0 0 1 50 620 Tm (Second) Tj 1 0 0 1 230 620 Tm (20) Tj ET";
    assert_eq!(
        table_values(&parse(&simple(content))),
        vec![vec![
            vec!["Name", "Value"],
            vec!["First", "10"],
            vec!["Second", "20"]
        ]]
    );
}

#[test]
fn cell_edge_font_tolerance_preserves_whole_words() {
    let s="40 600 360 120 re S 220 600 m 220 720 l S 40 680 m 400 680 l S 40 640 m 400 640 l S BT /F1 10 Tf 1 0 0 1 195 700 Tm (ABCDE) Tj 1 0 0 1 250 700 Tm (Header) Tj 1 0 0 1 195 660 Tm (FGHIJ) Tj 1 0 0 1 250 660 Tm (10) Tj 1 0 0 1 195 620 Tm (KLMNO) Tj 1 0 0 1 250 620 Tm (20) Tj ET";
    assert_eq!(
        table_values(&parse(&simple(s)))[0],
        vec![
            vec!["ABCDE", "Header"],
            vec!["FGHIJ", "10"],
            vec!["KLMNO", "20"]
        ]
    );
}

#[test]
fn boxed_multilevel_header_is_not_a_separate_table_from_unruled_data() {
    let s="40 660 360 60 re S 160 660 m 160 720 l S 280 660 m 280 690 l S 160 690 m 400 690 l S BT /F1 10 Tf 1 0 0 1 50 680 Tm (Region) Tj 1 0 0 1 170 700 Tm (Annual totals) Tj 1 0 0 1 170 670 Tm (2024) Tj 1 0 0 1 290 670 Tm (2025) Tj 1 0 0 1 50 648 Tm (North) Tj 1 0 0 1 170 648 Tm (10) Tj 1 0 0 1 290 648 Tm (20) Tj 1 0 0 1 50 632 Tm (South) Tj 1 0 0 1 170 632 Tm (30) Tj 1 0 0 1 290 632 Tm (40) Tj ET";
    let result = parse(&simple(s));
    for table in table_values(&result) {
        let text = table.into_iter().flatten().collect::<Vec<_>>().join(" ");
        assert!(
            !text.contains("Annual totals") || text.contains("North"),
            "isolated header: {text}"
        );
    }
    assert!(result.to_markdown().text.contains("Annual totals"));
}

#[test]
fn repeated_numeric_tables_keep_headers_rows_and_centered_values() {
    let mut s = String::new();
    // Same-width tables separated by prose must remain separate.
    for offset in [0, 160] {
        for y in [730, 710, 660] {
            s += &format!("40 {} m 500 {} l S ", y - offset, y - offset);
        }
        for (x, t) in [
            (45, "Model"),
            (150, "Version"),
            (245, "Rate A"),
            (335, "Rate B"),
            (425, "Rate C"),
        ] {
            s += &format!("BT /F1 10 Tf 1 0 0 1 {x} {} Tm ({t}) Tj ET ", 718 - offset);
        }
        for (r, label) in ["Short", "Longer"].iter().enumerate() {
            let y = 695 - offset - r * 18;
            for (x, t) in [
                (45, *label),
                (170, "O"),
                (265, "72.4"),
                (355, "81.1"),
                (445, "95.6"),
            ] {
                s += &format!("BT /F1 10 Tf 1 0 0 1 {x} {y} Tm ({t}) Tj ET ");
            }
        }
    }
    s += "BT /F1 10 Tf 1 0 0 1 45 630 Tm (An ordinary paragraph separates the tables.) Tj ET";
    let result = parse(&simple(&s));
    let t = table_values(&result);
    assert_eq!(t.len(), 2);
    for rows in t {
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].len(), 5);
        assert_eq!(rows[1][2], "72.4");
    }
    assert!(result.to_markdown().text.contains("An ordinary paragraph"));
}

#[test]
fn internal_dividers_recover_open_paragraph_grid_and_external_header() {
    let mut s = String::new();
    for y in [700, 640, 580] {
        s += &format!("40 {y} m 520 {y} l S ");
    }
    for (x, bottom) in [(160, 520), (280, 510), (400, 510)] {
        s += &format!("{x} 700 m {x} {bottom} l S ");
    }
    for (x, t) in [(180, "North"), (300, "South"), (420, "East")] {
        s += &format!("BT /F1 10 Tf 1 0 0 1 {x} 712 Tm ({t}) Tj ET ");
    }
    for (row, y) in [680, 620, 560].iter().enumerate() {
        for x in [60, 180, 300, 420] {
            s += &format!(
                "BT /F1 10 Tf 1 0 0 1 {x} {y} Tm (Row {}) Tj 0 -14 Td (details) Tj ET ",
                row + 1
            );
        }
    }
    let result = parse(&simple(&s));
    let tables = table_values(&result);
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].len(), 4);
    assert_eq!(tables[0][0], vec!["", "North", "South", "East"]);
    assert!(tables[0][3]
        .iter()
        .all(|c| c.split_whitespace().collect::<Vec<_>>().join(" ") == "Row 3 details"));
    // Prose crossing a divider is incompatible with a repaired grid.
    s += " BT /F1 10 Tf 1 0 0 1 145 550 Tm (crossing) Tj ET";
    let tables = table_values(&parse(&simple(&s)));
    assert!(!tables
        .iter()
        .any(|t| t.len() == 4 && t[0] == vec!["", "North", "South", "East"]));
}

#[test]
fn a_painted_group_divider_keeps_numeric_whitespace_subcolumns() {
    let mut s = String::new();
    for y in [730, 700, 670, 640, 610] {
        s += &format!("40 {y} m 410 {y} l S ");
    }
    s += "160 730 m 160 610 l S ";
    for (r, label) in ["Kind", "Alpha", "Beta", "Gamma"].iter().enumerate() {
        let y = 710 - r * 30;
        for (x, t) in [
            (50, *label),
            (200, if r == 0 { "Before" } else { "12" }),
            (310, if r == 0 { "After" } else { "34" }),
        ] {
            s += &format!("BT /F1 10 Tf 1 0 0 1 {x} {y} Tm ({t}) Tj ET ");
        }
    }
    let values = table_values(&parse(&simple(&s)));
    assert_eq!(values.len(), 1);
    assert_eq!(values[0][0], vec!["Kind", "Before", "After"]);
    assert_eq!(values[0][3], vec!["Gamma", "12", "34"]);
}

#[test]
fn borderless_catalogue_preserves_tables_annotations_and_reading_order() {
    // An original generated catalogue, safe to redistribute. Paint by column
    // rather than row so extraction must reconstruct the geometric reading order.
    let rows = [
        (
            740,
            vec![
                (40, "Copper spool"),
                (220, "$18.75"),
                (320, "includes assembly"),
            ],
        ),
        (
            720,
            vec![
                (40, "Steel bracket"),
                (220, "$4.25"),
                (320, "includes fittings"),
            ],
        ),
        (640, vec![(40, "Room hire"), (360, "Morning")]),
        (
            620,
            vec![(40, "Horizon"), (220, "EUR 180"), (360, "EUR 120")],
        ),
        (600, vec![(40, "Meadow"), (220, "EUR 90"), (360, "EUR 60")]),
        (
            520,
            vec![
                (40, "Storage"),
                (200, "Capacity"),
                (330, "Personal"),
                (450, "Business"),
            ],
        ),
        (
            500,
            vec![
                (40, "Large crate"),
                (200, "~40 units"),
                (330, "$145"),
                (450, "$230"),
            ],
        ),
        (
            480,
            vec![
                (40, "Small crate"),
                (200, "~20 units"),
                (330, "$85"),
                (450, "$150"),
            ],
        ),
    ];
    let mut positions: Vec<_> = rows
        .iter()
        .flat_map(|(y, cells)| cells.iter().map(move |(x, text)| (*x, *y, *text)))
        .collect();
    positions.extend([
        (40, 780, "Catalogue v3.14 + extras."),
        (40, 680, "Orders require advance notice by 13.30."),
        (40, 440, "Adjustment +15%. Payment is due on receipt."),
    ]);
    positions.sort_by_key(|(x, y, _)| (*x, *y));
    let mut content = String::from("BT /F1 10 Tf ");
    for (x, y, text) in positions {
        content += &format!("1 0 0 1 {x} {y} Tm ({text}) Tj ");
    }
    content += "ET";
    let result = parse(&simple(&content));
    assert_eq!(
        table_values(&result),
        vec![
            vec![
                vec!["Copper spool", "$18.75 includes assembly"],
                vec!["Steel bracket", "$4.25 includes fittings"]
            ],
            vec![
                vec!["Room hire", "", "Morning"],
                vec!["Horizon", "EUR 180", "EUR 120"],
                vec!["Meadow", "EUR 90", "EUR 60"]
            ],
            vec![
                vec!["Storage", "Capacity", "Personal", "Business"],
                vec!["Large crate", "~40 units", "$145", "$230"],
                vec!["Small crate", "~20 units", "$85", "$150"]
            ],
        ]
    );
    let md = result.to_markdown().text;
    assert!(!md.contains('\\'), "unnecessary punctuation escapes: {md}");
    for pair in [
        "Catalogue",
        "Copper spool",
        "Orders require",
        "Room hire",
        "Storage",
        "Adjustment",
    ]
    .windows(2)
    {
        assert!(
            md.find(pair[0]).unwrap() < md.find(pair[1]).unwrap(),
            "{md}"
        );
    }
    for block in &result.document.blocks {
        if let BlockContent::Table { rows, header_rows } = &block.content {
            assert_eq!(*header_rows, 0);
            for (r, row) in rows.iter().enumerate() {
                for (c, cell) in row.iter().enumerate() {
                    assert_eq!(cell.source.row, Some(r as u32 + 1));
                    assert_eq!(cell.source.cell, Some(c as u32 + 1));
                }
            }
        }
    }
}

#[test]
fn borderless_fields_are_scale_and_paint_order_independent() {
    for scale in [0.65, 1.0, 1.3] {
        for reversed in [false, true] {
            let mut ops = Vec::new();
            for (r, label) in ["Copper coil", "Valve", "Pump"].iter().enumerate() {
                let y = 520 - r * 20;
                ops.push(format!("1 0 0 1 40 {y} Tm ({label}) Tj "));
                let price = ["$9.50", "$120.00", "$7.00"][r];
                let x = 280.0 - price.len() as f64 * 6.0;
                ops.push(format!("1 0 0 1 {x} {y} Tm ({price}) Tj "));
                ops.push(format!("1 0 0 1 320 {y} Tm (\\(includes fitting\\)) Tj "));
            }
            if reversed {
                ops.reverse();
            }
            let content = format!(
                "q {scale} 0 0 {scale} 5 7 cm BT /F1 10 Tf {} ET Q",
                ops.join("")
            );
            let tables = table_values(&parse(&simple(&content)));
            assert_eq!(tables.len(), 1, "{tables:?}");
            assert_eq!(tables[0][1], ["Valve", "$120.00 (includes fitting)"]);
            assert_eq!(tables[0][2][0], "Pump");
        }
    }
}

#[test]
fn borderless_fields_support_units_headers_and_separate_sections() {
    let mut content = String::from("BT /F1 10 Tf ");
    for (x, y, t) in [
        (40, 700, "Material"),
        (200, 700, "Mass"),
        (40, 680, "Copper"),
        (200, 680, "12 kg"),
        (40, 660, "Steel"),
        (200, 660, "25 kg"),
        (40, 610, "A prose paragraph separates these tables."),
        (40, 550, "North"),
        (200, 550, "17 m/s"),
        (40, 530, "South"),
        (200, 530, "23 m/s"),
    ] {
        content += &format!("1 0 0 1 {x} {y} Tm ({t}) Tj ");
    }
    content += "ET";
    let result = parse(&simple(&content));
    let tables = table_values(&result);
    assert_eq!(tables.len(), 2);
    assert_eq!(
        tables[0],
        vec![
            vec!["Material", "Mass"],
            vec!["Copper", "12 kg"],
            vec!["Steel", "25 kg"]
        ]
    );
    assert_eq!(tables[1][1], vec!["South", "23 m/s"]);
    let md = result.to_markdown().text;
    assert!(md.find("Steel").unwrap() < md.find("A prose").unwrap());
    assert!(md.find("A prose").unwrap() < md.find("North").unwrap());
}

#[test]
fn borderless_field_candidates_reject_prose_isolated_values_and_crossing_gutters() {
    for values in [
        [
            "12 people arrived",
            "15 people departed",
            "18 people stayed",
        ],
        ["100", "200", "300"],
    ] {
        let mut content = String::from("BT /F1 10 Tf ");
        for (r, value) in values.iter().enumerate() {
            let y = 700 - r * 20;
            content += &format!(
                "1 0 0 1 40 {y} Tm (Left paragraph {r}) Tj 1 0 0 1 320 {y} Tm ({value}) Tj "
            );
        }
        content += "ET";
        let result = parse(&simple(&content));
        assert!(table_values(&result).is_empty());
        let md = result.to_markdown().text;
        assert!(
            md.find("Left paragraph 2").unwrap() < md.find(values[0]).unwrap(),
            "{md}"
        );
    }
    assert!(table_values(&parse(&simple(
        "BT /F1 10 Tf 40 700 Td (Widget) Tj 1 0 0 1 200 700 Tm ($10) Tj ET"
    )))
    .is_empty());
    // Labels and values have overlapping column envelopes despite each row
    // having a local gap; no global separator can establish the same schema.
    let crossing="BT /F1 10 Tf 40 700 Td (Short) Tj 1 0 0 1 120 700 Tm ($12) Tj 1 0 0 1 40 680 Tm (A much longer label) Tj 1 0 0 1 200 680 Tm ($123) Tj ET";
    assert!(table_values(&parse(&simple(crossing))).is_empty());
}

#[test]
fn borderless_fields_obey_cell_limits() {
    let bytes=simple("BT /F1 10 Tf 40 700 Td (Alpha) Tj 1 0 0 1 200 700 Tm (12 kg) Tj 1 0 0 1 40 680 Tm (Beta) Tj 1 0 0 1 200 680 Tm (25 kg) Tj ET");
    let mut options = Options::default();
    options.limits.max_table_cells = 3;
    assert!(matches!(
        Parser::new(options).parse_bytes(&bytes, Some("x.pdf")),
        Err(Error::Limit("max_table_cells"))
    ));
}

#[test]
fn zero_width_word_breaks_are_spaces_without_rewriting_actualtext() {
    let cmap="1 begincodespacerange<0000><FFFF>endcodespacerange 3 beginbfchar<0001><0041><0002><200B><0003><0042>endbfchar";
    let bytes = mapped("BT /F1 12 Tf 40 700 Td <000100020003> Tj ET", cmap);
    assert_eq!(text(&bytes).trim(), "A B");
    let options = Options {
        pdf_normalize_unicode: false,
        ..Default::default()
    };
    assert!(Parser::new(options)
        .parse_bytes(&bytes, Some("x.pdf"))
        .unwrap()
        .to_markdown()
        .text
        .contains('\u{200b}'));
    let actual=simple("BT /F1 12 Tf 40 700 Td /Span <</ActualText <FEFF0041200B0042>>> BDC (placeholder) Tj EMC ET");
    assert_eq!(text(&actual).trim(), "A\u{200b}B");
}

#[test]
fn borderless_fields_attach_right_aligned_headers_and_currency_codes() {
    let mut s =
        String::from("BT /F1 10 Tf 1 0 0 1 40 700 Tm (Product) Tj 1 0 0 1 250 700 Tm (Price) Tj ");
    for (r, (name, price)) in [
        ("Adapter", "USD 9.50"),
        ("Cable", "USD 120.00"),
        ("Socket", "USD 7.00"),
    ]
    .into_iter()
    .enumerate()
    {
        let y = 680 - r * 20;
        let x = 280.0 - price.len() as f64 * 6.0;
        s += &format!("1 0 0 1 40 {y} Tm ({name}) Tj 1 0 0 1 {x} {y} Tm ({price}) Tj ");
    }
    s += "ET";
    let tables = table_values(&parse(&simple(&s)));
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].len(), 4);
    assert_eq!(tables[0][0], ["Product", "Price"]);
    assert_eq!(tables[0][2], ["Cable", "USD 120.00"]);
}
