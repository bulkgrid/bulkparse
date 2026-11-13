//! Benchmark protocol only. The conversion implementation is in the core crate.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::{self, BufRead, Read, Seek, Write};
use std::time::Instant;
#[path = "../../../benchmarks/realworld/cpu.rs"]
mod cpu;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut stdout = io::stdout().lock();
    let mut parser = bulkparse::Parser::new(bulkparse::Options {
        partial_on_limit: std::env::args().any(|a| a == "--partial-on-limit"),
        extract_images: !std::env::args().any(|a| a == "--no-images"),
        ..Default::default()
    });
    let streaming = std::env::args().any(|a| a == "--stream-markdown");
    if std::env::args().any(|a| a == "--large-documents") {
        parser.options.limits = bulkparse::Limits::large_spreadsheets();
    }
    if std::env::args().any(|a| a == "--conservative-limits") {
        parser.options.limits = bulkparse::Limits::conservative();
    }
    for line in io::stdin().lock().lines() {
        let request: Value = serde_json::from_str(&line?)?;
        let mut rows = vec![];
        let batch = Instant::now();
        for path in request["documents"].as_array().unwrap() {
            let path = path.as_str().unwrap();
            let cpu_start = cpu::seconds();
            let start = Instant::now();
            if streaming {
                let mut file = tempfile::tempfile()?;
                let result = std::fs::read(path)
                    .map_err(bulkparse::Error::from)
                    .and_then(|b| parser.parse_to_markdown(&b, Some(path), &mut file));
                let ns = start.elapsed().as_nanos();
                let cpu_seconds = cpu::elapsed(cpu_start);
                // Readback is for benchmark verification/protocol transport, not
                // part of conversion timing. Lifetime RSS still includes it.
                file.rewind()?;
                let mut markdown = String::new();
                file.read_to_string(&mut markdown)?;
                rows.push(match result {
                    Ok(r) => json!({"path":path,"elapsed_ns":ns,"ok":true,"status":r.status,"diagnostics":r.diagnostics,"render_diagnostics":r.render_diagnostics,"markdown":markdown}),
                    Err(e) => json!({"path":path,"elapsed_ns":ns,"ok":false,"error":e.to_string(),"error_code":e.code(),"markdown":""}),
                });
                rows.last_mut().unwrap()["cpu_seconds"] = json!(cpu_seconds);
                continue;
            }
            let result = std::fs::read(path)
                .map_err(bulkparse::Error::from)
                .and_then(|b| parser.parse_bytes(&b, Some(path)))
                .map(|r| {
                    let md = r.to_markdown();
                    (r, md)
                });
            let ns = start.elapsed().as_nanos();
            let cpu_seconds = cpu::elapsed(cpu_start);
            let json_check = if std::env::args().any(|a| a == "--verify-json") {
                if let Ok((r, _)) = &result {
                    // Validate full structured serialization without copying a
                    // second JSON String into the benchmark protocol. Keep r
                    // alive throughout. This extra check is timed separately.
                    let check_start = Instant::now();
                    let mut file = tempfile::tempfile()?;
                    let mut buffered = io::BufWriter::with_capacity(64 * 1024, &mut file);
                    serde_json::to_writer(&mut buffered, r)?;
                    buffered.flush()?;
                    drop(buffered);
                    let bytes = file.metadata()?.len();
                    file.rewind()?;
                    serde_json::from_reader::<_, serde::de::IgnoredAny>(io::BufReader::new(
                        &mut file,
                    ))?;
                    file.rewind()?;
                    let mut hash = Sha256::new();
                    let mut chunk = [0u8; 64 * 1024];
                    loop {
                        let n = file.read(&mut chunk)?;
                        if n == 0 {
                            break;
                        }
                        hash.update(&chunk[..n]);
                    }
                    let digest: String =
                        hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
                    Some(
                        json!({"bytes":bytes,"sha256":digest,"valid":true,"serialization_and_validation_ns":check_start.elapsed().as_nanos()}),
                    )
                } else {
                    None
                }
            } else {
                None
            };
            rows.push(match result{Ok((r,md))=>json!({"path":path,"elapsed_ns":ns,"ok":true,"status":r.status,"diagnostics":r.diagnostics,"render_diagnostics":md.diagnostics,"markdown":md.text,"image_count":r.document.assets.len(),"image_bytes":r.document.assets.iter().map(|a| a.data.len()).sum::<usize>(),"image_assets":r.document.assets.iter().map(|a|json!({"id":a.id,"media_type":a.media_type,"bytes":a.data.len(),"source":a.source})).collect::<Vec<_>>()}),Err(e)=>json!({"path":path,"elapsed_ns":ns,"ok":false,"error":e.to_string(),"error_code":e.code(),"markdown":""})});
            rows.last_mut().unwrap()["cpu_seconds"] = json!(cpu_seconds);
            if let Some(check) = json_check {
                rows.last_mut().unwrap()["json_check"] = check;
            }
        }
        serde_json::to_writer(
            &mut stdout,
            &json!({"wall_ns":batch.elapsed().as_nanos(),"documents":rows}),
        )?;
        writeln!(stdout)?;
        stdout.flush()?;
    }
    Ok(())
}
