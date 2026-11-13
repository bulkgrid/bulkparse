use bulkparse::Options;
use bulkparse_pool::{Pool, PoolConfig};
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        return Err("usage: parse /absolute/path/bulkparse-worker /path/document".into());
    }
    let mut config = PoolConfig::new(&args[1]);
    config.workers = 2;
    config.queue_capacity = 8;
    let pool = Pool::new(config)?;
    let input = std::fs::canonicalize(&args[2])?;
    let job = pool.try_submit(input, Options::default(), Duration::from_secs(30))?;
    let report = job.wait()?;
    println!("{}", serde_json::to_string(&report)?);
    pool.shutdown();
    eprintln!("{}", serde_json::to_string(&pool.metrics())?);
    Ok(())
}
