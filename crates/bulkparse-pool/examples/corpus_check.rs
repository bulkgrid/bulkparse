//! Functional complete-object check of the default process host, not a timing leaderboard.
use bulkparse::Options;
use bulkparse_pool::{Pool, PoolConfig};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::Write;
struct HashWriter(Sha256);
impl Write for HashWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
use std::path::PathBuf;
use std::time::Duration;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 4 {
        return Err("usage: corpus_check WORKER MANIFEST OUTPUT_DIR".into());
    }
    let out = PathBuf::from(&args[3]);
    std::fs::create_dir(&out)?;
    let corpus: Vec<Value> = serde_json::from_slice(&std::fs::read(&args[2])?)?;
    let config = PoolConfig::new(std::fs::canonicalize(&args[1])?);
    let limit = config.max_response_bytes;
    let pool = Pool::new(config)?;
    let mut rows = Vec::new();
    for doc in corpus {
        let path = std::fs::canonicalize(doc["path"].as_str().ok_or("path")?)?;
        let report = pool
            .try_submit(path, Options::default(), Duration::from_secs(90))?
            .wait()?;
        let md = report
            .conversion
            .as_ref()
            .map(|c| c.markdown.as_str())
            .unwrap_or("");
        std::fs::write(out.join(format!("{}.md", doc["id"].as_str().unwrap())), md)?;
        let json_sha256 = if let Some(conversion) = &report.conversion {
            let mut hash = HashWriter(Sha256::new());
            serde_json::to_writer(&mut hash, &conversion.result)?;
            Some(format!("{:x}", hash.0.finalize()))
        } else {
            None
        };
        let row = json!({"id":doc["id"],"status":report.status,"error":report.error,
            "returned":report.conversion.is_some(),"json_sha256":json_sha256,"top_level_blocks":report.conversion.as_ref().map(|c|c.result.document.blocks.len()),
            "diagnostics":report.conversion.as_ref().map(|c|&c.result.diagnostics),"execution_ms":report.execution_ms});
        println!("{}", row);
        rows.push(row);
    }
    pool.shutdown();
    let result = json!({"scope":"Full structured object returned through the default process pool. Per-job durations include IPC; not a parser timing comparison.",
        "max_response_bytes":limit,"returned":rows.iter().filter(|r|r["returned"]==true).count(),"documents":rows,"metrics":pool.metrics()});
    std::fs::write(
        out.join("results.json"),
        serde_json::to_vec_pretty(&result)?,
    )?;
    if result["returned"] != 112 {
        return Err("some pool conversions failed".into());
    }
    Ok(())
}
