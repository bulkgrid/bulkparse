//! Sequential persistent JSONL benchmark worker. File I/O and JSON serialization
//! are outside the measured interval. Native extraction only; OCR never enabled.
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
use std::time::Instant;

#[cfg(feature = "bulkparse-engine")]
fn run(bytes: &[u8], mode: &str) -> Result<(u128, Value), String> {
    let parser = bulkparse::Parser::default();
    let start = Instant::now();
    if mode == "inspect" {
        let result = parser.inspect_pdf(bytes).map_err(|e| e.to_string())?;
        let ns = start.elapsed().as_nanos();
        Ok((
            ns,
            json!({"page_count":result.page_count,"ocr_pages":result.pages_needing_ocr,"report":result}),
        ))
    } else {
        let result = parser
            .parse_bytes(bytes, Some("input.pdf"))
            .map_err(|e| e.to_string())?;
        let markdown = result.to_markdown().text;
        let ns = start.elapsed().as_nanos();
        let inspection = result.pdf_inspection.as_ref().unwrap();
        Ok((
            ns,
            json!({"page_count":inspection.page_count,"ocr_pages":inspection.pages_needing_ocr,"markdown":markdown,"status":result.status}),
        ))
    }
}
#[cfg(not(feature = "bulkparse-engine"))]
fn run(bytes: &[u8], mode: &str) -> Result<(u128, Value), String> {
    use pdf_inspector::{DetectionConfig, PdfOptions, ProcessMode, ScanStrategy};
    let options = PdfOptions::new()
        .detection(DetectionConfig {
            strategy: ScanStrategy::Full,
            ..Default::default()
        })
        .mode(if mode == "inspect" {
            ProcessMode::DetectOnly
        } else {
            ProcessMode::Full
        });
    let start = Instant::now();
    let result =
        pdf_inspector::process_pdf_mem_with_options(bytes, options).map_err(|e| e.to_string())?;
    let ns = start.elapsed().as_nanos();
    Ok((
        ns,
        json!({"page_count":result.page_count,"ocr_pages":result.pages_needing_ocr,"kind":format!("{:?}",result.pdf_type),"markdown":result.markdown}),
    ))
}
fn main() {
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    for line in stdin.lock().lines() {
        let request: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let bytes = std::fs::read(request["path"].as_str().unwrap()).unwrap();
        let mode = request["mode"].as_str().unwrap();
        assert!(matches!(mode, "inspect" | "convert"));
        let start = Instant::now();
        let result = std::panic::catch_unwind(|| run(&bytes, mode));
        let reply = match result {
            Ok(Ok((ns, result))) => json!({"ok":true,"ns":ns,"result":result}),
            Ok(Err(error)) => json!({"ok":false,"ns":start.elapsed().as_nanos(),"error":error}),
            Err(_) => json!({"ok":false,"ns":start.elapsed().as_nanos(),"error":"panic"}),
        };
        serde_json::to_writer(&mut stdout, &reply).unwrap();
        writeln!(stdout).unwrap();
        stdout.flush().unwrap();
    }
}
