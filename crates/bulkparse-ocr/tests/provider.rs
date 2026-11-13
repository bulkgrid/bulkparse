use bulkparse::{CancellationToken, Error, PdfOcrProvider, PdfOcrRequest};
use bulkparse_ocr::{Config, ProcessOcr};
use std::path::Path;
use std::time::{Duration, Instant};

fn setup(mode: &str, timeout: u64) -> (tempfile::TempDir, ProcessOcr) {
    let dir = tempfile::tempdir().unwrap();
    let manifest = dir.path().join("models.json");
    std::fs::write(&manifest, "{}").unwrap();
    let config = Config {
        executable: "python3".into(),
        args: vec![
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fake_backend.py")
                .to_str()
                .unwrap()
                .into(),
            "--mode".into(),
            mode.into(),
        ],
        models: manifest,
        dpi: 200,
        max_pixels: 8_000_000,
        page_timeout_ms: timeout,
        max_input_bytes: 1024,
        reuse_process: false,
        max_pages_per_worker: 128,
    };
    (dir, ProcessOcr::new(config).unwrap())
}

fn request(token: &CancellationToken, limit: usize) -> PdfOcrRequest<'_> {
    PdfOcrRequest {
        document: b"%PDF-fake",
        page: 2,
        remaining_output_bytes: limit,
        cancellation: token,
    }
}

fn reaped(dir: &Path) {
    let pid = std::fs::read_to_string(dir.join("models.started")).unwrap();
    #[cfg(unix)]
    assert!(!std::process::Command::new("kill")
        .args(["-0", &pid])
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap()
        .success());
    #[cfg(not(unix))]
    let _ = pid;
}

#[test]
fn unicode_output_and_repeated_calls_are_cleaned_up() {
    let (dir, provider) = setup("ok", 5000);
    for _ in 0..2 {
        assert_eq!(
            provider
                .recognize(request(&CancellationToken::default(), 100))
                .unwrap(),
            "Åtta lådor"
        );
        reaped(dir.path());
    }
}

#[test]
fn invalid_responses_crashes_and_backend_failures_are_errors() {
    for mode in [
        "invalid",
        "wrong-page",
        "crash",
        "error",
        "bad-geometry",
        "mixed-envelope",
        "wrong-version",
        "bad-confidence",
        "bad-regions",
        "ambiguous-regions",
    ] {
        let (dir, provider) = setup(mode, 5000);
        assert!(matches!(
            provider.recognize(request(&CancellationToken::default(), 100)),
            Err(Error::Malformed(_))
        ));
        reaped(dir.path());
    }
}

#[test]
fn wire_and_decoded_output_limits_are_enforced() {
    for (mode, limit) in [("oversize", 100), ("ok", 1)] {
        let (dir, provider) = setup(mode, 5000);
        assert!(matches!(
            provider.recognize(request(&CancellationToken::default(), limit)),
            Err(Error::Limit(_))
        ));
        reaped(dir.path());
    }
}

#[test]
fn timeout_terminates_and_reaps_backend() {
    let (dir, provider) = setup("hang", 3000);
    let start = Instant::now();
    assert!(
        matches!(provider.recognize(request(&CancellationToken::default(), 100)), Err(Error::Malformed(s)) if s.contains("timeout"))
    );
    assert!(start.elapsed() < Duration::from_secs(6));
    reaped(dir.path());
}

#[test]
fn cancellation_stops_running_inference() {
    let (dir, provider) = setup("hang", 5000);
    let token = CancellationToken::default();
    let child_token = token.clone();
    let thread = std::thread::spawn(move || provider.recognize(request(&child_token, 100)));
    let end = Instant::now() + Duration::from_secs(3);
    while !dir.path().join("models.started").exists() {
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(5));
    }
    token.cancel();
    assert!(matches!(thread.join().unwrap(), Err(Error::Cancelled)));
    reaped(dir.path());
}

#[test]
fn cancelled_and_oversized_requests_do_not_start_backend() {
    let (dir, provider) = setup("ok", 5000);
    let token = CancellationToken::default();
    token.cancel();
    assert!(matches!(
        provider.recognize(request(&token, 100)),
        Err(Error::Cancelled)
    ));
    let token = CancellationToken::default();
    assert!(matches!(
        provider.recognize(PdfOcrRequest {
            document: &[0; 1025],
            ..request(&token, 100)
        }),
        Err(Error::Limit(_))
    ));
    assert!(!dir.path().join("models.started").exists());
}

#[test]
fn structured_process_protocol_preserves_evidence() {
    let (dir, provider) = setup("geometry", 5000);
    let output = provider
        .recognize_page(request(&CancellationToken::default(), 100))
        .unwrap();
    match output {
        bulkparse::PdfOcrOutput::Lines { image_size, lines } => {
            assert_eq!(image_size, [600, 800]);
            assert_eq!(lines.len(), 1);
            assert_eq!(lines[0].text, "Åtta lådor");
            assert_eq!(lines[0].quad[0], [111.78385925292969, 20.]);
            assert_eq!(lines[0].confidence, 0.875);
        }
        _ => panic!("missing line evidence"),
    }
    reaped(dir.path());
}

#[test]
fn semantic_process_protocol_preserves_formula_and_missing_confidence() {
    let (dir, provider) = setup("regions", 5000);
    let output = provider
        .recognize_page(request(&CancellationToken::default(), 100))
        .unwrap();
    let bulkparse::PdfOcrOutput::Regions {
        image_size,
        regions,
    } = output
    else {
        panic!("expected semantic regions")
    };
    assert_eq!(image_size, [600, 800]);
    assert_eq!(regions[0].confidence, None);
    assert!(
        matches!(&regions[0].content, bulkparse::PdfOcrRegionContent::Formula { tex } if tex == "E=mc^2")
    );
    reaped(dir.path());
}
