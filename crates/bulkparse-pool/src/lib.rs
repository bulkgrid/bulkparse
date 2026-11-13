//! Optional native process host. The portable parser has no dependency on this crate.
//!
//! Jobs refer to immutable files owned by the host. Keep each file until its report
//! arrives. Deadlines include queueing and startup. Completed reports belong to the
//! caller: drain/drop handles to avoid retaining arbitrarily many large results.
#![forbid(unsafe_code)]

mod process;
#[allow(dead_code)] // Protocol helpers are shared with the separately built child endpoint.
mod protocol;

use bulkparse::{Conversion, Options, Status};
use serde::Serialize;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const POLL: Duration = Duration::from_millis(10);

/// The executable is trusted, fixed host configuration, never a document option.
#[derive(Debug, Clone)]
pub struct PoolConfig {
    pub executable: PathBuf,
    pub worker_args: Vec<String>,
    pub workers: usize,
    pub queue_capacity: usize,
    pub startup_timeout: Duration,
    pub max_response_bytes: usize,
    pub max_input_bytes: usize,
    /// Recycle a process after this many attempts to limit accumulated state.
    pub max_jobs_per_worker: usize,
}

impl PoolConfig {
    pub fn new(executable: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
            worker_args: vec![],
            workers: 1,
            queue_capacity: 32,
            startup_timeout: Duration::from_secs(10),
            max_response_bytes: protocol::MAX_RESPONSE,
            max_input_bytes: 32 * 1024 * 1024,
            max_jobs_per_worker: 1000,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SubmitError {
    #[error("pool is closed")]
    Closed,
    #[error("waiting queue is full")]
    QueueFull,
    #[error("invalid job: {0}")]
    Invalid(String),
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JobError {
    Cancelled,
    DeadlineExceeded,
    Shutdown,
    Worker { message: String },
    Conversion { code: String, message: String },
}

#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Complete,
    Partial,
    Failed,
}

#[derive(Debug, Serialize)]
pub struct JobReport {
    pub id: u64,
    pub status: JobStatus,
    pub conversion: Option<Box<Conversion>>,
    pub error: Option<JobError>,
    pub queue_ms: f64,
    /// Includes process startup/replacement and IPC when needed, not just parsing.
    pub execution_ms: f64,
    pub total_ms: f64,
}

/// Cumulative counters plus current gauges. Resource sampling belongs to the host;
/// worker_pids allows an OS monitor to collect RSS/CPU without a core dependency.
#[derive(Debug, Serialize, Clone, Default)]
pub struct Metrics {
    pub accepted: u64,
    pub rejected_full: u64,
    pub queued: usize,
    pub active: usize,
    pub peak_queued: usize,
    pub peak_active: usize,
    pub complete: u64,
    pub partial: u64,
    pub failed: u64,
    pub cancelled: u64,
    pub deadline_exceeded: u64,
    pub shutdown_jobs: u64,
    pub worker_failures: u64,
    pub worker_starts: u64,
    pub worker_stops: u64,
    pub queue_ms_total: f64,
    pub execution_ms_total: f64,
    pub worker_pids: Vec<u32>,
}

struct Job {
    id: u64,
    request: Vec<u8>,
    submitted: Instant,
    deadline: Instant,
    cancelled: Arc<AtomicBool>,
    reply: mpsc::SyncSender<JobReport>,
}

struct State {
    queue: VecDeque<Job>,
    closed: bool,
    next_id: u64,
    metrics: Metrics,
}

struct Shared {
    state: Mutex<State>,
    wake: Condvar,
}

pub struct JobHandle {
    id: u64,
    receiver: mpsc::Receiver<JobReport>,
    cancelled: Arc<AtomicBool>,
    shared: Arc<Shared>,
}

/// Cloneable cancellation signal for another thread or request handler.
#[derive(Clone)]
pub struct CancelHandle {
    cancelled: Arc<AtomicBool>,
    shared: Arc<Shared>,
}

impl CancelHandle {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.shared.wake.notify_all();
    }
}

impl JobHandle {
    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn cancellation_handle(&self) -> CancelHandle {
        CancelHandle {
            cancelled: self.cancelled.clone(),
            shared: self.shared.clone(),
        }
    }

    /// Idempotent. Completion may win a race with cancellation. Wait for the
    /// terminal report before removing input files or reusing external resources.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.shared.wake.notify_all();
    }

    pub fn wait(self) -> Result<JobReport, mpsc::RecvError> {
        self.receiver.recv()
    }

    pub fn wait_timeout(&self, timeout: Duration) -> Result<JobReport, mpsc::RecvTimeoutError> {
        self.receiver.recv_timeout(timeout)
    }
}

