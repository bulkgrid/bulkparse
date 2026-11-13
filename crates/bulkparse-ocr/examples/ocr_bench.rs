//! Benchmark-only JSONL driver exercising the production parser and OCR provider.
use bulkparse::{CancellationToken, Options, Parser, PdfOcrBudget};
use bulkparse_ocr::ProcessOcr;
use serde::Deserialize;
use std::io::{BufRead, Write};
use std::time::Instant;

#[derive(Deserialize)]
struct Request {
    id: String,
    path: String,
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let settings = std::fs::read(&args[1])?;
    let provider = ProcessOcr::from_json(&settings)?;
    let parser = Parser::new(Options::default());
    println!("{{\"ready\":true}}");
    for line in std::io::stdin().lock().lines() {
        let request: Request = serde_json::from_str(&line?)?;
        let start = Instant::now();
        let result = std::fs::read(&request.path)
            .map_err(bulkparse::Error::from)
            .and_then(|bytes| {
                parser.parse_with_pdf_ocr(
                    &bytes,
                    Some(&request.path),
                    &CancellationToken::default(),
                    &provider,
                    PdfOcrBudget::default(),
                )
            });
        let value = match result {
            Ok(result) => {
                serde_json::json!({"id":request.id,"wall_seconds":start.elapsed().as_secs_f64(),"result":result})
            }
            Err(error) => {
                serde_json::json!({"id":request.id,"wall_seconds":start.elapsed().as_secs_f64(),"error":error.to_string()})
            }
        };
        println!("{value}");
        std::io::stdout().flush()?;
    }
    Ok(())
}
