//! Isolate Markdown rendering from parsing; use release builds for comparisons.
use std::{hint::black_box, time::Instant};
#[path = "../../../benchmarks/realworld/cpu.rs"]
mod cpu;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("usage: markdown_bench FILE [REPEATS]")?;
    let repeats = args
        .next()
        .map(|s| s.parse::<usize>())
        .transpose()?
        .unwrap_or(100);
    let bytes = std::fs::read(&path)?;
    let result = bulkparse::Parser::default().parse_bytes(&bytes, Some(&path))?;
    for _ in 0..5 {
        black_box(result.to_markdown());
    }
    let start = Instant::now();
    let before_cpu = cpu::seconds();
    let mut output_bytes = 0usize;
    for _ in 0..repeats {
        let output = result.to_markdown();
        output_bytes += output.text.len();
        black_box(output);
    }
    println!(
        "{}",
        serde_json::json!({"repeats": repeats, "wall_seconds": start.elapsed().as_secs_f64(), "cpu_seconds": cpu::elapsed(before_cpu), "output_bytes": output_bytes})
    );
    Ok(())
}
