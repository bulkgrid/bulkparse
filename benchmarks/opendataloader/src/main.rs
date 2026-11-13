//! One sequential full-corpus request per line, in one persistent process.
//! The timer includes input reads, native conversion, Markdown rendering and
//! output writes, matching the cited benchmark's file-to-file boundary.
use serde::{Deserialize, Serialize};
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::time::Instant;

#[derive(Deserialize)]
struct Request {
    documents: Vec<PathBuf>,
    output_dir: PathBuf,
}

#[derive(Serialize)]
struct Outcome {
    document_id: String,
    elapsed_ns: u128,
    ok: bool,
    status: Option<bulkparse::Status>,
    page_count: Option<usize>,
    ocr_pages: Vec<u32>,
    diagnostic_codes: Vec<String>,
    error: Option<String>,
}

fn run(request: Request) -> io::Result<serde_json::Value> {
    std::fs::create_dir_all(&request.output_dir)?;
    let parser = bulkparse::Parser::default();
    let mut outcomes = Vec::with_capacity(request.documents.len());
    let started = Instant::now();
    for path in &request.documents {
        let document_id = path.file_stem().unwrap().to_str().unwrap().to_owned();
        let output = request.output_dir.join(format!("{document_id}.md"));
        let doc_started = Instant::now();
        let result =
            std::panic::catch_unwind(|| parser.parse_file(path).map_err(|error| error.to_string()))
                .unwrap_or_else(|_| Err("parser panic".to_owned()));
        let mut outcome = Outcome {
            document_id,
            elapsed_ns: 0,
            ok: result.is_ok(),
            status: None,
            page_count: None,
            ocr_pages: Vec::new(),
            diagnostic_codes: Vec::new(),
            error: None,
        };
        match result {
            Ok(result) => {
                let rendered = result.to_markdown();
                std::fs::write(output, &rendered.text)?;
                outcome.status = Some(result.status);
                if let Some(inspection) = result.pdf_inspection {
                    outcome.page_count = Some(inspection.page_count);
                    outcome.ocr_pages = inspection.pages_needing_ocr;
                }
                outcome.diagnostic_codes = result
                    .diagnostics
                    .iter()
                    .chain(rendered.diagnostics.iter())
                    .map(|diagnostic| diagnostic.code.clone())
                    .collect();
            }
            Err(error) => {
                // An empty prediction stays in evaluation; never drop a failure
                // or accidentally reuse an output from the preceding pass.
                std::fs::write(output, "")?;
                outcome.error = Some(error);
            }
        }
        outcome.elapsed_ns = doc_started.elapsed().as_nanos();
        outcomes.push(outcome);
    }
    let elapsed_ns = started.elapsed().as_nanos();
    Ok(serde_json::json!({
        "elapsed_ns": elapsed_ns,
        "options": parser.options,
        "documents": outcomes,
    }))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    for line in stdin.lock().lines() {
        let request: Request = serde_json::from_str(&line?)?;
        let response = run(request)?;
        serde_json::to_writer(&mut stdout, &response)?;
        writeln!(stdout)?;
        stdout.flush()?;
    }
    Ok(())
}
