//! Benchmark-only JSONL adapter. One engine instance per process, no result cache.
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
use std::time::Instant;

fn emit(value: Value) {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    serde_json::to_writer(&mut out, &value).unwrap();
    writeln!(out).unwrap();
    out.flush().unwrap();
}
fn main() {
    #[cfg(feature = "bulkparse")]
    let parser = bulkparse::Parser::default();
    emit(json!({"ready":true,"pid":std::process::id()}));
    for line in io::stdin().lock().lines() {
        let line = line.unwrap();
        let request: Value = serde_json::from_str(&line).unwrap();
        let started = Instant::now();
        let result = (|| -> Result<(String, String), String> {
            let path = request["input"].as_str().ok_or("missing input")?;
            if std::fs::metadata(path).map_err(|e| e.to_string())?.len() > 33_554_432 {
                return Err("input limit".into());
            }
            #[cfg(feature = "bulkparse")]
            {
                let result = parser.parse_file(path).map_err(|e| e.to_string())?;
                let status = match result.status {
                    bulkparse::Status::Complete => "complete",
                    bulkparse::Status::Partial => "partial",
                };
                Ok((result.to_markdown().text, status.into()))
            }
            #[cfg(not(feature = "bulkparse"))]
            {
                let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
                let md = anydoc::to_markdown_bytes(&bytes, anydoc::Format::Pdf)
                    .map_err(|e| e.to_string())?;
                Ok((md, "converted".into()))
            }
        })();
        let service_ms = started.elapsed().as_secs_f64() * 1000.;
        match result {
            Ok((markdown, status)) => emit(
                json!({"id":request["id"],"ok":true,"status":status,"markdown":markdown,"service_ms":service_ms}),
            ),
            Err(error) => emit(
                json!({"id":request["id"],"ok":false,"error":error,"markdown":"","service_ms":service_ms}),
            ),
        }
    }
}
