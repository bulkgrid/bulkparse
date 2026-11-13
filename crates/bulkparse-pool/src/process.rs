use crate::protocol::{self, Reply};
use crate::{interruption, Job, JobError, PoolConfig, Shared, POLL};
use bulkparse::Conversion;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub struct Worker {
    child: Child,
    requests: Option<SyncSender<Vec<u8>>>,
    replies: Receiver<Result<Reply, String>>,
    io: Option<JoinHandle<()>>,
}

fn failure(message: impl ToString) -> JobError {
    JobError::Worker {
        message: message.to_string(),
    }
}

impl Worker {
    pub fn spawn(config: &PoolConfig) -> Result<Self, JobError> {
        let mut child = Command::new(&config.executable)
            .args(&config.worker_args)
            .arg("--max-response-bytes")
            .arg(config.max_response_bytes.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(failure)?;
        let mut input = child.stdin.take().unwrap();
        let mut output = child.stdout.take().unwrap();
        let (requests, jobs) = mpsc::sync_channel::<Vec<u8>>(1);
        let (events, replies) = mpsc::sync_channel(1);
        let limit = config.max_response_bytes;
        let io = thread::Builder::new()
            .name("bulkparse-pipe".into())
            .spawn(move || {
                let mut read = || -> Result<Reply, String> {
                    protocol::read_reply(&mut output, limit).map_err(|e| e.to_string())
                };
                let ready = read();
                let failed = ready.is_err();
                if events.send(ready).is_err() || failed {
                    return;
                }
                while let Ok(job) = jobs.recv() {
                    let reply = protocol::write_frame(&mut input, &job)
                        .map_err(|e| e.to_string())
                        .and_then(|()| read());
                    let failed = reply.is_err();
                    if events.send(reply).is_err() || failed {
                        break;
                    }
                }
            });
        match io {
            Ok(io) => Ok(Self {
                child,
                requests: Some(requests),
                replies,
                io: Some(io),
            }),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                Err(failure(error))
            }
        }
    }

    pub fn pid(&self) -> u32 {
        self.child.id()
    }
    pub fn exited(&mut self) -> bool {
        !matches!(self.child.try_wait(), Ok(None))
    }

    fn receive(&self, shared: &Shared, job: &Job, deadline: Instant) -> Result<Reply, JobError> {
        loop {
            if let Some(error) = interruption(job, shared.state.lock().unwrap().closed) {
                return Err(error);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(if Instant::now() >= job.deadline {
                    JobError::DeadlineExceeded
                } else {
                    failure("worker startup timeout")
                });
            }
            match self.replies.recv_timeout(POLL.min(remaining)) {
                Ok(reply) => {
                    // Do not accept a reply that crossed the end-to-end deadline.
                    if let Some(error) = interruption(job, shared.state.lock().unwrap().closed) {
                        return Err(error);
                    }
                    if Instant::now() >= deadline {
                        return Err(failure("worker startup timeout"));
                    }
                    return reply.map_err(failure);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => (),
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(failure("worker pipe closed"))
                }
            }
        }
    }

    pub fn ready(&mut self, shared: &Shared, job: &Job, timeout: Duration) -> Result<(), JobError> {
        let deadline = Instant::now()
            .checked_add(timeout)
            .unwrap_or(job.deadline)
            .min(job.deadline);
        match self.receive(shared, job, deadline)? {
            Reply::Ready {
                version: protocol::VERSION,
            } => Ok(()),
            _ => Err(failure("invalid worker handshake")),
        }
    }

    pub fn parse(&mut self, shared: &Shared, job: &Job) -> Result<Box<Conversion>, JobError> {
        if let Some(error) = interruption(job, shared.state.lock().unwrap().closed) {
            return Err(error);
        }
        self.requests
            .as_ref()
            .unwrap()
            .try_send(job.request.clone())
            .map_err(failure)?;
        match self.receive(shared, job, job.deadline)? {
            Reply::Converted { id, conversion } if id == job.id => Ok(conversion),
            Reply::Failed { id, code, message } if id == job.id => {
                Err(JobError::Conversion { code, message })
            }
            _ => Err(failure("unexpected worker response or job ID")),
        }
    }

    pub fn stop(&mut self) {
        // The bundled worker never spawns descendants. An external wrapper must
        // exec the worker, not fork descendants that inherit these pipes.
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.requests.take();
        if let Some(io) = self.io.take() {
            let _ = io.join();
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop();
    }
}