impl Drop for JobHandle {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// One supervisor thread and one pipe-I/O thread per live process, plus one
/// queue-maintenance thread. No async runtime, network listener, or OCR provider.
pub struct Pool {
    shared: Arc<Shared>,
    config: PoolConfig,
    threads: Mutex<Vec<JoinHandle<()>>>,
}

impl Pool {
    pub fn new(config: PoolConfig) -> std::io::Result<Self> {
        if config.workers == 0
            || config.workers > 256
            || config.queue_capacity == 0
            || config.startup_timeout.is_zero()
            || config.max_jobs_per_worker == 0
            || config.max_input_bytes == 0
            || !(1024..=protocol::MAX_RESPONSE).contains(&config.max_response_bytes)
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "invalid pool limits",
            ));
        }
        let pool = Self {
            config: config.clone(),
            shared: Arc::new(Shared {
                state: Mutex::new(State {
                    queue: VecDeque::new(),
                    closed: false,
                    next_id: 1,
                    metrics: Metrics::default(),
                }),
                wake: Condvar::new(),
            }),
            threads: Mutex::new(vec![]),
        };
        // Drop shuts down already-created threads if a later spawn fails.
        for index in 0..config.workers {
            let shared = pool.shared.clone();
            let config = config.clone();
            let handle = thread::Builder::new()
                .name(format!("bulkparse-slot-{index}"))
                .spawn(move || supervise(shared, config))?;
            pool.threads.lock().unwrap().push(handle);
        }
        let shared = pool.shared.clone();
        let handle = thread::Builder::new()
            .name("bulkparse-queue".into())
            .spawn(move || maintain_queue(shared))?;
        pool.threads.lock().unwrap().push(handle);
        Ok(pool)
    }

    /// Nonblocking admission. The queue bound counts waiting jobs; up to
    /// `workers` additional jobs can be active. Timeout starts at this call.
    pub fn try_submit(
        &self,
        path: impl AsRef<Path>,
        mut options: Options,
        timeout: Duration,
    ) -> Result<JobHandle, SubmitError> {
        let submitted = Instant::now();
        let deadline = submitted
            .checked_add(timeout)
            .filter(|_| !timeout.is_zero())
            .ok_or_else(|| SubmitError::Invalid("timeout must be positive and finite".into()))?;
        let path = path.as_ref();
        if !path.is_absolute() || path.to_str().is_none() || path.as_os_str().len() > 8192 {
            return Err(SubmitError::Invalid(
                "use a bounded absolute UTF-8 input path".into(),
            ));
        }
        options.limits.max_input_bytes = options
            .limits
            .max_input_bytes
            .min(self.config.max_input_bytes);
        let mut state = self.shared.state.lock().unwrap();
        if state.closed {
            return Err(SubmitError::Closed);
        }
        if state.queue.len() >= self.config.queue_capacity {
            state.metrics.rejected_full += 1;
            return Err(SubmitError::QueueFull);
        }
        let id = state.next_id;
        let next = id
            .checked_add(1)
            .ok_or_else(|| SubmitError::Invalid("job IDs exhausted".into()))?;
        let request = protocol::encode(
            &protocol::Request {
                id,
                path: path.into(),
                options,
            },
            protocol::MAX_REQUEST,
        )
        .map_err(|e| SubmitError::Invalid(e.to_string()))?;
        state.next_id = next;
        let (reply, receiver) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        state.queue.push_back(Job {
            id,
            request,
            submitted,
            deadline,
            cancelled: cancelled.clone(),
            reply,
        });
        state.metrics.accepted += 1;
        state.metrics.queued = state.queue.len();
        state.metrics.peak_queued = state.metrics.peak_queued.max(state.queue.len());
        drop(state);
        self.shared.wake.notify_all();
        Ok(JobHandle {
            id,
            receiver,
            cancelled,
            shared: self.shared.clone(),
        })
    }

    pub fn metrics(&self) -> Metrics {
        self.shared.state.lock().unwrap().metrics.clone()
    }

    /// Stop admission, fail queued/active jobs, kill and reap children, join host
    /// threads. Idempotent. Completed reports are left available to their callers.
    pub fn shutdown(&self) {
        self.shared.state.lock().unwrap().closed = true;
        self.shared.wake.notify_all();
        let mut threads = self.threads.lock().unwrap();
        for thread in threads.drain(..) {
            let _ = thread.join();
        }
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn interruption(job: &Job, closed: bool) -> Option<JobError> {
    if closed {
        Some(JobError::Shutdown)
    } else if job.cancelled.load(Ordering::Acquire) {
        Some(JobError::Cancelled)
    } else if Instant::now() >= job.deadline {
        Some(JobError::DeadlineExceeded)
    } else {
        None
    }
}

fn finish(
    shared: &Shared,
    job: Job,
    started: Option<Instant>,
    result: Result<Box<Conversion>, JobError>,
) {
    let now = Instant::now();
    let queue_ms = (started.unwrap_or(now) - job.submitted).as_secs_f64() * 1000.0;
    let execution_ms = started.map_or(0.0, |s| (now - s).as_secs_f64() * 1000.0);
    let (status, conversion, error) = match result {
        Ok(conversion) => {
            let partial = conversion.result.status == Status::Partial
                || conversion.render_diagnostics.iter().any(|d| d.content_loss);
            (
                if partial {
                    JobStatus::Partial
                } else {
                    JobStatus::Complete
                },
                Some(conversion),
                None,
            )
        }
        Err(error) => (JobStatus::Failed, None, Some(error)),
    };
    let report = JobReport {
        id: job.id,
        status,
        conversion,
        error,
        queue_ms,
        execution_ms,
        total_ms: (now - job.submitted).as_secs_f64() * 1000.0,
    };
    {
        let mut state = shared.state.lock().unwrap();
        let m = &mut state.metrics;
        if started.is_some() {
            m.active -= 1;
        }
        m.queue_ms_total += queue_ms;
        m.execution_ms_total += execution_ms;
        match status {
            JobStatus::Complete => m.complete += 1,
            JobStatus::Partial => m.partial += 1,
            JobStatus::Failed => m.failed += 1,
        }
        match &report.error {
            Some(JobError::Cancelled) => m.cancelled += 1,
            Some(JobError::DeadlineExceeded) => m.deadline_exceeded += 1,
            Some(JobError::Shutdown) => m.shutdown_jobs += 1,
            Some(JobError::Worker { .. }) => m.worker_failures += 1,
            _ => (),
        }
    }
    let _ = job.reply.send(report);
}

fn maintain_queue(shared: Arc<Shared>) {
    loop {
        let mut state = shared.state.lock().unwrap();
        let mut removed = vec![];
        let mut keep = VecDeque::new();
        while let Some(job) = state.queue.pop_front() {
            if let Some(error) = interruption(&job, state.closed) {
                removed.push((job, error));
            } else {
                keep.push_back(job);
            }
        }
        state.queue = keep;
        state.metrics.queued = state.queue.len();
        let closed = state.closed;
        drop(state);
        for (job, error) in removed {
            finish(&shared, job, None, Err(error));
        }
        if closed {
            return;
        }
        let state = shared.state.lock().unwrap();
        if !state.closed {
            drop(shared.wake.wait_timeout(state, POLL).unwrap());
        }
    }
}

fn supervise(shared: Arc<Shared>, config: PoolConfig) {
    let mut worker: Option<process::Worker> = None;
    let mut attempts = 0;
    loop {
        let mut state = shared.state.lock().unwrap();
        while state.queue.is_empty() && !state.closed {
            state = shared.wake.wait(state).unwrap();
        }
        if state.closed {
            break;
        }
        let job = state.queue.pop_front().unwrap();
        state.metrics.queued = state.queue.len();
        if let Some(error) = interruption(&job, false) {
            drop(state);
            finish(&shared, job, None, Err(error));
            continue;
        }
        state.metrics.active += 1;
        state.metrics.peak_active = state.metrics.peak_active.max(state.metrics.active);
        drop(state);
        let started = Instant::now();
        let result = (|| {
            if worker.as_mut().is_some_and(|w| w.exited()) {
                stop_worker(&shared, &mut worker);
            }
            if worker.is_none() {
                let w = process::Worker::spawn(&config)?;
                let pid = w.pid();
                worker = Some(w);
                {
                    let mut state = shared.state.lock().unwrap();
                    state.metrics.worker_starts += 1;
                    state.metrics.worker_pids.push(pid);
                }
                attempts = 0;
                worker
                    .as_mut()
                    .unwrap()
                    .ready(&shared, &job, config.startup_timeout)?;
            }
            attempts += 1;
            worker.as_mut().unwrap().parse(&shared, &job)
        })();
        // Conversion failures are valid replies; transport/timeout/cancel failures
        // require disposal. Never retry an in-flight document automatically.
        let dispose = matches!(&result, Err(e) if !matches!(e, JobError::Conversion { .. }))
            || attempts >= config.max_jobs_per_worker;
        if dispose {
            stop_worker(&shared, &mut worker);
        }
        finish(&shared, job, Some(started), result);
    }
    stop_worker(&shared, &mut worker);
}

fn stop_worker(shared: &Shared, worker: &mut Option<process::Worker>) {
    if let Some(mut worker) = worker.take() {
        let pid = worker.pid();
        worker.stop();
        let mut state = shared.state.lock().unwrap();
        state.metrics.worker_pids.retain(|&p| p != pid);
        state.metrics.worker_stops += 1;
    }
}
