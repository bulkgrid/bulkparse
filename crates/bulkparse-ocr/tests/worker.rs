use bulkparse::{CancellationToken, Error, PdfOcrProvider, PdfOcrRequest};
use bulkparse_ocr::ProcessOcr;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn setup(mode: &str, timeout: u64, pages: u32) -> (tempfile::TempDir, ProcessOcr) {
    let dir = tempfile::tempdir().unwrap();
    let manifest = dir.path().join("models.json");
    std::fs::write(&manifest, "{}").unwrap();
    let config = serde_json::json!({"executable":"python3", "args":[
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fake_worker.py"), "--mode", mode],
        "models":manifest,"page_timeout_ms":timeout,"reuse_process":true,"max_pages_per_worker":pages});
    (
        dir,
        ProcessOcr::from_json(&serde_json::to_vec(&config).unwrap()).unwrap(),
    )
}
fn call(
    provider: &ProcessOcr,
    bytes: &[u8],
    token: &CancellationToken,
    limit: usize,
) -> Result<String, Error> {
    provider.recognize(PdfOcrRequest {
        document: bytes,
        page: 1,
        cancellation: token,
        remaining_output_bytes: limit,
    })
}
fn pids(dir: &Path) -> Vec<String> {
    std::fs::read_to_string(dir.join("models.pids"))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}
fn cleaned(dir: &Path) {
    #[cfg(unix)]
    for pid in pids(dir) {
        assert!(!std::process::Command::new("kill")
            .args(["-0", &pid])
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap()
            .success());
    }
    for input in std::fs::read_to_string(dir.join("models.inputs"))
        .unwrap_or_default()
        .lines()
    {
        assert!(
            !Path::new(input).exists(),
            "temporary input remains: {input}"
        );
    }
}
#[test]
fn lazy_worker_reuses_models_but_never_results_and_recycles() {
    let (dir, provider) = setup("ok", 5000, 2);
    assert!(pids(dir.path()).is_empty());
    let token = CancellationToken::default();
    for bytes in [b"first".as_slice(), b"second", b"third"] {
        assert_eq!(
            call(&provider, bytes, &token, 100).unwrap(),
            std::str::from_utf8(bytes).unwrap()
        );
    }
    assert_eq!(pids(dir.path()).len(), 2);
    drop(provider);
    cleaned(dir.path());
}

#[test]
fn semantic_worker_reuses_process_and_returns_distinct_regions() {
    let (dir, provider) = setup("regions", 5000, 128);
    for text in ["x^2", "y^3"] {
        assert_eq!(
            call(
                &provider,
                text.as_bytes(),
                &CancellationToken::default(),
                100
            )
            .unwrap(),
            text
        );
    }
    assert_eq!(pids(dir.path()).len(), 1);
    drop(provider);
    cleaned(dir.path());
}
#[test]
fn failed_worker_is_reaped_and_next_request_can_restart() {
    let (dir, provider) = setup("crash-once", 5000, 128);
    let token = CancellationToken::default();
    assert!(call(&provider, b"first", &token, 100).is_err());
    cleaned(dir.path());
    assert_eq!(call(&provider, b"second", &token, 100).unwrap(), "second");
    assert_eq!(pids(dir.path()).len(), 2);
    drop(provider);
    cleaned(dir.path());
}
#[test]
fn invalid_frames_and_limits_discard_worker() {
    for (mode, limit) in [
        ("bad-id", 100),
        ("bad-page", 100),
        ("bad-json", 100),
        ("flood", 100),
        ("ok", 1),
    ] {
        let (dir, provider) = setup(mode, 5000, 128);
        assert!(call(&provider, b"payload", &CancellationToken::default(), limit).is_err());
        cleaned(dir.path());
    }
}
#[test]
fn page_timeout_kills_cached_worker_and_removes_tempfile() {
    let (dir, provider) = setup("hang", 500, 128);
    let started = Instant::now();
    assert!(
        matches!(call(&provider,b"payload",&CancellationToken::default(),100),Err(Error::Malformed(s)) if s.contains("timeout"))
    );
    assert!(started.elapsed() < Duration::from_secs(3));
    cleaned(dir.path());
}
#[test]
fn cancellation_of_inference_and_queued_request_is_bounded() {
    let (dir, provider) = setup("hang", 5000, 128);
    let provider = Arc::new(provider);
    let token = CancellationToken::default();
    let (other, cancel) = (Arc::clone(&provider), token.clone());
    let running = std::thread::spawn(move || call(&other, b"first", &cancel, 100));
    let end = Instant::now() + Duration::from_secs(3);
    while !dir.path().join("models.inputs").exists() {
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(5));
    }
    let queued_token = CancellationToken::default();
    let (other, cancel) = (Arc::clone(&provider), queued_token.clone());
    let queued = std::thread::spawn(move || call(&other, b"second", &cancel, 100));
    queued_token.cancel();
    assert!(matches!(queued.join().unwrap(), Err(Error::Cancelled)));
    token.cancel();
    assert!(matches!(running.join().unwrap(), Err(Error::Cancelled)));
    drop(provider);
    cleaned(dir.path());
}
#[test]
fn pre_cancelled_request_does_not_load_models() {
    let (dir, provider) = setup("ok", 5000, 128);
    let token = CancellationToken::default();
    token.cancel();
    assert!(matches!(
        call(&provider, b"ignored", &token, 100),
        Err(Error::Cancelled)
    ));
    assert!(pids(dir.path()).is_empty());
}
