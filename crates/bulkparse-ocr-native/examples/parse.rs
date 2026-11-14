//! Development runner: parse CONFIG.json INPUT [INPUT ...], reusing native models.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let config = args.next().ok_or("expected configuration path")?;
    let engine = bulkparse_ocr_native::NativeOcr::from_json(&std::fs::read(config)?)?;
    for path in args {
        let start = std::time::Instant::now();
        let result = engine.parse_json(&std::fs::read(&path)?, Some(&path), None, None);
        match result {
            Ok(result) => println!(
                "{}",
                serde_json::json!({"path":path,"seconds":start.elapsed().as_secs_f64(),"result":serde_json::from_str::<serde_json::Value>(&result)?})
            ),
            Err(error) => return Err(format!("{path}: {error}").into()),
        }
    }
    Ok(())
}
