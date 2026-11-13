//! Diagnostic parse/render/drop timings. Sampling repeats are not benchmark scores.
use bulkparse::Parser;
use std::{hint::black_box, time::Instant};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let path = args.get(1).ok_or("usage: phase_profile FILE [REPEATS]")?;
    let repeats = args
        .get(2)
        .map(|s| s.parse::<usize>())
        .transpose()?
        .unwrap_or(1);
    let bytes = std::fs::read(path)?;
    let parser = Parser::default();
    for _ in 0..repeats {
        let start = Instant::now();
        let result = parser.parse_bytes(&bytes, Some(path))?;
        let parsed = start.elapsed();
        let start = Instant::now();
        let markdown = result.to_markdown();
        let rendered = start.elapsed();
        black_box(&markdown);
        let start = Instant::now();
        drop(result);
        drop(markdown);
        println!(
            "{}",
            serde_json::json!({"parse_ms":parsed.as_secs_f64()*1000.,"render_ms":rendered.as_secs_f64()*1000.,"drop_ms":start.elapsed().as_secs_f64()*1000.})
        );
    }
    Ok(())
}
