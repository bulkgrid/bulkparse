#![cfg(feature = "office")]
use bulkparse::{BlockContent, CancellationToken, Error, Format, Options, Parser, Status};
use std::io::{Cursor, Write};
fn zip(parts: &[(&str, &str)]) -> Vec<u8> {
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (n, s) in parts {
        z.start_file(*n, zip::write::SimpleFileOptions::default())
            .unwrap();
        z.write_all(s.as_bytes()).unwrap();
    }
    z.finish().unwrap().into_inner()
}
fn xlsx(sheet: &str, extra: &[(&str, &str)]) -> Vec<u8> {
    let mut p = vec![
        (
            "xl/workbook.xml",
            r#"<workbook xmlns:r="urn:r"><sheets><sheet name="Ledger" sheetId="1" r:id="sheet"/></sheets></workbook>"#,
        ),
        (
            "xl/_rels/workbook.xml.rels",
            r#"<Relationships><Relationship Id="sheet" Type="worksheet" Target="worksheets/a.xml"/></Relationships>"#,
        ),
        ("xl/worksheets/a.xml", sheet),
    ];
    p.extend_from_slice(extra);
    zip(&p)
}
fn parse(b: &[u8], n: &str) -> bulkparse::ParseResult {
    Parser::default().parse_bytes(b, Some(n)).unwrap()
}
fn rows(r: &bulkparse::ParseResult) -> &Vec<Vec<bulkparse::Cell>> {
    r.document
        .blocks
        .iter()
        .find_map(|b| {
            if let BlockContent::Table { rows, .. } = &b.content {
                Some(rows)
            } else {
                None
            }
        })
        .unwrap()
}
#[test]
fn xlsx_shared_cached_hidden_and_original_coordinates() {
    let b = xlsx(
        r#"<worksheet><dimension ref="A1:XFD1048576"/><sheetData><row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1"><f>1+2</f><v>3</v></c></row><row r="2" hidden="1"><c r="A2" t="inlineStr"><is><t>secret</t></is></c></row><row r="3"><c r="B3" t="b"><v>1</v></c></row></sheetData></worksheet>"#,
        &[(
            "xl/sharedStrings.xml",
            "<sst><si><r><t>東京</t></r><rPh><t>phonetic</t></rPh><r><t> total</t></r></si></sst>",
        )],
    );
    let r = parse(&b, "book.xlsm");
    assert_eq!(r.document.format, Format::Xlsx);
    let m = r.to_markdown().text;
    assert!(m.contains("東京 total") && m.contains("3") && m.contains("TRUE"));
    assert!(!m.contains("secret") && !m.contains("phonetic"));
    assert_eq!(rows(&r)[2][1].source.row, Some(3));
    assert_eq!(rows(&r)[2][1].source.cell, Some(2));
}

