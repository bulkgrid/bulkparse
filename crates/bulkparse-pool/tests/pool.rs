use bulkparse::Options;
use bulkparse_pool::{JobError, JobStatus, Pool, PoolConfig, SubmitError};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(5);
static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "bulkparse-pool-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn until(mut predicate: impl FnMut() -> bool) {
    let end = Instant::now() + WAIT;
    while !predicate() {
        assert!(Instant::now() < end, "condition did not become true");
        thread::sleep(Duration::from_millis(5));
    }
}

fn real_config() -> PoolConfig {
    let mut config = PoolConfig::new(env!("CARGO_BIN_EXE_bulkparse-worker"));
    config.workers = 1;
    config.queue_capacity = 4;
    config
}

fn fake_config() -> PoolConfig {
    let mut config = real_config();
    config.executable = "python3".into();
    config.worker_args = vec![Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fault_worker.py")
        .to_str()
        .unwrap()
        .into()];
    config
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
        .canonicalize()
        .unwrap()
}

fn assert_clean(pool: &Pool) {
    let pids = pool.metrics().worker_pids;
    pool.shutdown();
    let m = pool.metrics();
    assert_eq!(m.active, 0);
    assert_eq!(m.queued, 0);
    assert!(m.worker_pids.is_empty());
    assert_eq!(m.worker_starts, m.worker_stops);
    assert_eq!(m.accepted, m.complete + m.partial + m.failed);
    #[cfg(unix)]
    for pid in pids {
        assert!(
            !std::process::Command::new("kill")
                .args(["-0", &pid.to_string()])
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap()
                .success(),
            "worker still alive"
        );
    }
    #[cfg(not(unix))]
    let _ = pids;
}

#[test]
fn real_worker_preserves_conversion_and_partial_status() {
    let scratch = Scratch::new();
    std::fs::write(scratch.path("plain.txt"), "Plain text").unwrap();
    std::fs::write(scratch.path("tabular.tsv"), "a\tb\nx\ty").unwrap();
    let pool = Pool::new(real_config()).unwrap();
    let mut paths: Vec<_> = [
        "hello.docx",
        "table.csv",
        "hello.md",
        "notebook.ipynb",
        "native-and-empty.pdf",
    ]
    .into_iter()
    .map(fixture)
    .collect();
    paths.extend([scratch.path("plain.txt"), scratch.path("tabular.tsv")]);
    for path in paths {
        let name = path.file_name().unwrap().to_str().unwrap();
        let direct = bulkparse::parse(
            &std::fs::read(&path).unwrap(),
            Some(name),
            Options::default(),
        )
        .unwrap();
        let report = pool
            .try_submit(&path, Options::default(), WAIT)
            .unwrap()
            .wait()
            .unwrap();
        assert_eq!(
            serde_json::to_value(report.conversion.unwrap()).unwrap(),
            serde_json::to_value(direct).unwrap()
        );
        assert_eq!(
            report.status,
            if name.ends_with("pdf") {
                JobStatus::Partial
            } else {
                JobStatus::Complete
            }
        );
        assert!(report.total_ms >= report.queue_ms);
    }
    let strict = Options {
        strict: true,
        ..Default::default()
    };
    let report = pool
        .try_submit(fixture("native-and-empty.pdf"), strict, WAIT)
        .unwrap()
        .wait()
        .unwrap();
    assert!(
        matches!(report.error, Some(JobError::Conversion { code, .. }) if code == "incomplete")
    );
    assert_eq!(pool.metrics().worker_starts, 1);
    assert_clean(&pool);
}

