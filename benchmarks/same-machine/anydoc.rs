//! Full-corpus adapter only; preserves AnyDoc's default native PDF behavior.
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

fn run(request: Value) -> io::Result<Value> {
    let paths: Vec<PathBuf> = serde_json::from_value(request["documents"].clone())?;
    let output_dir = PathBuf::from(request["output_dir"].as_str().unwrap());
    std::fs::create_dir_all(&output_dir)?;
    let mut outcomes = Vec::with_capacity(paths.len());
    let started = Instant::now();
    for path in paths {
        let id = path.file_stem().unwrap().to_str().unwrap();
        let output = output_dir.join(format!("{id}.md"));
        let doc_started = Instant::now();
        let result = std::panic::catch_unwind(|| convert(&path))
            .unwrap_or_else(|_| Err("parser panic".to_owned()));
        let (ok, error, status, markdown) = match result {
            Ok(md) => (true, None, Some("converted"), md),
            Err(error) => (false, Some(error), None, String::new()),
        };
        // Failed documents are empty predictions, never omitted or stale files.
        std::fs::write(output, markdown)?;
        outcomes.push(
            json!({"document_id":id,"elapsed_ns":doc_started.elapsed().as_nanos(),
            "ok":ok,"status":status,"error":error}),
        );
    }
    Ok(
        json!({"elapsed_ns":started.elapsed().as_nanos(),"documents":outcomes,
        "options":{"ocr":false,"pipeline":"AnyDoc default native PDF","rayon_threads":1}}),
    )
}
fn convert(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    anydoc::to_markdown_bytes(&bytes, anydoc::Format::Pdf).map_err(|e| e.to_string())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    for line in stdin.lock().lines() {
        let response = run(serde_json::from_str(&line?)?)?;
        serde_json::to_writer(&mut stdout, &response)?;
        writeln!(stdout)?;
        stdout.flush()?;
    }
    Ok(())
}