#[test]
fn xlsx_plain_cell_dedup_preserves_rich_run_boundaries() {
    let bytes = xlsx(
        r#"<worksheet><sheetData><row><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c><c r="C1" t="inlineStr"><is><r><rPr><b val="0"/><i val="0"/></rPr><t>plain</t></r></is></c></row></sheetData></worksheet>"#,
        &[(
            "xl/sharedStrings.xml",
            "<sst><si><t>single</t></si><si><r><t>first</t></r><r><t>second</t></r></si></sst>",
        )],
    );
    let parsed = parse(&bytes, "runs.xlsx");
    let cells = &rows(&parsed)[0];
    for (index, expected) in [
        (0, vec!["single"]),
        (1, vec!["first", "second"]),
        (2, vec!["plain"]),
    ] {
        let BlockContent::Paragraph { inlines } = &cells[index].blocks[0].content else {
            panic!("expected paragraph");
        };
        let texts: Vec<_> = inlines
            .iter()
            .map(|i| match i {
                bulkparse::Inline::Text { text } => text.as_str(),
                _ => panic!("unexpected style"),
            })
            .collect();
        assert_eq!(texts, expected);
    }
}
#[test]
fn xlsx_blank_merge_anchor_and_html_preserves_spans() {
    let b = xlsx(
        r#"<worksheet><sheetData><row r="2"><c r="C2" t="inlineStr"><is><t>tail</t></is></c></row></sheetData><mergeCells><mergeCell ref="A1:B2"/></mergeCells></worksheet>"#,
        &[],
    );
    let r = parse(&b, "a.xlsx");
    assert_eq!(rows(&r)[0][0].row_span, 2);
    assert_eq!(rows(&r)[0][0].col_span, 2);
    let m = r.to_markdown().text;
    assert!(m.contains("rowspan=\"2\"") && m.contains("colspan=\"2\""));
}
#[test]
fn formulas_without_cache_are_explicitly_partial() {
    let b=xlsx("<worksheet><sheetData><row><c r=\"A1\"><f>SUM(B1:B3)</f></c></row></sheetData></worksheet>",&[]);
    let r = parse(&b, "a.xlsx");
    assert_eq!(r.status, Status::Partial);
    assert!(r.to_markdown().text.contains("SUM"));
    let options = Options {
        strict: true,
        ..Default::default()
    };
    assert!(matches!(
        Parser::new(options).parse_bytes(&b, Some("a.xlsx")),
        Err(Error::Incomplete(_))
    ));
}
#[test]
fn workbook_date_epochs_are_honored() {
    for (epoch, serial, want) in [(false, 61, "1900-03-01"), (true, 0, "1904-01-01")] {
        let wb = format!(
            r#"<workbook xmlns:r="urn:r"><workbookPr date1904="{}"/><sheets><sheet name="D" r:id="r"/></sheets></workbook>"#,
            u8::from(epoch)
        );
        let sheet=format!("<worksheet><sheetData><row><c r=\"A1\" s=\"0\"><v>{serial}</v></c></row></sheetData></worksheet>");
        let b = zip(&[
            ("xl/workbook.xml", &wb),
            (
                "xl/_rels/workbook.xml.rels",
                r#"<Relationships><Relationship Id="r" Type="worksheet" Target="a.xml"/></Relationships>"#,
            ),
            ("xl/a.xml", &sheet),
            (
                "xl/styles.xml",
                "<styleSheet><cellXfs><xf numFmtId=\"14\"/></cellXfs></styleSheet>",
            ),
        ]);
        assert!(parse(&b, "a.xlsx").to_markdown().text.contains(want));
    }
}
#[test]
fn package_traversal_and_xml_entities_are_rejected() {
    let b = zip(&[("../xl/workbook.xml", "<workbook/>")]);
    assert!(Parser::default().parse_bytes(&b, Some("a.xlsx")).is_err());
    let b = xlsx(
        "<!DOCTYPE worksheet [<!ENTITY x 'boom'>]><worksheet>&x;</worksheet>",
        &[],
    );
    assert!(Parser::default().parse_bytes(&b, Some("a.xlsx")).is_err());
}
#[test]
fn sparse_sheet_and_repeated_text_respect_limits() {
    let b=xlsx("<worksheet><sheetData><row><c r=\"A1\"><v>1</v></c><c r=\"XFD1048576\"><v>2</v></c></row></sheetData></worksheet>",&[]);
    assert!(matches!(
        Parser::default().parse_bytes(&b, Some("a.xlsx")),
        Err(Error::Limit("max_table_cells"))
    ));
    let b=xlsx("<worksheet><sheetData><row><c r=\"A1\" t=\"s\"><v>0</v></c><c r=\"B1\" t=\"s\"><v>0</v></c></row></sheetData></worksheet>",&[("xl/sharedStrings.xml","<sst><si><t>abcdefghijklmnop</t></si></sst>")]);
    let mut o = Options::default();
    o.limits.max_text_bytes = 25;
    assert!(matches!(
        Parser::new(o).parse_bytes(&b, Some("a.xlsx")),
        Err(Error::Limit("max_text_bytes"))
    ));
}
#[test]
fn pptx_slide_order_grouped_text_and_layout_title() {
    let b=zip(&[("ppt/presentation.xml",r#"<presentation xmlns:r="urn:r"><sldIdLst><sldId id="256" r:id="second"/><sldId id="257" r:id="first"/></sldIdLst></presentation>"#),("ppt/_rels/presentation.xml.rels",r#"<Relationships><Relationship Id="first" Type="slide" Target="slides/a.xml"/><Relationship Id="second" Type="slide" Target="slides/b.xml"/></Relationships>"#),("ppt/slides/a.xml","<sld><cSld><spTree><grpSp><sp><txBody><p><r><t>Last</t></r></p></txBody></sp></grpSp></spTree></cSld></sld>"),("ppt/slides/b.xml","<sld><cSld><spTree><sp><nvSpPr><nvPr><ph idx=\"7\"/></nvPr></nvSpPr><txBody><p><r><t>First</t></r></p></txBody></sp></spTree></cSld></sld>"),("ppt/slides/_rels/b.xml.rels",r#"<Relationships><Relationship Id="l" Type="urn/slideLayout" Target="../slideLayouts/l.xml"/></Relationships>"#),("ppt/slideLayouts/l.xml","<sldLayout><cSld><spTree><sp><nvSpPr><nvPr><ph idx=\"7\" type=\"title\"/></nvPr></nvSpPr><txBody><lstStyle><lvl1pPr><defRPr b=\"1\"/></lvl1pPr></lstStyle></txBody></sp></spTree></cSld></sldLayout>")]);
    let r = parse(&b, "a.pptm");
    let m = r.to_markdown().text;
    assert!(m.contains("## **First**"));
    assert!(m.find("First").unwrap() < m.find("Last").unwrap());
    assert_eq!(r.document.blocks.last().unwrap().source.page, Some(2));
}
fn odf(mime: &str, body: &str) -> Vec<u8> {
    zip(&[("mimetype", mime), ("content.xml", body)])
}
#[test]
fn odf_text_lists_and_repeated_cells() {
    let b=odf("application/vnd.oasis.opendocument.text","<document-content><body><text><h outline-level=\"2\">Title</h><p>one<s c=\"2\"/>two</p><list><list-item><p>item</p></list-item></list></text></body></document-content>");
    let m = parse(&b, "a.odt").to_markdown().text;
    assert!(m.contains("## Title") && m.contains("one  two") && m.contains("- item"));
    let b=odf("application/vnd.oasis.opendocument.spreadsheet","<document-content><body><spreadsheet><table name=\"S\"><table-row number-rows-repeated=\"2\"><table-cell number-columns-repeated=\"2\" value=\"42\"/></table-row></table></spreadsheet></body></document-content>");
    let r = parse(&b, "a.ods");
    assert_eq!(rows(&r).len(), 2);
    assert_eq!(rows(&r)[0].len(), 2);
}
#[test]
fn odp_slide_provenance_and_repeat_amplification() {
    let b=odf("application/vnd.oasis.opendocument.presentation","<document-content><body><presentation><page name=\"Intro\"><frame><text-box><p>Hello</p></text-box></frame></page></presentation></body></document-content>");
    let r = parse(&b, "a.odp");
    assert_eq!(r.document.blocks[1].source.page, Some(1));
    let b=odf("application/vnd.oasis.opendocument.spreadsheet","<document-content><body><spreadsheet><table><table-row number-rows-repeated=\"1000000\"><table-cell value=\"1\"/></table-row></table></spreadsheet></body></document-content>");
    assert!(matches!(
        Parser::default().parse_bytes(&b, Some("a.ods")),
        Err(Error::Limit(_))
    ));
}
fn epub(body: &str) -> Vec<u8> {
    zip(&[("mimetype","application/epub+zip"),("META-INF/container.xml","<container><rootfiles><rootfile full-path=\"OPS/book.opf\"/></rootfiles></container>"),("OPS/book.opf","<package><manifest><item id=\"c\" href=\"chapter.xhtml\" media-type=\"application/xhtml+xml\"/></manifest><spine><itemref idref=\"c\"/></spine></package>"),("OPS/chapter.xhtml",body)])
}
#[test]
fn epub_doctype_image_and_following_content() {
    let b=epub("<!DOCTYPE html><html><body><h1>Chapter</h1><img alt=\"Diagram\"/><p>After image</p><ol start=\"-1\"><li>Item</li></ol></body></html>");
    let r = parse(&b, "a.epub");
    let m = r.to_markdown().text;
    assert_eq!(m.matches("Diagram").count(), 1);
    assert!(m.contains("After image") && m.contains("Item"));
    assert_eq!(r.status, Status::Partial);
}
#[test]
fn rtf_unicode_destinations_and_styling() {
    let b=br"{\rtf1\ansi\ansicpg1252{\fonttbl{\f0 HiddenFont;}}Hello {\b bold}\par \uc1\u945? \u-10179?\u-8704?{\*\unknown Secret}\par End}";
    let r = parse(b, "a.rtf");
    let m = r.to_markdown().text;
    assert!(m.contains("**bold**") && m.contains("α") && m.contains("😀") && m.contains("End"));
    assert!(!m.contains("Secret") && !m.contains("HiddenFont"));
}
#[test]
fn office_cancel_limits_and_aliases() {
    for (name, f) in [
        ("xlsm", Format::Xlsx),
        ("ppsx", Format::Pptx),
        ("ppsm", Format::Pptx),
        ("pot", Format::Ppt),
        ("xlsb", Format::Xlsb),
        ("docm", Format::Docx),
    ] {
        assert_eq!(Format::from_name(name), Some(f))
    }
    let b = xlsx("<worksheet/>", &[]);
    let t = CancellationToken::default();
    t.cancel();
    assert!(matches!(
        Parser::default().parse_with_cancel(&b, Some("a.xlsx"), &t),
        Err(Error::Cancelled)
    ));
    for key in 0..3 {
        let mut o = Options::default();
        match key {
            0 => o.limits.max_archive_entries = 1,
            1 => o.limits.max_nodes = 1,
            _ => o.limits.max_expanded_bytes = 1,
        }
        assert!(matches!(
            Parser::new(o).parse_bytes(&b, Some("a.xlsx")),
            Err(Error::Limit(_))
        ));
    }
}
fn ole(streams: &[(&str, &[u8])]) -> Vec<u8> {
    let mut o = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
    for (name, bytes) in streams {
        o.create_stream(name).unwrap().write_all(bytes).unwrap();
    }
    o.into_inner().into_inner()
}
fn put16(b: &mut [u8], p: usize, v: u16) {
    b[p..p + 2].copy_from_slice(&v.to_le_bytes())
}
fn put32(b: &mut [u8], p: usize, v: u32) {
    b[p..p + 4].copy_from_slice(&v.to_le_bytes())
}
#[test]
fn doc_simple_story_fields_and_encryption() {
    let text = b"Before\r\x13INSTRUCTION\x14Result\x15\rAfter\r";
    let mut b = vec![0; 512];
    put16(&mut b, 0, 0xA5EC);
    put16(&mut b, 2, 0xC1);
    put32(&mut b, 0x18, 512);
    put32(&mut b, 0x1c, 512 + text.len() as u32);
    put32(&mut b, 0x4c, text.len() as u32);
    b.extend_from_slice(text);
    let r = parse(&ole(&[("WordDocument", &b)]), "a.doc");
    let m = r.to_markdown().text;
    assert!(m.contains("Before") && m.contains("Result") && m.contains("After"));
    assert!(!m.contains("INSTRUCTION"));
    put16(&mut b, 10, 0x100);
    assert!(matches!(
        Parser::default().parse_bytes(&ole(&[("WordDocument", &b)]), Some("a.doc")),
        Err(Error::Encrypted)
    ));
}
fn biff(id: u16, data: &[u8]) -> Vec<u8> {
    let mut b = id.to_le_bytes().to_vec();
    b.extend((data.len() as u16).to_le_bytes());
    b.extend(data);
    b
}
#[test]
fn xls_biff_continuation_switches_encoding() {
    let mut bof = vec![0; 4];
    put16(&mut bof, 0, 0x600);
    put16(&mut bof, 2, 5);
    let mut b = biff(0x809, &bof);
    let sheet_pos = b.len() + 4;
    let mut sheet = vec![0; 6];
    sheet.extend([1, 0, b'S']);
    b.extend(biff(0x85, &sheet));
    let mut sst = vec![0; 8];
    put32(&mut sst, 0, 1);
    put32(&mut sst, 4, 1);
    sst.extend([4, 0, 0, b'A', b'B']);
    b.extend(biff(0xfc, &sst));
    b.extend(biff(0x3c, &[1, 0xa9, 0x03, 0xac, 0x20]));
    b.extend(biff(10, &[]));
    let offset = b.len() as u32;
    put32(&mut b, sheet_pos, offset);
    put16(&mut bof, 2, 0x10);
    b.extend(biff(0x809, &bof));
    b.extend(biff(0xfd, &[0; 10]));
    b.extend(biff(10, &[]));
    let r = parse(&ole(&[("Workbook", &b)]), "a.xls");
    assert!(r.to_markdown().text.contains("ABΩ€"));
}

#[test]
fn xls_embedded_chart_cells_do_not_collide_or_end_the_worksheet() {
    let mut bof = vec![0; 4];
    put16(&mut bof, 0, 0x600);
    put16(&mut bof, 2, 5);
    let mut b = biff(0x809, &bof);
    let sheet_pos = b.len() + 4;
    let mut sheet = vec![0; 6];
    sheet.extend([1, 0, b'S']);
    b.extend(biff(0x85, &sheet));
    b.extend(biff(10, &[]));
    let offset = b.len() as u32;
    put32(&mut b, sheet_pos, offset);
    put16(&mut bof, 2, 0x10);
    b.extend(biff(0x809, &bof));
    let number = |row: u16, v: f64| {
        let mut data = vec![0; 6];
        put16(&mut data, 0, row);
        data.extend(v.to_le_bytes());
        biff(0x203, &data)
    };
    b.extend(number(0, 123.0));
    put16(&mut bof, 2, 0x20);
    b.extend(biff(0x809, &bof));
    b.extend(number(0, 999.0)); // Chart has its own A1.
    b.extend(biff(10, &[]));
    b.extend(number(1, 456.0)); // Must survive the chart's EOF.
    b.extend(biff(10, &[]));
    let r = parse(&ole(&[("Workbook", &b)]), "chart.xls");
    let md = r.to_markdown().text;
    assert!(
        md.contains("123") && md.contains("456") && !md.contains("999"),
        "{md}"
    );
    assert!(r
        .diagnostics
        .iter()
        .any(|d| d.code == "xls_embedded_substream"));
    // Actual duplicates in the same worksheet are still malformed.
    b.truncate(b.len() - 4);
    b.extend(number(0, 777.0));
    b.extend(biff(10, &[]));
    assert!(Parser::default()
        .parse_bytes(&ole(&[("Workbook", &b)]), Some("duplicate.xls"))
        .is_err());
}
fn brt(id: u16, data: &[u8]) -> Vec<u8> {
    let mut b = if id > 127 {
        vec![(id as u8 & 127) | 128, (id >> 7) as u8]
    } else {
        vec![id as u8]
    };
    let mut n = data.len();
    loop {
        let v = (n & 127) as u8;
        n >>= 7;
        b.push(if n > 0 { v | 128 } else { v });
        if n == 0 {
            break;
        }
    }
    b.extend(data);
    b
}
fn wide(s: &str) -> Vec<u8> {
    let units = s.encode_utf16().collect::<Vec<_>>();
    let mut b = (units.len() as u32).to_le_bytes().to_vec();
    for c in units {
        b.extend(c.to_le_bytes())
    }
    b
}
#[test]
fn xlsb_binary_records_cached_values_and_truncation() {
    let mut sh = vec![0; 8];
    sh.extend(wide("r"));
    sh.extend(wide("Binary"));
    let wb = brt(156, &sh);
    let mut sheet = brt(0, &[0; 12]);
    let mut cell = vec![0; 8];
    cell.extend(wide("こんにちは"));
    sheet.extend(brt(6, &cell));
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for(name,b)in [("xl/workbook.bin",wb.as_slice()),("xl/a.bin",sheet.as_slice()),("xl/_rels/workbook.bin.rels",br#"<Relationships><Relationship Id="r" Type="worksheet" Target="a.bin"/></Relationships>"#.as_slice())]{z.start_file(name,zip::write::SimpleFileOptions::default()).unwrap();z.write_all(b).unwrap();}
    let b = z.finish().unwrap().into_inner();
    let r = parse(&b, "a.xlsb");
    assert!(r.to_markdown().text.contains("こんにちは"));
    let bad = zip(&[("xl/workbook.bin", "\u{80}")]);
    assert!(Parser::default().parse_bytes(&bad, Some("a.xlsb")).is_err());
}
#[test]
fn ppt_broken_edit_does_not_scan_stale_text() {
    let user = vec![0; 20];
    let b = ole(&[
        ("PowerPoint Document", b"stale secret text"),
        ("Current User", &user),
    ]);
    assert!(Parser::default().parse_bytes(&b, Some("a.ppt")).is_err());
}

#[test]
fn plain_pk_text_and_invalid_implicit_cell_coordinates() {
    let r = parse(b"PK is a normal text prefix.", "note.txt");
    assert_eq!(r.document.format, Format::Text);
    let b = xlsx(
        "<worksheet><sheetData><row r=\"0\"><c><v>1</v></c></row></sheetData></worksheet>",
        &[],
    );
    assert!(Parser::default().parse_bytes(&b, Some("bad.xlsx")).is_err());
}

#[test]
fn epub_css_cascade_visibility_and_reset() {
    let b = epub(
        r#"<html><head><style>p {font-weight:bold} .off {font-weight:normal} #chosen {font-style:italic!important} .hide {display:none} .hide span {display:block} </style></head><body><p>Heavy</p><p class="off">Plain</p><p id="chosen" style="font-style:normal">Slant</p><div class="hide"><span>SECRET</span></div><p>Before <b> spaced </b>after</p></body></html>"#,
    );
    let m = parse(&b, "cascade.epub").to_markdown().text;
    assert!(m.contains("**Heavy**"), "{m}");
    assert!(m.contains("Plain") && !m.contains("**Plain**"), "{m}");
    assert!(m.contains("Slant") && m.contains("***"), "{m}");
    assert!(!m.contains("SECRET"), "{m}");
    assert!(!m.contains("spaced **"), "{m}");
}

#[test]
fn epub_numbered_lists_preserve_reversed_signed_and_restarts() {
    let b = epub(
        r#"<html><body><ol reversed="reversed"><li>North</li><li>South</li></ol><ol start="-1"><li>Debt</li><li>Zero</li></ol><ol type="A"><li>First</li><li value="4">Fourth</li></ol></body></html>"#,
    );
    let m = parse(&b, "lists.epub").to_markdown().text;
    for value in [
        "2\\. North",
        "1\\. South",
        "-1. Debt",
        "0\\. Zero",
        "A. First",
        "D. Fourth",
    ] {
        assert!(m.contains(value), "{value}: {m}");
    }
}

#[test]
fn odf_inherited_styles_numbering_and_preserved_spacing() {
    let b = odf(
        "application/vnd.oasis.opendocument.text",
        r#"<document-content><automatic-styles><style name="base"><text-properties font-weight="bold"/></style><style name="child" parent-style-name="base"><text-properties font-style="italic"/></style><style name="plain" parent-style-name="child"><text-properties font-weight="normal"/></style><list-style name="ordered"><list-level-style-number level="1" num-format="1" start-value="3"/></list-style></automatic-styles><body><text><p style-name="child">Inherited</p><p style-name="plain">Reset</p><list style-name="ordered"><list-header><p>Header</p></list-header><list-item><p>Third</p></list-item><list-item start-value="9"><p>Ninth</p></list-item></list><p>A<s c="3"/>B<tab/>C</p></text></body></document-content>"#,
    );
    let m = parse(&b, "styles.odt").to_markdown().text;
    assert!(
        m.contains("***Inherited***") && m.contains("*Reset*") && !m.contains("**Reset**"),
        "{m}"
    );
    assert!(
        m.contains("3. Third") && m.contains("9. Ninth") && !m.contains("3. Header"),
        "{m}"
    );
    assert!(m.contains("A   B\tC"), "{m}");
}

#[test]
fn xlsx_rich_merged_cells_safe_links_comments_and_number_sections() {
    let b = xlsx(r#"<worksheet xmlns:r="urn:r"><sheetData><row><c r="A1" t="inlineStr"><is><r><rPr><b/></rPr><t>Rich &amp; safe</t></r></is></c><c r="C1" s="0"><v>-12.5</v></c><c r="C2" s="0"><v>0</v></c></row></sheetData><mergeCells><mergeCell ref="A1:B1"/></mergeCells><hyperlinks><hyperlink ref="A1" r:id="link"/></hyperlinks></worksheet>"#, &[
    ("xl/styles.xml", r#"<styleSheet><numFmts><numFmt numFmtId="164" formatCode="0.0;&quot;(&quot;0.0&quot;)&quot;;&quot;ZERO&quot;"/></numFmts><cellXfs><xf numFmtId="164"/></cellXfs></styleSheet>"#),
    ("xl/worksheets/_rels/a.xml.rels", r#"<Relationships><Relationship Id="link" Type="hyperlink" Target="javascript:alert(1)" TargetMode="External"/><Relationship Id="comments" Type="urn/comments" Target="../comments.xml"/></Relationships>"#),
    ("xl/comments.xml", "<comments><authors><author>Reviewer</author></authors><commentList><comment ref=\"C1\" authorId=\"0\"><text><t>Verify amount</t></text></comment></commentList></comments>"),
    ]);
    let r = parse(&b, "rich.xlsx");
    let m = r.to_markdown().text;
    for value in [
        "<strong>Rich &amp; safe</strong>",
        "(12.5)",
        "ZERO",
        "Verify amount",
        "Reviewer",
    ] {
        assert!(m.contains(value), "{value}: {m}");
    }
    assert!(!m.contains("javascript:"), "{m}");
    assert!(r
        .document
        .blocks
        .iter()
        .any(|b| b.source.part.as_deref() == Some("xl/comments.xml")));
}

#[test]
fn pptx_nested_mixed_lists_and_roman_numbering() {
    let b = zip(&[
        (
            "ppt/presentation.xml",
            r#"<presentation xmlns:r="urn:r"><sldIdLst><sldId r:id="s"/></sldIdLst></presentation>"#,
        ),
        (
            "ppt/_rels/presentation.xml.rels",
            r#"<Relationships><Relationship Id="s" Type="slide" Target="slides/a.xml"/></Relationships>"#,
        ),
        (
            "ppt/slides/a.xml",
            r#"<sld><cSld><spTree><sp><txBody><lstStyle><lvl1pPr><buAutoNum type="romanLcPeriod" startAt="3"/></lvl1pPr><lvl2pPr><buChar char="•"/></lvl2pPr></lstStyle><p><r><t>Third</t></r></p><p><pPr lvl="1"/><r><t>Nested</t></r></p><p><r><t>Fourth</t></r></p></txBody></sp></spTree></cSld></sld>"#,
        ),
    ]);
    let m = parse(&b, "nested.pptx").to_markdown().text;
    assert!(
        m.contains("iii. Third") && m.contains("iv. Fourth") && m.contains("- Nested"),
        "{m}"
    );
    assert!(
        m.find("Third").unwrap() < m.find("Nested").unwrap()
            && m.find("Nested").unwrap() < m.find("Fourth").unwrap()
    );
}

#[test]
fn charts_extract_cached_points_and_report_absent_cache() {
    let b = xlsx(
        r#"<worksheet/>"#,
        &[
            (
                "xl/worksheets/_rels/a.xml.rels",
                r#"<Relationships><Relationship Id="d" Type="urn/drawing" Target="../drawings/a.xml"/></Relationships>"#,
            ),
            (
                "xl/drawings/a.xml",
                r#"<drawing xmlns:r="urn:r"><graphicFrame><chart r:id="c"/></graphicFrame></drawing>"#,
            ),
            (
                "xl/drawings/_rels/a.xml.rels",
                r#"<Relationships><Relationship Id="c" Type="urn/chart" Target="../charts/a.xml"/></Relationships>"#,
            ),
            (
                "xl/charts/a.xml",
                r#"<chartSpace><chart><title><tx><rich><p><r><t>Quarterly</t></r></p></rich></tx></title><plotArea><barChart><ser><tx><v>Revenue</v></tx><cat><strLit><pt idx="4"><v>West</v></pt><pt idx="0"><v>East</v></pt></strLit></cat><val><numLit><pt idx="0"><v>17</v></pt><pt idx="4"><v>29</v></pt></numLit></val></ser><ser><tx><v>Uncached</v></tx><val><numRef><f>Sheet!A1</f></numRef></val></ser></barChart></plotArea></chart></chartSpace>"#,
            ),
        ],
    );
    let r = parse(&b, "chart.xlsx");
    let m = r.to_markdown().text;
    for value in ["Quarterly", "Revenue", "East", "West", "17", "29"] {
        assert!(m.contains(value), "{value}: {m}");
    }
    assert!(m.find("East").unwrap() < m.find("West").unwrap());
    assert!(r
        .diagnostics
        .iter()
        .any(|d| d.code == "office_chart_missing_cache"));
}

#[test]
fn odf_family_defaults_and_same_name_styles_do_not_collide() {
    let b = odf(
        "application/vnd.oasis.opendocument.text",
        r#"<document-content><styles><default-style family="paragraph"><text-properties font-weight="bold"/></default-style><style name="Same" family="paragraph"><text-properties font-weight="normal"/></style><style name="Same" family="text"><text-properties font-style="italic"/></style></styles><body><text><p>Default</p><p style-name="Same">Normal <span style-name="Same">Slant</span></p></text></body></document-content>"#,
    );
    let m = parse(&b, "families.odt").to_markdown().text;
    assert!(
        m.contains("**Default**") && m.contains("Normal *Slant*") && !m.contains("**Normal"),
        "{m}"
    );
}

#[test]
fn ods_plain_paragraph_plan_matches_styled_path_and_budgets() {
    // An explicitly named empty style forces the general cell-body path while
    // inheriting exactly the same default style as an unstyled paragraph.
    // Check both direct XML and streamed XML, including a mixed rich/plain row.
    for padding in ["".to_owned(), format!("<!--{}-->", "x".repeat(270_000))] {
        for (attributes, defaults) in [
            ("", "<text-properties font-weight=\"bold\"/>"),
            ("", "<text-properties font-style=\"italic\"/>"),
            ("display-name=\"Preformatted Text\"", ""),
            ("display-name=\"Quotation\"", ""),
            ("default-outline-level=\"3\"", ""),
        ] {
            let make = |style: &str| {
                odf(
                    "application/vnd.oasis.opendocument.spreadsheet",
                    &format!(
                        r#"<document-content>{padding}<styles><default-style family="paragraph" {attributes}>{defaults}</default-style><style family="paragraph" name="plain"/></styles><body><spreadsheet><table name="Values"><table-row number-rows-repeated="2"><table-cell number-columns-repeated="2"><p{style}>  Å &amp; 東京  </p></table-cell><table-cell><p>Rich <span>label</span></p></table-cell><table-cell><p{style}>Tail</p></table-cell></table-row></table></spreadsheet></body></document-content>"#
                    ),
                )
            };
            let fast = make("");
            let general = make(" style-name=\"plain\"");
            let parser = Parser::default();
            let expected = parser.parse_bytes(&general, Some("sheet.ods")).unwrap();
            let result = parser.parse_bytes(&fast, Some("sheet.ods")).unwrap();
            assert_eq!(
                serde_json::to_value(&result).unwrap(),
                serde_json::to_value(&expected).unwrap()
            );
            let mut output = Vec::new();
            parser
                .parse_to_markdown(&fast, Some("sheet.ods"), &mut output)
                .unwrap();
            assert_eq!(
                String::from_utf8(output).unwrap(),
                expected.to_markdown().text
            );
            for max_text in [1, 8, 20, 100] {
                let mut options = Options::default();
                options.limits.max_text_bytes = max_text;
                let parser = Parser::new(options);
                let signature = |bytes: &[u8]| {
                    parser
                        .parse_bytes(bytes, Some("sheet.ods"))
                        .map(|r| serde_json::to_value(r).unwrap())
                        .map_err(|e| e.code().to_owned())
                };
                assert_eq!(signature(&fast), signature(&general));
            }
        }
    }
}

#[test]
fn odf_style_cycles_and_css_work_limits_are_bounded() {
    let b = odf(
        "application/vnd.oasis.opendocument.text",
        r#"<document-content><styles><style name="a" parent-style-name="b"/><style name="b" parent-style-name="a"/></styles><body><text><p style-name="a">Loop</p></text></body></document-content>"#,
    );
    assert!(Parser::default().parse_bytes(&b, Some("loop.odt")).is_err());
    let rules = ".x{font-weight:bold}".repeat(200);
    let body = "<p class=\"x\">Text</p>".repeat(200);
    let b = epub(&format!(
        "<html><head><style>{rules}</style></head><body>{body}</body></html>"
    ));
    let mut o = Options::default();
    o.limits.max_nodes = 1000;
    assert!(matches!(
        Parser::new(o).parse_bytes(&b, Some("bounded.epub")),
        Err(Error::Limit(_))
    ));
}

#[test]
fn unsupported_conditional_css_cannot_silently_remove_restored_text() {
    let b = epub(
        r#"<html><head><style>h1{display:none}@supports(font-size:0){h1{display:block}}</style></head><body><h1>Accessible title</h1><p hidden="hidden">HTML hidden</p></body></html>"#,
    );
    let r = parse(&b, "conditional.epub");
    let m = r.to_markdown().text;
    assert!(
        m.contains("Accessible title") && !m.contains("HTML hidden"),
        "{m}"
    );
    assert_eq!(r.status, Status::Partial);
}

#[test]
fn legacy_rtf_styles_inherit_and_reset_without_losing_code_spacing() {
    let b=br"{\rtf1\ansi{\stylesheet{\s0 Normal;}{\s7\b\outlinelevel2 Section;}{\s8\sbasedon7\b0 Child;}{\s20 Source Code;}{\s21\sbasedon20 Custom Listing;}{\s30 Quote;}}\pard\s8 A heading\par\pard\plain\s21   let x = 8;\par\pard\s20 next();\par\pard\plain\s30 A quote\par\pard\plain Body\par}";
    let m = parse(b, "inherited.rtf").to_markdown().text;
    assert!(
        m.contains("### A heading") && !m.contains("**A heading**"),
        "{m}"
    );
    assert!(
        m.contains("```\n  let x = 8;\nnext();\n```")
            && m.contains("> A quote")
            && m.ends_with("Body\n"),
        "{m}"
    );
    let b = br"{\rtf1{\stylesheet{\s1\sbasedon2 A;}{\s2\sbasedon1 B;}}\s1 Cycle}";
    assert!(Parser::default().parse_bytes(b, Some("cycle.rtf")).is_err());
}
#[test]
fn rtf_equations_preserve_structure_and_skip_binary_payloads() {
    let b=br"{\rtf1 Before {\mmath{\*\moMath{\mf{\mnum{\mr a}}{\mden{\msSup{\me{\mr z}}{\msup{\mr 3}}}}}}} after.\par{\*\ignore{\mmath{\moMath{\mr SECRET}}}}}";
    let r = parse(b, "math.rtf");
    let m = r.to_markdown().text;
    assert!(
        m.contains("$\\frac{a}{z^{3}}$") && m.contains("after") && !m.contains("SECRET"),
        "{m}"
    );
    assert!(r.to_json().unwrap().contains("\"type\": \"math\""));
    let b = br"{\rtf1\bin6 {\s99}Visible}";
    let m = parse(b, "binary.rtf").to_markdown().text;
    assert!(m.contains("Visible"), "{m}");
}

#[test]
fn rtf_prelude_does_not_materialize_repeated_body_formatting() {
    let b = format!("{{\\rtf1\\ansi{{\\stylesheet{{\\s2\\outlinelevel1 Heading;}}}}\\s2 Title\\par\\pard {}Tail\\par}}", "\\b0\\i0\\fs24 ".repeat(2_000));
    let mut options = Options::default();
    options.limits.max_nodes = 100;
    let r = Parser::new(options.clone())
        .parse_bytes(b.as_bytes(), Some("formatting.rtf"))
        .unwrap();
    let md = r.to_markdown().text;
    assert!(md.contains("## Title") && md.contains("Tail"), "{md}");
    // Real output blocks still consume the node budget.
    let b = format!("{{\\rtf1 {}}}", "Text\\par ".repeat(101));
    assert!(matches!(
        Parser::new(options).parse_bytes(b.as_bytes(), Some("blocks.rtf")),
        Err(Error::Limit("max_nodes"))
    ));
}

#[test]
fn rtf_retained_inline_and_table_definition_allocations_stay_bounded() {
    for body in [
        "\\b A\\b0 B".repeat(120),
        "\\line ".repeat(120),
        "\\trowd ".to_owned() + &"\\cellx100 ".repeat(120),
    ] {
        let mut options = Options::default();
        options.limits.max_nodes = 100;
        let bytes = format!("{{\\rtf1 {body}}}");
        assert!(matches!(
            Parser::new(options).parse_bytes(bytes.as_bytes(), Some("limited.rtf")),
            Err(Error::Limit("max_nodes"))
        ));
    }
}
#[test]
fn odf_inline_and_embedded_math_keep_surrounding_text() {
    let b = zip(&[
        ("mimetype", "application/vnd.oasis.opendocument.text"),
        (
            "content.xml",
            r#"<document-content><body><text><p>Before <frame anchor-type="as-char"><object href="./Equation"/><image href="replacement.png"/></frame> after <object><math><mfrac><mi>q</mi><msqrt><mi>r</mi></msqrt></mfrac></math></object>.</p></text></body></document-content>"#,
        ),
        (
            "Equation/content.xml",
            r#"<math display="block"><semantics><msup><mi>y</mi><mn>7</mn></msup><annotation encoding="application/x-tex">EVIL</annotation></semantics></math>"#,
        ),
    ]);
    let r = parse(&b, "equations.odt");
    let m = r.to_markdown().text;
    assert!(
        m.contains("Before $y^{7}$ after $\\frac{q}{\\sqrt{r}}$"),
        "{m}"
    );
    assert!(!m.contains("EVIL") && !r.diagnostics.iter().any(|d| d.code == "odf_object"));
}
#[test]
fn epub_links_resolve_unique_case_sensitive_targets_and_missing_targets_warn() {
    let b = zip(&[
        ("mimetype", "application/epub+zip"),
        (
            "META-INF/container.xml",
            "<container><rootfiles><rootfile full-path=\"B/book.opf\"/></rootfiles></container>",
        ),
        (
            "B/book.opf",
            r#"<package><metadata><title>Navigation Test</title></metadata><manifest><item id="a" href="a.xhtml" media-type="application/xhtml+xml"/><item id="b" href="nested/b.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="a"/><itemref idref="b"/></spine></package>"#,
        ),
        (
            "B/a.xhtml",
            r##"<html><body><p><a href="nested/b.xhtml#A">Upper</a><a href="nested/b.xhtml#a">Lower</a><a href="#missing">Missing</a><a href="https://example.com/#A">Web</a></p></body></html>"##,
        ),
        (
            "B/nested/b.xhtml",
            r##"<html><body><h2 id="A">Upper target</h2><p id="a">Lower target <a href="../a.xhtml">Back</a></p></body></html>"##,
        ),
    ]);
    let r = parse(&b, "navigation.epub");
    let m = r.to_markdown().text;
    assert!(m.starts_with("# Navigation Test"), "{m}");
    let upper = m
        .split("[Upper](#")
        .nth(1)
        .unwrap()
        .split(')')
        .next()
        .unwrap();
    let lower = m
        .split("[Lower](#")
        .nth(1)
        .unwrap()
        .split(')')
        .next()
        .unwrap();
    assert_ne!(upper, lower);
    for id in [upper, lower] {
        assert_eq!(m.matches(&format!("<a id=\"{id}\"")).count(), 1);
    }
    assert!(m.contains("[Web](https://example.com/#A)"));
    assert!(r.diagnostics.iter().any(|d| d.code == "epub_link_target"));
}
#[test]
fn mathml_unknown_nodes_are_partial_and_annotations_cannot_inject_tex() {
    let b = epub(
        r#"<html><body><p><math><semantics><mrow><mi>x</mi><unsupported><mi>y</mi></unsupported></mrow><annotation>\input{secret}</annotation></semantics></math></p></body></html>"#,
    );
    let r = parse(&b, "unsupported.epub");
    let m = r.to_markdown().text;
    assert!(m.contains("$xy$") && !m.contains("secret"), "{m}");
    assert!(r
        .diagnostics
        .iter()
        .any(|d| d.code == "office_math_structure"));
    let mut o = Options::default();
    o.limits.max_depth = 5;
    assert!(matches!(
        Parser::new(o).parse_bytes(&b, Some("depth.epub")),
        Err(Error::Limit(_))
    ));
}
#[test]
fn ods_durations_retain_calendar_ambiguity_and_handle_elapsed_values() {
    let b = odf(
        "application/vnd.oasis.opendocument.spreadsheet",
        r#"<document-content><body><spreadsheet><table><table-row><table-cell time-value="P2DT3H"/><table-cell time-value="-PT0.5M"/><table-cell time-value="P1M"/><table-cell time-value="PT99999999999999999999H"/></table-row></table></spreadsheet></body></document-content>"#,
    );
    let m = parse(&b, "durations.ods").to_markdown().text;
    for value in ["51:00:00", "-0:00:30", "P1M", "PT99999999999999999999H"] {
        assert!(m.contains(value), "{value}: {m}");
    }
}
fn word_std(name: &str, parent: u16) -> Vec<u8> {
    let mut b = vec![0; 10];
    put16(&mut b, 2, (parent << 4) | 1);
    let units: Vec<_> = name.encode_utf16().collect();
    b.extend((units.len() as u16).to_le_bytes());
    for u in units {
        b.extend(u.to_le_bytes());
    }
    b.extend([0, 0]);
    let mut out = (b.len() as u16).to_le_bytes().to_vec();
    out.extend(b);
    out
}
#[test]
fn word_binary_style_chains_and_optional_corruption() {
    let text = b"first();\r  second();\rQuoted\r";
    let mut word = vec![0; 1536];
    put16(&mut word, 0, 0xa5ec);
    put16(&mut word, 2, 0xc1);
    put32(&mut word, 0x18, 512);
    put32(&mut word, 0x1c, 512 + text.len() as u32);
    put32(&mut word, 0x4c, text.len() as u32);
    word[512..512 + text.len()].copy_from_slice(text);
    let mut styles = vec![0; 20];
    put16(&mut styles, 0, 18);
    put16(&mut styles, 2, 4);
    put16(&mut styles, 4, 10);
    for (name, parent) in [
        ("Normal", 0xfff),
        ("HTML Preformatted", 0),
        ("My inherited style", 1),
        ("Quote", 0),
    ] {
        styles.extend(word_std(name, parent));
    }
    put32(&mut word, 0xa6, styles.len() as u32);
    put32(&mut word, 0x102, styles.len() as u32);
    put32(&mut word, 0x106, 12);
    styles.extend(512u32.to_le_bytes());
    styles.extend((512 + text.len() as u32).to_le_bytes());
    styles.extend(2u32.to_le_bytes());
    let page = &mut word[1024..1536];
    page[511] = 3;
    let ends = [512, 521, 534, 512 + text.len() as u32];
    for (i, v) in ends.into_iter().enumerate() {
        put32(page, i * 4, v);
    }
    for (i, style) in [1, 2, 3].into_iter().enumerate() {
        page[16 + i * 13] = (80 + i * 5) as u8;
        let off = 160 + i * 10;
        page[off] = 2;
        put16(page, off + 1, style);
    }
    let m = parse(
        &ole(&[("WordDocument", &word), ("0Table", &styles)]),
        "custom.doc",
    )
    .to_markdown()
    .text;
    assert!(
        m.contains("```\nfirst();\n  second();\n```") && m.contains("> Quoted"),
        "{m}"
    );
    put32(&mut word, 0xa2, 999999);
    let r = parse(
        &ole(&[("WordDocument", &word), ("0Table", &styles)]),
        "bad-style.doc",
    );
    assert!(
        r.to_markdown().text.contains("Quoted")
            && r.diagnostics.iter().any(|d| d.code == "doc_style_metadata")
    );
}
fn ppt_rec(version: u16, kind: u16, data: &[u8]) -> Vec<u8> {
    let mut b = version.to_le_bytes().to_vec();
    b.extend(kind.to_le_bytes());
    b.extend((data.len() as u32).to_le_bytes());
    b.extend(data);
    b
}
#[test]
fn ppt_notes_follow_slide_ids_not_sparse_list_positions() {
    let atom = |persist: u32, slide: u32| {
        let mut b = vec![0; 20];
        put32(&mut b, 0, persist);
        put32(&mut b, 12, slide);
        ppt_rec(0, 0x3f3, &b)
    };
    let mut list = atom(2, 777);
    list.extend(ppt_rec(0, 0xfa8, b"First"));
    list.extend(atom(3, 888));
    list.extend(ppt_rec(0, 0xfa8, b"Second"));
    let notes = atom(4, 0);
    let mut doc = ppt_rec(15, 0xff0, &list);
    doc.extend(ppt_rec(47, 0xff0, &notes));
    let doc = ppt_rec(15, 0x3e8, &doc);
    let mut stream = vec![0; 8];
    let mut offsets = vec![];
    for b in [doc, ppt_rec(15, 0x3ee, &[]), ppt_rec(15, 0x3ee, &[]), {
        let mut a = 888u32.to_le_bytes().to_vec();
        a.extend([0; 4]);
        let mut b = ppt_rec(0, 0x3f1, &a);
        b.extend(ppt_rec(0, 0xfa8, b"Only second has notes"));
        ppt_rec(15, 0x3f0, &b)
    }] {
        offsets.push(stream.len() as u32);
        stream.extend(b);
    }
    let dir = stream.len() as u32;
    let mut p = ((4u32 << 20) | 1).to_le_bytes().to_vec();
    for o in offsets {
        p.extend(o.to_le_bytes());
    }
    stream.extend(ppt_rec(0, 0x1772, &p));
    let edit = stream.len() as u32;
    let mut e = vec![0; 28];
    put32(&mut e, 12, dir);
    put32(&mut e, 16, 1);
    stream.extend(ppt_rec(0, 0xff5, &e));
    let mut user = vec![0; 20];
    put32(&mut user, 16, edit);
    let r = parse(
        &ole(&[("PowerPoint Document", &stream), ("Current User", &user)]),
        "notes.ppt",
    );
    let m = r.to_markdown().text;
    assert!(
        m.find("Second").unwrap() < m.find("Only second").unwrap(),
        "{m}"
    );
    let note = r
        .document
        .blocks
        .iter()
        .find(|b| matches!(b.content, BlockContent::Quote { .. }))
        .unwrap();
    assert_eq!(note.source.page, Some(2));
}

#[test]
fn pptx_math_choice_is_selected_once_with_surrounding_text() {
    let b = zip(&[
        (
            "ppt/presentation.xml",
            r#"<presentation xmlns:r="urn:r"><sldIdLst><sldId r:id="s"/></sldIdLst></presentation>"#,
        ),
        (
            "ppt/_rels/presentation.xml.rels",
            r#"<Relationships><Relationship Id="s" Type="slide" Target="slides/s.xml"/></Relationships>"#,
        ),
        (
            "ppt/slides/s.xml",
            r#"<sld><cSld><spTree><AlternateContent><Choice><sp><txBody><p><r><t>Evaluate </t></r><m><oMath><rad><deg/><e><sSub><e><r><t>v</t></r></e><sub><r><t>k</t></r></sub></sSub></e></rad></oMath></m><r><t> now</t></r></p></txBody></sp></Choice><Fallback><sp><txBody><p><r><t>FALLBACK DUPLICATE</t></r></p></txBody></sp></Fallback></AlternateContent></spTree></cSld></sld>"#,
        ),
    ]);
    let m = parse(&b, "choice.pptx").to_markdown().text;
    assert!(
        m.contains("Evaluate $\\sqrt{v_{k}}$ now") && !m.contains("FALLBACK"),
        "{m}"
    );
}
#[test]
fn epub_navigation_manifest_is_read_even_outside_spine() {
    let b = zip(&[
        ("mimetype", "application/epub+zip"),
        (
            "META-INF/container.xml",
            "<container><rootfiles><rootfile full-path=\"book.opf\"/></rootfiles></container>",
        ),
        (
            "book.opf",
            r#"<package><manifest><item id="toc" href="toc.xhtml" properties="nav" media-type="application/xhtml+xml"/><item id="c" href="c.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c"/></spine></package>"#,
        ),
        (
            "toc.xhtml",
            r#"<html><body><nav><h1>Contents</h1><ol><li><a href="c.xhtml#section">The chapter</a></li></ol></nav></body></html>"#,
        ),
        (
            "c.xhtml",
            r#"<html><body><h1 id="section">A section</h1><p>Body</p></body></html>"#,
        ),
    ]);
    let m = parse(&b, "toc.epub").to_markdown().text;
    let id = m
        .split("[The chapter](#")
        .nth(1)
        .unwrap()
        .split(')')
        .next()
        .unwrap();
    assert!(m.contains(&format!("<a id=\"{id}\"")), "{m}");
    assert!(m.find("Contents").unwrap() < m.find("A section").unwrap());
}

#[test]
fn epub_anchor_expansion_is_budgeted_and_math_text_cannot_inject_html_or_tex() {
    let b = epub(&format!(
        "<html><body><p id=\"{}\">tiny</p></body></html>",
        "a".repeat(100)
    ));
    let mut o = Options::default();
    o.limits.max_text_bytes = 150;
    assert!(matches!(
        Parser::new(o).parse_bytes(&b, Some("anchors.epub")),
        Err(Error::Limit("max_text_bytes"))
    ));
    let b = epub(
        r#"<html><body><p><math><mtext>\input{secret} &lt;script&gt;</mtext></math></p></body></html>"#,
    );
    let m = parse(&b, "literal.epub").to_markdown().text;
    assert!(!m.contains("\\input{") && !m.contains("<script>"), "{m}");
    assert!(
        m.contains("\\backslash{}input\\{secret\\}") && m.contains("\\lt script\\gt"),
        "{m}"
    );
}

#[test]
fn rtf_boundaries_define_spans_and_vertical_chains_end_at_gaps() {
    let r = parse(br"{\rtf1\trowd\trhdr\trleft-100\clvmgf\cellx100\cellx300 A\cell B\cell\row\trowd\trleft-100\clvmrg\cellx100\cellx200\cellx300 \cell C\cell D\cell\row\trowd\trleft-100\cellx300 gap\cell\row\trowd\trleft-100\clvmrg\cellx100\cellx300 orphan\cell E\cell\row}", "grid.rtf");
    let t = rows(&r);
    assert_eq!((t[0][0].row_span, t[0][1].col_span), (2, 2));
    assert_eq!(t[1].len(), 2);
    assert_eq!(t[2][0].col_span, 3);
    assert!(r.diagnostics.iter().any(|d| d.code == "rtf_merge_orphan"));
    let m = r.to_markdown().text;
    assert!(
        m.contains("<th rowspan=\"2\">A</th>") && m.contains("orphan"),
        "{m}"
    );
}
#[test]
fn rtf_horizontal_merge_retains_all_content_and_is_bounded() {
    let b=br"{\rtf1\trowd\clmgf\cellx100\clmrg\cellx200\cellx300 first\cell continued\cell tail\cell\row}";
    let r = parse(b, "merged.rtf");
    assert_eq!(rows(&r)[0].len(), 2);
    assert_eq!(rows(&r)[0][0].col_span, 2);
    let m = r.to_markdown().text;
    assert!(m.contains("first") && m.contains("continued") && m.contains("tail"));
    let mut o = Options::default();
    o.limits.max_table_cells = 2;
    assert!(matches!(
        Parser::new(o).parse_bytes(b, Some("a.rtf")),
        Err(Error::Limit("max_table_cells"))
    ));
}
#[test]
fn covered_merge_content_keeps_its_styles_and_original_provenance() {
    let b = xlsx(
        r#"<worksheet><sheetData><row><c r="A1" t="inlineStr"><is><t>anchor</t></is></c><c r="B1" t="inlineStr"><is><r><rPr><b/></rPr><t>covered</t></r></is></c></row></sheetData><mergeCells><mergeCell ref="A1:B1"/></mergeCells></worksheet>"#,
        &[],
    );
    let r = parse(&b, "a.xlsx");
    let t = rows(&r);
    assert_eq!(t[0].len(), 1);
    assert_eq!(t[0][0].col_span, 2);
    assert_eq!(t[0][0].blocks[1].source.cell, Some(2));
    assert!(r.to_markdown().text.contains("<strong>covered</strong>"));
    assert!(r
        .diagnostics
        .iter()
        .any(|d| d.code == "office_merge_content"));
}
#[test]
fn odf_cells_keep_lists_nested_tables_headers_and_line_breaks() {
    let b = odf(
        "application/vnd.oasis.opendocument.text",
        r#"<document-content><body><text><table><table-header-rows><table-row><table-cell number-columns-spanned="2"><p>Heading</p></table-cell><covered-table-cell/></table-row></table-header-rows><table-row><table-cell><list><list-item><p>Alpha &lt;tag&gt;<line-break/>Beta</p></list-item></list></table-cell><table-cell><table><table-row><table-cell><p>Nested</p></table-cell></table-row></table></table-cell></table-row></table></text></body></document-content>"#,
    );
    let r = parse(&b, "a.odt");
    let m = r.to_markdown().text;
    assert!(
        m.contains("<th colspan=\"2\">Heading</th>")
            && m.contains("<ul><li>")
            && m.contains("&lt;tag&gt;<br>Beta"),
        "{m}"
    );
    assert_eq!(m.matches("<table>").count(), 2);
    assert_eq!(m.matches("Nested").count(), 1);
}
fn slide(body: &str) -> Vec<u8> {
    let s = format!("<sld><cSld><spTree>{body}</spTree></cSld></sld>");
    zip(&[
        (
            "ppt/presentation.xml",
            r#"<presentation xmlns:r="urn:r"><sldIdLst><sldId r:id="s"/></sldIdLst></presentation>"#,
        ),
        (
            "ppt/_rels/presentation.xml.rels",
            r#"<Relationships><Relationship Id="s" Type="slide" Target="slides/s.xml"/></Relationships>"#,
        ),
        ("ppt/slides/s.xml", &s),
    ])
}
fn shape(text: &str, y: i64) -> String {
    format!(
        r#"<sp><spPr><xfrm><off x="0" y="{y}"/><ext cx="100" cy="10"/></xfrm></spPr><txBody><p><r><t>{text}</t></r></p></txBody></sp>"#
    )
}
#[test]
fn positioned_slides_sort_locally_but_unknowns_and_rotated_groups_keep_order() {
    let unknown = "<sp><txBody><p><r><t>Barrier</t></r></p></txBody></sp>";
    let body = format!(
        "{}{}{unknown}{}{}",
        shape("Bottom", 100),
        shape("Top", 0),
        shape("Later", 300),
        shape("Earlier", 200)
    );
    let m = parse(&slide(&body), "a.pptx").to_markdown().text;
    assert!(
        m.find("Top") < m.find("Bottom")
            && m.find("Bottom") < m.find("Barrier")
            && m.find("Earlier") < m.find("Later"),
        "{m}"
    );
    let group = format!(
        r#"<grpSp><grpSpPr><xfrm rot="5400000"><off x="0" y="0"/><ext cx="100" cy="200"/></xfrm></grpSpPr>{}{}</grpSp>"#,
        shape("First", 100),
        shape("Second", 0)
    );
    let m = parse(&slide(&group), "b.pptx").to_markdown().text;
    assert!(m.find("First") < m.find("Second"), "{m}");
}
#[test]
fn pptx_merged_header_and_rich_cell_lists_remain_structured() {
    let b = slide(
        r#"<graphicFrame><tbl><tblPr firstRow="1"/><tblGrid><gridCol/><gridCol/></tblGrid><tr><tc gridSpan="2"><txBody><p><r><rPr b="1"/><t>Heading</t></r></p></txBody></tc><tc hMerge="1"/></tr><tr><tc><txBody><p><pPr><buChar char="•"/></pPr><r><t>Item</t></r></p></txBody></tc><tc><txBody><p><r><t>Value</t></r></p></txBody></tc></tr></tbl></graphicFrame>"#,
    );
    let r = parse(&b, "a.pptx");
    let m = r.to_markdown().text;
    assert_eq!(rows(&r)[0].len(), 1);
    assert!(
        m.contains("<th colspan=\"2\"><strong>Heading</strong></th>") && m.contains("<ul><li>"),
        "{m}"
    );
}
#[test]
fn odp_position_units_sort_frames_without_reordering_text_documents() {
    let content = r#"<frame x="0cm" y="2.54cm" width="1in" height="10pt"><text-box><p>Bottom</p></text-box></frame><frame x="0pt" y="10mm" width="72pt" height="10pt"><text-box><p>Top</p></text-box></frame>"#;
    let b=odf("application/vnd.oasis.opendocument.presentation",&format!("<document-content><body><presentation><page>{content}</page></presentation></body></document-content>"));
    let m = parse(&b, "a.odp").to_markdown().text;
    assert!(m.find("Top") < m.find("Bottom"), "{m}");
    let b = odf(
        "application/vnd.oasis.opendocument.presentation",
        &format!(
            r#"<document-content><body><presentation><page><g transform="rotate(1.57)">{content}</g></page></presentation></body></document-content>"#
        ),
    );
    let m = parse(&b, "rotated.odp").to_markdown().text;
    assert!(m.find("Bottom") < m.find("Top"), "{m}");
}

#[test]
fn rtf_escaped_line_endings_are_paragraphs_and_raw_newlines_are_not() {
    for ending in ["\n", "\r", "\r\n"] {
        let b = format!("{{\\rtf1 Alpha\\{ending}Beta\\{ending}Gamma}}");
        let r = parse(b.as_bytes(), "newlines.rtf");
        assert_eq!(r.document.blocks.len(), 3);
        assert_eq!(r.to_markdown().text, "Alpha\n\nBeta\n\nGamma\n");
    }
    let r = parse(b"{\\rtf1 Al\npha\r\n}", "raw.rtf");
    assert_eq!(r.to_markdown().text, "Alpha\n");
    let b = b"{\\rtf1 Before\\\n\\trowd\\cellx100\\cellx200 A\\\nB\\cell C\\cell\\row After\\par}";
    let m = parse(b, "table.rtf").to_markdown().text;
    assert!(
        m.contains("Before\n\n") && m.contains("A  <br>B") && m.contains("After"),
        "{m}"
    );
}
#[test]
fn rtf_label_local_paragraph_resets_do_not_override_body_headings() {
    let b = br"{\rtf1\pard\outlinelevel2{\listtext\pard\plain \tab}\b Heading\par\pard Body\par}";
    let r = parse(b, "outline.rtf");
    assert!(matches!(
        &r.document.blocks[0].content,
        BlockContent::Heading { level: 3, .. }
    ));
    assert!(matches!(
        &r.document.blocks[1].content,
        BlockContent::Paragraph { .. }
    ));
}
fn binary_word_table_fixture() -> Vec<u8> {
    let text = b"A\x07B\x07\x07C\x07D\x07\x07after\r";
    let mut word = vec![0; 1536];
    put16(&mut word, 0, 0xa5ec);
    put16(&mut word, 2, 0xc1);
    put32(&mut word, 0x18, 512);
    put32(&mut word, 0x1c, 512 + text.len() as u32);
    put32(&mut word, 0x4c, text.len() as u32);
    word[512..512 + text.len()].copy_from_slice(text);
    let mut table = vec![];
    table.extend(512u32.to_le_bytes());
    table.extend((512 + text.len() as u32).to_le_bytes());
    table.extend(2u32.to_le_bytes());
    put32(&mut word, 0x106, 12);
    let normal = vec![0x16, 0x24, 1];
    let row = |edges: [i16; 3], merge: bool| {
        let mut p = vec![0x16, 0x24, 1, 0x17, 0x24, 1, 0x04, 0x34, 1];
        let mut operand = vec![2];
        for edge in edges {
            operand.extend(edge.to_le_bytes());
        }
        for flag in if merge { [2u16, 1] } else { [0, 0] } {
            operand.extend(flag.to_le_bytes());
            operand.extend([0; 18]);
        }
        p.extend([8, 0xd6]);
        p.extend((operand.len() as u16 + 1).to_le_bytes());
        p.extend(operand);
        p
    };
    let props = [
        normal.clone(),
        normal.clone(),
        row([0, 100, 300], true),
        normal.clone(),
        normal,
        row([0, 200, 300], false),
        vec![],
    ];
    let page = &mut word[1024..];
    page[511] = 7;
    for (i, end) in [0, 2, 4, 5, 7, 9, 10, 16].into_iter().enumerate() {
        put32(page, i * 4, 512 + end);
    }
    let mut off = 140;
    for (i, p) in props.iter().enumerate() {
        if off % 2 != 0 {
            off += 1;
        }
        page[32 + i * 13] = (off / 2) as u8;
        let mut pap = vec![0, 0];
        pap.extend(p);
        if pap.len() % 2 == 0 {
            pap.push(0);
        }
        page[off] = pap.len().div_ceil(2) as u8;
        page[off + 1..off + 1 + pap.len()].copy_from_slice(&pap);
        off += 1 + pap.len();
    }
    ole(&[("WordDocument", &word), ("0Table", &table)])
}
#[test]
fn word_rows_use_explicit_edges_and_preserve_merged_cell_content() {
    let r = parse(&binary_word_table_fixture(), "table.doc");
    let t = rows(&r);
    assert_eq!(t.len(), 2);
    assert_eq!(t[0][0].col_span, 3);
    assert_eq!(t[0].len(), 1);
    assert_eq!(t[1][0].col_span, 2);
    let m = r.to_markdown().text;
    assert!(
        m.contains("<th colspan=\"3\"><p>A</p><p>B</p></th>") && m.ends_with("after\n"),
        "{m}"
    );
    assert!(!r
        .diagnostics
        .iter()
        .any(|d| d.code == "doc_style_metadata" || d.code == "doc_table_metadata"));
    let mut o = Options::default();
    o.limits.max_table_cells = 5;
    assert!(matches!(
        Parser::new(o).parse_bytes(&binary_word_table_fixture(), Some("bounded.doc")),
        Err(Error::Limit("max_table_cells"))
    ));
}

#[test]
fn bounded_xlsx_preview_preserves_coordinates_and_shared_text() {
    let body = (1..=500).map(|i|format!("<row r=\"{i}\"><c r=\"C{i}\" t=\"s\"><v>0</v></c><c r=\"E{i}\"><v>{i}</v></c></row>")).collect::<String>();
    let bytes=zip(&[
        ("xl/workbook.xml",r#"<workbook xmlns:r="urn:r"><sheets><sheet name="Ledger" r:id="sheet"/></sheets></workbook>"#),
        ("xl/worksheets/a.xml",&format!("<worksheet><sheetData>{body}</sheetData></worksheet>")),
        ("xl/sharedStrings.xml","<sst><si><r><t>東京 &amp; </t></r><rPh><t>PHONETIC</t></rPh><r><t>café</t></r></si></sst>"),
        ("xl/_rels/workbook.xml.rels", "<Relationships><Relationship Id=\"sheet\" Type=\"worksheet\" Target=\"worksheets/a.xml\"/><Relationship Id=\"sst\" Type=\"/sharedStrings\" Target=\"sharedStrings.xml\"/></Relationships>")]);
    let mut options = Options::default();
    options.limits.max_nodes = 500;
    assert!(matches!(
        Parser::new(options.clone()).parse_bytes(&bytes, Some("large.xlsx")),
        Err(Error::Limit(_))
    ));
    options.partial_on_limit = true;
    let r = Parser::new(options.clone())
        .parse_bytes(&bytes, Some("large.xlsx"))
        .unwrap();
    assert_eq!(r.status, Status::Partial);
    assert!(r
        .diagnostics
        .iter()
        .any(|d| d.code == "resource_limit_partial" && d.content_loss));
    let md = r.to_markdown().text;
    assert!(
        md.contains("| 1 | 3 | 東京 &amp; café |") || md.contains("| 1 | 3 | 東京 & café |"),
        "{md}"
    );
    assert!(!md.contains("PHONETIC") && !md.contains("| 500 |"));
    assert!(rows(&r).len() > 2);
    for cell in rows(&r).iter().skip(1).flatten() {
        assert!(cell.source.row.is_some() && cell.source.cell.is_some());
    }
    options.strict = true;
    assert!(matches!(
        Parser::new(options).parse_bytes(&bytes, Some("large.xlsx")),
        Err(Error::Limit(_))
    ));
}

fn preview_ods(content: &str) -> Vec<u8> {
    zip(&[
        ("mimetype", "application/vnd.oasis.opendocument.spreadsheet"),
        ("content.xml", content),
    ])
}
fn large_ods_body(rows: &str) -> String {
    format!("<document-content><body><spreadsheet><table name=\"Data\">{rows}</table></spreadsheet></body></document-content>")
}
#[test]
fn bounded_ods_stream_stops_before_expanded_limit_and_keeps_repeated_coordinates() {
    let row="<table-row number-rows-repeated=\"10000\"><table-cell number-columns-repeated=\"2\"><p>café &amp; tea</p></table-cell><table-cell value=\"7\"/></table-row>";
    let xml = large_ods_body(&(row.to_owned() + &" ".repeat(20_000)));
    let bytes = preview_ods(&xml);
    let mut options = Options {
        partial_on_limit: true,
        ..Options::default()
    };
    options.limits.max_expanded_bytes = 2048;
    options.limits.max_table_cells = 33;
    let r = Parser::new(options)
        .parse_bytes(&bytes, Some("repeated.ods"))
        .unwrap();
    assert_eq!(r.status, Status::Partial);
    assert!(rows(&r).iter().map(Vec::len).sum::<usize>() <= 33);
    let md = r.to_markdown().text;
    assert!(
        md.contains("| 1 | 1 | café &amp; tea |") || md.contains("| 1 | 1 | café & tea |"),
        "{md}"
    );
    assert!(
        md.contains("| 1 | 3 | 7 |") && md.contains("| 2 | 1 |"),
        "{md}"
    );
    assert!(!md.contains("| 10000 |"));
}
#[test]
fn bounded_recovery_rejects_bad_xml_dtd_depth_and_empty_results() {
    for prefix in [
        "<!DOCTYPE x [<!ENTITY a 'secret'>]><document-content/>",
        "<document-content><body></wrong>",
    ] {
        let b = preview_ods(&(prefix.to_owned() + &" ".repeat(4000)));
        let mut o = Options {
            partial_on_limit: true,
            ..Options::default()
        };
        o.limits.max_expanded_bytes = 1000;
        assert!(matches!(
            Parser::new(o).parse_bytes(&b, Some("invalid.ods")),
            Err(Error::Malformed(_))
        ));
    }
    let xml = large_ods_body(&format!(
        "<table-row><table-cell><p>{}text{}</p></table-cell></table-row>",
        "<span>".repeat(20),
        "</span>".repeat(20)
    ));
    let b = preview_ods(&(xml + &" ".repeat(4000)));
    let mut o = Options {
        partial_on_limit: true,
        ..Options::default()
    };
    o.limits.max_expanded_bytes = 1000;
    o.limits.max_depth = 8;
    assert!(matches!(
        Parser::new(o.clone()).parse_bytes(&b, Some("deep.ods")),
        Err(Error::Limit("max_depth"))
    ));
    o.limits.max_depth = 64;
    o.limits.max_table_cells = 0;
    assert!(Parser::new(o).parse_bytes(&b, Some("empty.ods")).is_err());
}
#[test]
fn bounded_option_does_not_change_successful_office_documents() {
    let bytes=xlsx("<worksheet><sheetData><row><c r=\"B2\" t=\"inlineStr\"><is><t>Hello</t></is></c></row></sheetData></worksheet>",&[]);
    let normal = parse(&bytes, "small.xlsx");
    let enabled = Parser::new(Options {
        partial_on_limit: true,
        ..Default::default()
    })
    .parse_bytes(&bytes, Some("small.xlsx"))
    .unwrap();
    assert_eq!(normal.to_json().unwrap(), enabled.to_json().unwrap());
}

#[test]
fn bounded_xls_keeps_only_completed_worksheets() {
    let mut bof = vec![0; 4];
    put16(&mut bof, 0, 0x600);
    put16(&mut bof, 2, 5);
    let mut b = biff(0x809, &bof);
    let mut offsets = vec![];
    for name in *b"AB" {
        offsets.push(b.len() + 4);
        let mut d = vec![0; 6];
        d.extend([1, 0, name]);
        b.extend(biff(0x85, &d));
    }
    b.extend(biff(10, &[]));
    put16(&mut bof, 2, 0x10);
    for (sheet, rows) in [1usize, 200].into_iter().enumerate() {
        let offset = b.len() as u32;
        put32(&mut b, offsets[sheet], offset);
        b.extend(biff(0x809, &bof));
        for row in 0..rows {
            let mut d = vec![0; 6];
            put16(&mut d, 0, row as u16);
            d.extend((if sheet == 0 { 123.0f64 } else { 999.0f64 }).to_le_bytes());
            b.extend(biff(0x203, &d));
        }
        b.extend(biff(10, &[]));
    }
    let bytes = ole(&[("Workbook", &b)]);
    let mut o = Options::default();
    o.limits.max_nodes = 100;
    assert!(matches!(
        Parser::new(o.clone()).parse_bytes(&bytes, Some("sheets.xls")),
        Err(Error::Limit(_))
    ));
    o.partial_on_limit = true;
    let r = Parser::new(o)
        .parse_bytes(&bytes, Some("sheets.xls"))
        .unwrap();
    let md = r.to_markdown().text;
    assert!(md.contains("123") && !md.contains("999"), "{md}");
    assert_eq!(r.status, Status::Partial);
    assert!(r
        .diagnostics
        .iter()
        .any(|d| d.code == "xls_sheets_truncated"));
    // Here the full BIFF index fits, but the second sheet exhausts the table
    // budget. Streaming must not leak its heading or any of its rows.
    let mut o = Options {
        partial_on_limit: true,
        ..Options::default()
    };
    o.limits.max_table_cells = 3;
    let parser = Parser::new(o);
    let normal = parser.parse_bytes(&bytes, Some("sheets.xls")).unwrap();
    let mut streamed = Vec::new();
    let summary = parser
        .parse_to_markdown(&bytes, Some("sheets.xls"), &mut streamed)
        .unwrap();
    assert_eq!(streamed, normal.to_markdown().text.as_bytes());
    assert_eq!(summary.status, normal.status);
    assert_eq!(
        serde_json::to_value(summary.diagnostics).unwrap(),
        serde_json::to_value(normal.diagnostics).unwrap()
    );
}

#[test]
fn bounded_xlsx_rejects_duplicate_cells_in_consumed_prefix() {
    let b=xlsx(&format!("<worksheet><sheetData><row><c r=\"A1\"><v>1</v></c><c r=\"A1\"><v>2</v></c></row></sheetData>{}</worksheet>"," ".repeat(10_000)),&[]);
    let mut o = Options {
        partial_on_limit: true,
        ..Options::default()
    };
    o.limits.max_expanded_bytes = 2000;
    assert!(matches!(
        Parser::new(o).parse_bytes(&b, Some("duplicate.xlsx")),
        Err(Error::Malformed(_))
    ));
}

#[test]
fn diagnostic_categories_distinguish_source_formula_gaps_from_parser_omissions() {
    use bulkparse::{DiagnosticCategory as C, DiagnosticEvidence as E};
    let bytes = xlsx("<worksheet><sheetData><row><c r=\"A1\"><f>1+2</f></c><c r=\"B1\" t=\"e\"><v>#REF!</v></c></row></sheetData></worksheet>", &[]);
    let result = parse(&bytes, "formulas.xlsx");
    for code in ["xlsx_formula_missing_cache", "xlsx_error_cell"] {
        let d = result.diagnostics.iter().find(|d| d.code == code).unwrap();
        assert_eq!((d.category, d.evidence), (C::Informational, E::Detected));
    }
    assert!(serde_json::to_string(&result.document)
        .unwrap()
        .contains("=1+2"));
    assert_eq!(result.status, Status::Partial); // Strict/fidelity contract unchanged.
    let strict = Parser::new(Options {
        strict: true,
        ..Default::default()
    });
    assert!(matches!(
        strict.parse_bytes(&bytes, Some("formulas.xlsx")),
        Err(Error::Incomplete(_))
    ));
    let mut stream = Vec::new();
    let streamed = Parser::default()
        .parse_to_markdown(&bytes, Some("formulas.xlsx"), &mut stream)
        .unwrap();
    assert_eq!(result.diagnostics, streamed.diagnostics);

    let rtf = parse(br"{\rtf1 Before {\pict\pngblip 00ff} after}", "image.rtf");
    let d = rtf
        .diagnostics
        .iter()
        .find(|d| d.code == "rtf_object")
        .unwrap();
    assert_eq!((d.category, d.evidence), (C::MissingContent, E::Detected));
}

#[test]
fn positioned_docx_tab_reports_layout_instead_of_unknown_content() {
    use bulkparse::{DiagnosticCategory as C, DiagnosticEvidence as E};
    let bytes = zip(&[(
        "word/document.xml",
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Before</w:t><w:ptab w:alignment="right"/><w:t>After</w:t></w:r></w:p></w:body></w:document>"#,
    )]);
    let result = parse(&bytes, "aligned.docx");
    assert!(result.to_markdown().text.contains("BeforeAfter"));
    let d = result
        .diagnostics
        .iter()
        .find(|d| d.code == "docx_positioned_tab_layout")
        .unwrap();
    assert_eq!(
        (d.category, d.evidence),
        (C::FormattingLimitation, E::Detected)
    );
    assert!(!result
        .diagnostics
        .iter()
        .any(|d| d.code == "docx_unsupported_inline"));
}

#[test]
fn xls_drawing_and_comment_text_preserves_provenance_and_streaming() {
    let mut bof = vec![0; 4];
    put16(&mut bof, 0, 0x600);
    put16(&mut bof, 2, 5);
    let mut b = biff(0x809, &bof);
    let sheet_pos = b.len() + 4;
    let mut sheet = vec![0; 6];
    sheet.extend([1, 0, b'S']);
    b.extend(biff(0x85, &sheet));
    b.extend(biff(10, &[]));
    let offset = b.len() as u32;
    put32(&mut b, sheet_pos, offset);
    put16(&mut bof, 2, 0x10);
    b.extend(biff(0x809, &bof));
    // Comment object 7; the NOTE locating it appears after its text records.
    b.extend(biff(0x5d, &[0x15, 0, 4, 0, 0x19, 0, 7, 0]));
    let mut txo = [0; 18];
    put16(&mut txo, 10, 4);
    b.extend(biff(0x1b6, &txo));
    b.extend(biff(0x3c, &[0, b'a', b'b']));
    b.extend(biff(0x3c, &[1, 0x3d, 0xd8, 0, 0xde]));
    b.extend(biff(0x3c, &[0; 8]));
    b.extend(biff(0x1c, &[2, 0, 3, 0, 0, 0, 7, 0]));
    // An incomplete optional text object must not corrupt existing text.
    b.extend(biff(0x1b6, &txo));
    b.extend(biff(0x3c, &[0, b'x']));
    b.extend(biff(10, &[]));
    let bytes = ole(&[("Workbook", &b)]);
    let parsed = parse(&bytes, "notes.xls");
    let markdown = parsed.to_markdown().text;
    assert_eq!(markdown.matches("ab😀").count(), 1);
    assert!(markdown.contains("row 3, column 4"));
    assert!(parsed
        .diagnostics
        .iter()
        .any(|d| d.code == "xls_drawing_text_unreadable"));
    let quote = parsed
        .document
        .blocks
        .iter()
        .find(|b| matches!(b.content, BlockContent::Quote { .. }))
        .unwrap();
    assert_eq!((quote.source.row, quote.source.cell), (Some(3), Some(4)));
    let mut out = Vec::new();
    let streamed = Parser::default()
        .parse_to_markdown(&bytes, Some("notes.xls"), &mut out)
        .unwrap();
    assert_eq!(out, markdown.as_bytes());
    assert_eq!(streamed.diagnostics, parsed.diagnostics);
}

#[test]
fn docx_drawing_titles_survive_text_boxes_without_duplicate_visible_descriptions() {
    let bytes = zip(&[(
        "word/document.xml",
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:x="urn:drawing"><w:body><w:p><w:r><w:drawing><x:docPr descr="Visible - LABEL." title="Additional explanation"/><w:txbxContent><w:p><w:r><w:t>Visible label</w:t></w:r></w:p></w:txbxContent></w:drawing></w:r></w:p></w:body></w:document>"#,
    )]);
    let parsed = parse(&bytes, "labels.docx");
    let md = parsed.to_markdown().text;
    assert_eq!(md.matches("Visible label").count(), 1);
    assert_eq!(md.matches("Additional explanation").count(), 1);
}

#[test]
fn binary_word_auxiliary_stories_keep_text_field_results_and_source() {
    let main = b"Body\r";
    let note = "Note 😀 \u{13}AUTHOR\u{14}cached\u{15}\r";
    let mut word = vec![0; 512];
    put16(&mut word, 0, 0xa5ec);
    put16(&mut word, 2, 0xc1);
    put32(&mut word, 0x4c, main.len() as u32);
    put32(&mut word, 0x50, note.encode_utf16().count() as u32);
    word.extend(main);
    let note_offset = word.len() as u32;
    for unit in note.encode_utf16() {
        word.extend(unit.to_le_bytes());
    }
    let mut pieces = vec![0; 28];
    put32(&mut pieces, 4, main.len() as u32);
    put32(
        &mut pieces,
        8,
        main.len() as u32 + note.encode_utf16().count() as u32,
    );
    put32(&mut pieces, 14, 0x40000000 | 1024);
    put32(&mut pieces, 22, note_offset);
    let mut table = vec![2];
    table.extend((pieces.len() as u32).to_le_bytes());
    table.extend(pieces);
    put32(&mut word, 0x1a6, table.len() as u32);
    let bytes = ole(&[("WordDocument", &word), ("0Table", &table)]);
    let result = parse(&bytes, "notes.doc");
    let md = result.to_markdown().text;
    assert!(md.starts_with("Body\n"));
    assert!(md.contains("### Footnotes"));
    assert!(md.contains("Note 😀 cached"));
    assert!(!md.contains("AUTHOR"));
    assert!(result
        .document
        .blocks
        .iter()
        .any(|b| b.source.part.as_deref() == Some("WordDocument/Footnotes")));
    assert!(result
        .diagnostics
        .iter()
        .any(|d| d.code == "doc_auxiliary_layout"
            && d.category == bulkparse::DiagnosticCategory::FormattingLimitation));
    let mut output = Vec::new();
    let stream = Parser::default()
        .parse_to_markdown(&bytes, Some("notes.doc"), &mut output)
        .unwrap();
    assert_eq!(output, md.as_bytes());
    assert_eq!(stream.diagnostics, result.diagnostics);
    let mut options = Options::default();
    options.limits.max_text_bytes = 8;
    assert!(matches!(
        Parser::new(options).parse_bytes(&bytes, Some("notes.doc")),
        Err(Error::Limit("max_text_bytes"))
    ));
}
