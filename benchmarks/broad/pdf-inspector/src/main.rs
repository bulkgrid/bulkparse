use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut stdout = io::stdout().lock();
    for line in io::stdin().lock().lines() {
        let q: Value = serde_json::from_str(&line?)?;
        let out = q["output_dir"].as_str().map(Path::new);
        if let Some(out) = out { std::fs::create_dir_all(out)?; }
        let mut rows = vec![];
        let batch = Instant::now();
        for path in q["documents"].as_array().unwrap() {
            let path = path.as_str().unwrap();
            let id = Path::new(path).file_stem().unwrap().to_str().unwrap();
            let start = Instant::now();
            let result = std::panic::catch_unwind(|| pdf_inspector::process_pdf(path).map_err(|e|e.to_string()))
                .unwrap_or_else(|_| Err("parser panic".into()));
            let (md, error, pages) = match result {
                Ok(r) => (r.markdown.unwrap_or_default(), None, r.pages_needing_ocr),
                Err(e) => (String::new(), Some(e), vec![]),
            };
            if let Some(out) = out { std::fs::write(out.join(format!("{id}.md")), &md)?; }
            rows.push(json!({"document_id":id,"path":path,"ok":error.is_none(),"error":error,
                "status":if pages.is_empty(){"returned"}else{"partial"},"ocr_pages":pages,
                "elapsed_ns":start.elapsed().as_nanos(),"markdown":if out.is_none(){md}else{String::new()}}));
        }
        serde_json::to_writer(&mut stdout, &json!({"elapsed_ns":batch.elapsed().as_nanos(),"documents":rows}))?;
        writeln!(stdout)?; stdout.flush()?;
    }
    Ok(())
}
