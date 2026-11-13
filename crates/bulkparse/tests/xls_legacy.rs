#![cfg(feature = "office")]
use bulkparse::{Error, Options, Parser};
use std::io::{Cursor, Write};

fn record(kind: u16, data: &[u8]) -> Vec<u8> {
    let mut out = kind.to_le_bytes().to_vec();
    out.extend((data.len() as u16).to_le_bytes());
    out.extend(data);
    out
}
fn legacy_book(codepage: Option<u16>, name: &[u8], globals: &[u8], cells: &[u8]) -> Vec<u8> {
    let mut out = record(0x0809, &[0, 5, 5, 0]);
    let offset_position = out.len() + 4;
    let mut sheet = vec![0; 6];
    sheet.push(name.len() as u8);
    sheet.extend(name);
    out.extend(record(0x0085, &sheet));
    out.extend(globals);
    // Intentionally after names/formats: decoding cannot depend on this order.
    if let Some(cp) = codepage {
        out.extend(record(0x0042, &cp.to_le_bytes()));
    }
    out.extend(record(0x000a, &[]));
    let offset = out.len() as u32;
    out[offset_position..offset_position + 4].copy_from_slice(&offset.to_le_bytes());
    out.extend(record(0x0809, &[0, 5, 0x10, 0]));
    out.extend(cells);
    out.extend(record(0x000a, &[]));
    out
}
fn ole(stream: &[u8], name: &str) -> Vec<u8> {
    let mut file = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
    file.create_stream(name).unwrap().write_all(stream).unwrap();
    file.into_inner().into_inner()
}
fn label(row: u16, col: u16, text: &[u8]) -> Vec<u8> {
    let mut b = row.to_le_bytes().to_vec();
    b.extend(col.to_le_bytes());
    b.extend([0, 0]);
    b.extend((text.len() as u16).to_le_bytes());
    b.extend(text);
    record(0x0204, &b)
}
fn number(row: u16, style: u16, value: f64) -> Vec<u8> {
    let mut b = row.to_le_bytes().to_vec();
    b.extend([0, 0]);
    b.extend(style.to_le_bytes());
    b.extend(value.to_le_bytes());
    record(0x0203, &b)
}
#[test]
fn legacy_codepages_apply_to_sheet_names_cells_and_formats() {
    let mut globals = record(
        0x041e,
        &[164, 0, 7, b'"', 0xa3, b'"', b'0', b'.', b'0', b'0'],
    );
    globals.extend(record(0x00e0, &[0, 0, 164, 0]));
    let mut cells = label(0, 0, b"Price \x80 / caf\xe9");
    cells.extend(number(1, 0, 12.5));
    let stream = legacy_book(Some(1252), b"R\xe9sum\xe9", &globals, &cells);
    for name in ["Book", "Workbook"] {
        let bytes = ole(&stream, name);
        let result = Parser::default()
            .parse_bytes(&bytes, Some("ledger.xls"))
            .unwrap();
        let markdown = result.to_markdown().text;
        assert!(
            markdown.contains("Résumé") && markdown.contains("Price € / café"),
            "{markdown}"
        );
        assert!(markdown.contains("£12\\.50"), "{markdown}");
        let mut output = Vec::new();
        let streamed = Parser::default()
            .parse_to_markdown(&bytes, Some("ledger.xls"), &mut output)
            .unwrap();
        assert_eq!(output, markdown.as_bytes());
        assert_eq!(streamed.diagnostics, result.diagnostics);
    }
    let stream = legacy_book(
        Some(1251),
        &[0xcb, 0xe8, 0xf1, 0xf2],
        &[],
        &label(0, 0, &[0xd2, 0xe5, 0xf1, 0xf2]),
    );
    let result = Parser::default()
        .parse_bytes(&ole(&stream, "Book"), Some("cyrillic.xls"))
        .unwrap();
    let markdown = result.to_markdown().text;
    assert!(
        markdown.contains("Лист") && markdown.contains("Тест"),
        "{markdown}"
    );
}
#[test]
fn workbook_globals_select_text_layout_even_with_newer_sheet_bof() {
    let mut stream = legacy_book(Some(1252), b"Legacy", &[], &label(0, 0, b"plain \xa3 text"));
    // BOUNDSHEET begins after the eight-byte globals BOF and its record header.
    let offset = u32::from_le_bytes(stream[12..16].try_into().unwrap()) as usize;
    stream[offset + 4..offset + 6].copy_from_slice(&0x0600u16.to_le_bytes());
    let bytes = ole(&stream, "Book");
    let parsed = Parser::default()
        .parse_bytes(&bytes, Some("mixed-headers.xls"))
        .unwrap();
    assert!(parsed.to_markdown().text.contains("plain £ text"));
}

