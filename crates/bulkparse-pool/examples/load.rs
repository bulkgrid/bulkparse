//! Local deployment exercise, not a competitor benchmark. See docs/worker-pool.md.
use bulkparse::Options;
use bulkparse_pool::{JobHandle, JobStatus, Pool, PoolConfig, SubmitError};
use serde_json::json;
use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn percentile(values: &mut [f64], p: f64) -> f64 {
    values.sort_by(f64::total_cmp);
    values[((values.len() as f64 * p).ceil() as usize).saturating_sub(1)]
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 5 {
        return Err("usage: load WORKER MANIFEST SECONDS WORKERS".into());
    }
    let paths: Vec<PathBuf> = serde_json::from_slice(&std::fs::read(&args[2])?)?;
    if paths.is_empty() || paths.iter().any(|p| !p.is_absolute()) {
        return Err("manifest must contain absolute input paths".into());
    }
    let seconds: u64 = args[3].parse()?;
    if !(1..=1800).contains(&seconds) {
        return Err("duration must be 1..1800 seconds".into());
    }
    let mut config = PoolConfig::new(&args[1]);
    config.workers = args[4].parse()?;
    config.queue_capacity = config.workers * 2;
    let window = config.workers;
    let pool = Pool::new(config)?;
    let start = Instant::now();
    let duration = Duration::from_secs(seconds);
    let mut last_metrics = start;
    let mut last_pids = vec![];
    let mut pending: VecDeque<(usize, JobHandle)> = VecDeque::new();
    let mut submitted = 0usize;
    let mut completed = 0usize;
    let mut statuses = [0u64; 3];
    let mut latencies = vec![];
    let mut queue_times = vec![];
    let mut execution_times = vec![];
    let mut outcomes: BTreeMap<String, BTreeMap<String, u64>> = BTreeMap::new();
    println!(
        "{}",
        json!({"event":"start","pid":std::process::id(),"seconds":seconds,"workers":window,"inputs":paths.len()})
    );
    loop {
        while pending.len() < window && (start.elapsed() < duration || submitted == 0) {
            let index = submitted % paths.len();
            match pool.try_submit(&paths[index], Options::default(), Duration::from_secs(30)) {
                Ok(job) => {
                    pending.push_back((index, job));
                    submitted += 1;
                }
                Err(SubmitError::QueueFull) => break,
                Err(error) => return Err(error.into()),
            }
        }
        // Collect whichever job finishes first so a slow document cannot block
        // replenishing other slots. The number of retained handles stays bounded.
        let mut ready = None;
        for _ in 0..pending.len() {
            let (index, job) = pending.pop_front().unwrap();
            match job.wait_timeout(Duration::ZERO) {
                Ok(report) => {
                    ready = Some((index, report));
                    break;
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => pending.push_back((index, job)),
                Err(error) => return Err(error.into()),
            }
        }
        let Some((index, report)) = ready else {
            if pending.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
            continue;
        };
        completed += 1;
        latencies.push(report.total_ms);
        queue_times.push(report.queue_ms);
        execution_times.push(report.execution_ms);
        let status = match report.status {
            JobStatus::Complete => {
                statuses[0] += 1;
                "complete"
            }
            JobStatus::Partial => {
                statuses[1] += 1;
                "partial"
            }
            JobStatus::Failed => {
                statuses[2] += 1;
                "failed"
            }
        };
        let label = paths[index]
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();
        *outcomes
            .entry(label)
            .or_default()
            .entry(status.into())
            .or_default() += 1;
        let current_metrics = pool.metrics();
        if last_metrics.elapsed() >= Duration::from_secs(1)
            || current_metrics.worker_pids != last_pids
        {
            println!(
                "{}",
                json!({"event":"metrics","elapsed_seconds":start.elapsed().as_secs_f64(),"metrics":current_metrics})
            );
            last_pids = current_metrics.worker_pids;
            last_metrics = Instant::now();
        }
    }
    let elapsed = start.elapsed().as_secs_f64();
    pool.shutdown();
    let metrics = pool.metrics();
    assert_eq!(submitted, completed);
    assert_eq!(
        metrics.accepted,
        metrics.complete + metrics.partial + metrics.failed
    );
    assert!(metrics.worker_pids.is_empty() && metrics.active == 0 && metrics.queued == 0);
    assert_eq!(metrics.worker_starts, metrics.worker_stops);
    println!(
        "{}",
        json!({"event":"summary","elapsed_seconds":elapsed,"attempts":completed,
        "attempts_per_second":completed as f64/elapsed,"complete":statuses[0],"partial":statuses[1],"failed":statuses[2],
        "total_p50_ms":percentile(&mut latencies,0.5),"total_p95_ms":percentile(&mut latencies,0.95),
        "queue_p95_ms":percentile(&mut queue_times,0.95),"execution_p95_ms":percentile(&mut execution_times,0.95),
        "outcomes":outcomes,"metrics":metrics})
    );
    Ok(())
}
