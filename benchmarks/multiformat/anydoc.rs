//! Independent competitor worker, never a dependency of BulkParse.
use serde_json::{json,Value};
use std::io::{self,BufRead,Write};
use std::time::Instant;
#[path="../realworld/cpu.rs"]
mod cpu;
fn main()->Result<(),Box<dyn std::error::Error>>{
 let mut stdout=io::stdout().lock();for line in io::stdin().lock().lines(){let q:Value=serde_json::from_str(&line?)?;let mut rows=vec![];let batch=Instant::now();
 for path in q["documents"].as_array().unwrap(){let path=path.as_str().unwrap();let cpu_start=cpu::seconds();let start=Instant::now();let r=std::panic::catch_unwind(||{let bytes=std::fs::read(path).map_err(|e|e.to_string())?;let f=anydoc::Format::from_bytes(&bytes).or_else(||anydoc::Format::from_path(std::path::Path::new(path))).ok_or("unknown format")?;anydoc::to_markdown_bytes(&bytes,f).map_err(|e|e.to_string())}).unwrap_or_else(|_|Err("parser panic".into()));let ns=start.elapsed().as_nanos();let cpu_seconds=cpu::elapsed(cpu_start);rows.push(match r{Ok(md)=>json!({"path":path,"elapsed_ns":ns,"cpu_seconds":cpu_seconds,"ok":true,"markdown":md}),Err(e)=>json!({"path":path,"elapsed_ns":ns,"cpu_seconds":cpu_seconds,"ok":false,"error":e,"markdown":""})});}
 serde_json::to_writer(&mut stdout,&json!({"wall_ns":batch.elapsed().as_nanos(),"documents":rows}))?;writeln!(stdout)?;stdout.flush()?;}Ok(())
}