#[test]
fn queue_full_queued_deadline_and_cancellation_do_not_wait_for_active_job() {
    let scratch = Scratch::new();
    let mut config = fake_config();
    config.queue_capacity = 1;
    let pool = Pool::new(config).unwrap();
    let active = pool
        .try_submit(scratch.path("hang"), Options::default(), WAIT)
        .unwrap();
    until(|| scratch.path("hang.started").exists());
    let queued = pool
        .try_submit(
            scratch.path("ok"),
            Options::default(),
            Duration::from_millis(100),
        )
        .unwrap();
    assert!(matches!(
        pool.try_submit(scratch.path("extra"), Options::default(), WAIT),
        Err(SubmitError::QueueFull)
    ));
    let report = queued.wait_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(report.error, Some(JobError::DeadlineExceeded));
    assert_eq!(report.execution_ms, 0.0);
    let queued = pool
        .try_submit(scratch.path("ok"), Options::default(), WAIT)
        .unwrap();
    queued.cancel();
    assert_eq!(
        queued.wait_timeout(Duration::from_secs(1)).unwrap().error,
        Some(JobError::Cancelled)
    );
    assert_eq!(pool.metrics().active, 1);
    active.cancel();
    assert_eq!(active.wait().unwrap().error, Some(JobError::Cancelled));
    assert_eq!(pool.metrics().rejected_full, 1);
    assert_clean(&pool);
}

#[test]
fn active_deadline_kills_worker_and_next_job_gets_replacement() {
    let scratch = Scratch::new();
    let mut config = fake_config();
    // Starting Python on a loaded runner can exceed the active-job deadline.
    // Deliberately exercise slow startup, then test an already-running worker.
    config.worker_args.push("--slow-ready".into());
    let pool = Pool::new(config).unwrap();
    let warmup = pool
        .try_submit(scratch.path("ok"), Options::default(), WAIT)
        .unwrap()
        .wait()
        .unwrap();
    assert!(
        matches!(warmup.error, Some(JobError::Conversion { code, .. }) if code == "fixture_ok")
    );
    let hang = pool
        .try_submit(
            scratch.path("hang"),
            Options::default(),
            Duration::from_millis(300),
        )
        .unwrap();
    until(|| scratch.path("hang.started").exists());
    let pid = pool.metrics().worker_pids[0];
    assert_eq!(hang.wait().unwrap().error, Some(JobError::DeadlineExceeded));
    let next = pool
        .try_submit(scratch.path("ok"), Options::default(), WAIT)
        .unwrap()
        .wait()
        .unwrap();
    assert!(matches!(next.error, Some(JobError::Conversion { code, .. }) if code == "fixture_ok"));
    assert_ne!(pool.metrics().worker_pids[0], pid);
    assert_eq!(pool.metrics().worker_starts, 2);
    assert_clean(&pool);
}

#[test]
fn cross_thread_cancellation_and_dropped_handle_release_capacity() {
    let scratch = Scratch::new();
    let pool = Pool::new(fake_config()).unwrap();
    let hang = pool
        .try_submit(scratch.path("hang"), Options::default(), WAIT)
        .unwrap();
    until(|| scratch.path("hang.started").exists());
    let cancel = hang.cancellation_handle();
    thread::spawn(move || cancel.cancel()).join().unwrap();
    assert_eq!(hang.wait().unwrap().error, Some(JobError::Cancelled));
    std::fs::remove_file(scratch.path("hang.started")).unwrap();
    let hang = pool
        .try_submit(scratch.path("hang"), Options::default(), WAIT)
        .unwrap();
    until(|| scratch.path("hang.started").exists());
    drop(hang);
    until(|| pool.metrics().cancelled == 2);
    assert_clean(&pool);
}

#[test]
fn crashes_and_invalid_frames_fail_once_and_recover() {
    let scratch = Scratch::new();
    let pool = Pool::new(fake_config()).unwrap();
    for name in ["crash", "oversize", "bad-json", "wrong-id"] {
        let bad = pool
            .try_submit(scratch.path(name), Options::default(), WAIT)
            .unwrap()
            .wait()
            .unwrap();
        assert!(matches!(bad.error, Some(JobError::Worker { .. })));
        let good = pool
            .try_submit(scratch.path("ok"), Options::default(), WAIT)
            .unwrap()
            .wait()
            .unwrap();
        assert!(
            matches!(good.error, Some(JobError::Conversion { code, .. }) if code == "fixture_ok")
        );
    }
    assert_eq!(pool.metrics().worker_failures, 4);
    assert_clean(&pool);
}

