//! One lazy supervised model worker per provider. No page/result cache.
use super::{decode_response, Config, OwnedChild, Response};
use bulkparse::{Error, PdfOcrOutput, PdfOcrRequest};
use serde::Deserialize;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};
use std::sync::{mpsc, Mutex, TryLockError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

struct Exchange {
    request: Vec<u8>,
    wire_limit: usize,
}

pub(super) struct Worker {
    child: Option<OwnedChild>,
    commands: Option<mpsc::SyncSender<Exchange>>,
    replies: mpsc::Receiver<std::io::Result<Vec<u8>>>,
    io: Option<JoinHandle<()>>,
    pages: u32,
}

impl Drop for Worker {
    fn drop(&mut self) {
        // Kill before joining: both a blocked pipe write and read then unblock.
        drop(self.child.take());
        drop(self.commands.take());
        if let Some(io) = self.io.take() {
            let _ = io.join();
        }
    }
}

impl Worker {
    fn start(config: &Config) -> Result<Self, Error> {
        let mut command = Command::new(&config.executable);
        command
            .args(&config.args)
            .arg("--worker")
            .arg("--models")
            .arg(&config.models)
            .arg("--dpi")
            .arg(config.dpi.to_string())
            .arg("--max-pixels")
            .arg(config.max_pixels.to_string())
            .arg("--max-input-bytes")
            .arg(config.max_input_bytes.to_string())
            .env("OMP_NUM_THREADS", "1")
            .env("OPENBLAS_NUM_THREADS", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = OwnedChild(command.spawn()?);
        let mut stdin = child.0.stdin.take().unwrap();
        let mut stdout = BufReader::new(child.0.stdout.take().unwrap());
        let (commands, work) = mpsc::sync_channel::<Exchange>(1);
        let (completed, replies) = mpsc::sync_channel(1);
        let io = thread::Builder::new()
            .name("bulkparse-ocr-worker".into())
            .spawn(move || {
                while let Ok(exchange) = work.recv() {
                    let result = (|| {
                        stdin.write_all(&exchange.request)?;
                        stdin.flush()?;
                        let mut bytes = Vec::new();
                        (&mut stdout)
                            .take(exchange.wire_limit as u64 + 1)
                            .read_until(b'\n', &mut bytes)?;
                        Ok(bytes)
                    })();
                    // There is one in-flight request. Never block teardown on a reply.
                    if completed.try_send(result).is_err() {
                        break;
                    }
                }
            })?;
        Ok(Self {
            child: Some(child),
            commands: Some(commands),
            replies,
            io: Some(io),
            pages: 0,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    id: u64,
    response: Response,
}

pub(super) fn recognize(
    config: &Config,
    state: &Mutex<Option<Worker>>,
    request: PdfOcrRequest<'_>,
    output_limit: usize,
    wire_limit: usize,
    deadline: Instant,
) -> Result<PdfOcrOutput, Error> {
    let check = || {
        if request.cancellation.is_cancelled() {
            return Err(Error::Cancelled);
        }
        if Instant::now() >= deadline {
            return Err(Error::Malformed("OCR page timeout".into()));
        }
        Ok(())
    };
    let mut slot = loop {
        check()?;
        match state.try_lock() {
            Ok(slot) => break slot,
            Err(TryLockError::WouldBlock) => thread::sleep(Duration::from_millis(5)),
            Err(TryLockError::Poisoned(_)) => {
                return Err(Error::Malformed("OCR worker lock poisoned".into()))
            }
        }
    };
    if slot
        .as_ref()
        .is_some_and(|worker| worker.pages >= config.max_pages_per_worker)
    {
        *slot = None;
    }
    // Keep the temporary PDF alive until a failed worker has been killed/reaped.
    let mut input = tempfile::NamedTempFile::new()?;
    let result = (|| {
        input.write_all(request.document)?;
        input.flush()?;
        check()?;
        if slot.is_none() {
            *slot = Some(Worker::start(config)?);
        }
        let worker = slot.as_mut().unwrap();
        worker.pages += 1;
        let id = u64::from(worker.pages);
        let mut bytes = serde_json::to_vec(&serde_json::json!({
            "version": 1, "id": id, "input": input.path(), "page": request.page,
            "max_output_bytes": output_limit,
        }))
        .map_err(|e| Error::Malformed(format!("OCR request: {e}")))?;
        bytes.push(b'\n');
        if bytes.len() > 16384 {
            return Err(Error::Limit("ocr_request_bytes"));
        }
        worker
            .commands
            .as_ref()
            .unwrap()
            .try_send(Exchange {
                request: bytes,
                wire_limit,
            })
            .map_err(|_| Error::Malformed("OCR worker stopped".into()))?;
        loop {
            check()?;
            match worker.replies.try_recv() {
                Ok(Ok(bytes)) => {
                    if bytes.len() > wire_limit {
                        return Err(Error::Limit("ocr_response_bytes"));
                    }
                    if !bytes.ends_with(b"\n") {
                        return Err(Error::Malformed("incomplete OCR worker frame".into()));
                    }
                    let reply: Reply = serde_json::from_slice(&bytes)
                        .map_err(|e| Error::Malformed(format!("OCR worker response: {e}")))?;
                    if reply.id != id {
                        return Err(Error::Malformed("OCR worker request id mismatch".into()));
                    }
                    return decode_response(reply.response, request.page, output_limit);
                }
                Ok(Err(e)) => return Err(Error::Io(e)),
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err(Error::Malformed("OCR worker output stopped".into()))
                }
                Err(mpsc::TryRecvError::Empty) => thread::sleep(Duration::from_millis(5)),
            }
        }
    })();
    // Failed inference/framing/budgets must never leave stale output for another request.
    if result.is_err() || request.cancellation.is_cancelled() {
        *slot = None;
    }
    if request.cancellation.is_cancelled() {
        return Err(Error::Cancelled);
    }
    result
}