#[test]
fn legacy_continuations_preserve_multibyte_text_and_formula_caches() {
    // LABEL containing a Shift-JIS character split across records. CONTINUE's
    // first byte is text, not a BIFF8 Unicode-option flag.
    let mut cells = record(0x0204, &[0, 0, 0, 0, 0, 0, 4, 0, 0x93]);
    cells.extend(record(0x003c, &[0x8c, 0x8b, 0x9e]));
    let mut formula = vec![0; 22];
    formula[0] = 1;
    formula[12] = 255;
    formula[13] = 255;
    cells.extend(record(0x0006, &formula));
    cells.extend(record(0x0207, &[4, 0, 0x93]));
    cells.extend(record(0x003c, &[0x8c, 0x8b, 0x9e]));
    // RSTRING uses the same two-byte length before its formatting-run trailer.
    let mut rich = vec![2, 0, 0, 0, 0, 0, 4, 0, 0x93, 0x8c, 0x8b, 0x9e];
    rich.extend([1, 0, 0]);
    cells.extend(record(0x00d6, &rich));
    let bytes = ole(&legacy_book(Some(932), b"Cities", &[], &cells), "Book");
    let result = Parser::default()
        .parse_bytes(&bytes, Some("cities.xls"))
        .unwrap();
    assert_eq!(result.to_markdown().text.matches("東京").count(), 3);
    assert!(result
        .diagnostics
        .iter()
        .any(|d| d.code == "sheet_formula_cache"));
}
#[test]
fn legacy_dates_numbers_and_merges_use_existing_sheet_semantics() {
    let mut globals = record(0x0022, &[1, 0]);
    globals.extend(record(0x00e0, &[0, 0, 14, 0]));
    let mut cells = number(0, 0, 1.0);
    cells.extend(label(1, 0, b"Spanning"));
    cells.extend(record(0x00e5, &[1, 0, 1, 0, 1, 0, 0, 0, 1, 0]));
    let bytes = ole(&legacy_book(Some(1252), b"Dates", &globals, &cells), "Book");
    let result = Parser::default()
        .parse_bytes(&bytes, Some("dates.xls"))
        .unwrap();
    let markdown = result.to_markdown().text;
    assert!(
        markdown.contains("1904-01-02") && markdown.contains("colspan=\"2\""),
        "{markdown}"
    );
}
#[test]
fn legacy_malformed_text_unknown_encodings_encryption_and_limits_are_not_bypassed() {
    for cells in [
        record(0x0204, &[0, 0, 0, 0, 0, 0, 5, 0, b'A']),
        record(0x0204, &[0, 0, 0, 0, 0, 0, 1, 0, 0x93]),
    ] {
        let bytes = ole(&legacy_book(Some(932), b"Bad", &[], &cells), "Book");
        assert!(matches!(
            Parser::default().parse_bytes(&bytes, Some("bad.xls")),
            Err(Error::Malformed(_))
        ));
    }
    let bytes = ole(
        &legacy_book(Some(42), b"Unknown", &[], &label(0, 0, b"value")),
        "Book",
    );
    assert!(matches!(
        Parser::default().parse_bytes(&bytes, Some("unknown.xls")),
        Err(Error::Unsupported(_))
    ));
    let encrypted = ole(
        &legacy_book(Some(1252), b"Locked", &record(0x002f, &[0, 0]), &[]),
        "Book",
    );
    assert!(matches!(
        Parser::default().parse_bytes(&encrypted, Some("locked.xls")),
        Err(Error::Encrypted)
    ));
    let bytes = ole(
        &legacy_book(None, b"Assumed", &[], &label(0, 0, b"value")),
        "Book",
    );
    let result = Parser::default()
        .parse_bytes(&bytes, Some("assumed.xls"))
        .unwrap();
    assert!(result
        .diagnostics
        .iter()
        .any(|d| d.code == "xls_codepage_assumed"));
    let mut options = Options::default();
    options.limits.max_table_cells = 0;
    assert!(matches!(
        Parser::new(options).parse_bytes(&bytes, Some("limited.xls")),
        Err(Error::Limit("max_table_cells"))
    ));
    let mut stream = legacy_book(Some(1252), b"Old", &[], &[]);
    stream[4..6].copy_from_slice(&0x0400u16.to_le_bytes());
    assert!(matches!(
        Parser::default().parse_bytes(&ole(&stream, "Book"), Some("old.xls")),
        Err(Error::Unsupported(_))
    ));
}