#[test]
fn startup_errors_are_bounded_and_shutdown_is_idempotent() {
    let scratch = Scratch::new();
    for mode in ["--no-ready", "--bad-ready", "missing-executable"] {
        let mut config = fake_config();
        config.startup_timeout = Duration::from_millis(200);
        if mode == "missing-executable" {
            config.executable = scratch.path(mode);
        } else {
            config.worker_args.push(mode.into());
        }
        let pool = Pool::new(config).unwrap();
        let job = pool
            .try_submit(scratch.path("ok"), Options::default(), WAIT)
            .unwrap();
        assert!(matches!(
            job.wait_timeout(Duration::from_secs(2)).unwrap().error,
            Some(JobError::Worker { .. })
        ));
        assert_clean(&pool);
        pool.shutdown();
        assert!(matches!(
            pool.try_submit(scratch.path("ok"), Options::default(), WAIT),
            Err(SubmitError::Closed)
        ));
    }
}

#[test]
fn shutdown_completes_active_and_queued_reports_and_reaps() {
    let scratch = Scratch::new();
    let pool = Pool::new(fake_config()).unwrap();
    let active = pool
        .try_submit(scratch.path("hang"), Options::default(), WAIT)
        .unwrap();
    until(|| scratch.path("hang.started").exists());
    let queued = pool
        .try_submit(scratch.path("ok"), Options::default(), WAIT)
        .unwrap();
    assert_clean(&pool);
    assert_eq!(active.wait().unwrap().error, Some(JobError::Shutdown));
    assert_eq!(queued.wait().unwrap().error, Some(JobError::Shutdown));
}

#[test]
fn output_and_input_caps_are_explicit_failures_and_worker_remains_usable() {
    let scratch = Scratch::new();
    std::fs::write(scratch.path("large.txt"), "hello ".repeat(1000)).unwrap();
    let mut config = real_config();
    config.max_response_bytes = 1024;
    let pool = Pool::new(config).unwrap();
    let report = pool
        .try_submit(scratch.path("large.txt"), Options::default(), WAIT)
        .unwrap()
        .wait()
        .unwrap();
    assert!(
        matches!(report.error, Some(JobError::Conversion { code, .. }) if code == "output_limit")
    );
    std::fs::write(scratch.path("tiny.txt"), "hello").unwrap();
    let report = pool
        .try_submit(scratch.path("tiny.txt"), Options::default(), WAIT)
        .unwrap()
        .wait()
        .unwrap();
    assert_eq!(report.status, JobStatus::Complete);
    assert_clean(&pool);
    let mut config = real_config();
    config.max_input_bytes = 8;
    let pool = Pool::new(config).unwrap();
    let report = pool
        .try_submit(scratch.path("large.txt"), Options::default(), WAIT)
        .unwrap()
        .wait()
        .unwrap();
    assert!(
        matches!(report.error, Some(JobError::Conversion { code, .. }) if code == "resource_limit")
    );
    assert_clean(&pool);
}

#[test]
fn recycling_and_concurrent_admission_keep_counts_and_bounds() {
    let mut config = real_config();
    config.workers = 2;
    config.queue_capacity = 2;
    config.max_jobs_per_worker = 3;
    let pool = Pool::new(config).unwrap();
    thread::scope(|scope| {
        for _ in 0..4 {
            let pool = &pool;
            scope.spawn(move || {
                for _ in 0..8 {
                    let job = loop {
                        match pool.try_submit(fixture("hello.md"), Options::default(), WAIT) {
                            Ok(job) => break job,
                            Err(SubmitError::QueueFull) => thread::yield_now(),
                            Err(error) => panic!("{error}"),
                        }
                    };
                    assert_eq!(job.wait().unwrap().status, JobStatus::Complete);
                }
            });
        }
    });
    let m = pool.metrics();
    assert_eq!(m.complete, 32);
    assert!(m.worker_starts >= 11);
    assert!(m.peak_active <= 2 && m.peak_queued <= 2);
    assert_clean(&pool);
}
