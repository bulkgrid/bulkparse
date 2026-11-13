# Persistent worker benchmark

This benchmark measures a **process pool with one initialized engine per worker**. It is not a shared-model thread pool and does not implement a production Bulkgrid queue. Worker counts are 1, 2, 4 and 8. Each worker has one request in flight; completed requests release capacity for the next document. Engines are compared sequentially so they do not compete with each other during measurement.

The two Rust adapters call the libraries directly. BulkParse retains one `Parser`; AnyDoc exposes a stateless conversion function in a persistent process. The Python adapter retains one Docling `DocumentConverter` and explicitly initializes its PDF pipeline before announcing readiness. Docling uses the previously pinned Heron model, CPU with four configured threads per worker, OCR off and table recognition off. BulkParse includes its ruled-table detector with Unicode normalization off; AnyDoc uses its native defaults. Feature sets differ and quality must be considered alongside performance.

## Build and run

The isolated worker crates do not change production APIs or the main Rust workspace. Their committed lockfiles preserve the original engine dependency versions. In particular, AnyDoc's broad semver dependency must not silently upgrade `pdf-inspector` beyond the measured 1.14.2.

```sh
cargo build --offline --locked --manifest-path benchmarks/workers/bulkparse/Cargo.toml \
  --release --target-dir target/workers/bulkparse
CARGO_HOME="$PWD/target/competitors/cargo-home" cargo build --offline --locked \
  --manifest-path benchmarks/workers/anydoc/Cargo.toml \
  --release --target-dir target/workers/anydoc

# Requires the comparison environment, psutil, source corpus and local model.
HF_HOME="$PWD/target/competitors/hf-cache" \
target/competitors/docling-venv/bin/python scripts/benchmark_workers.py \
  --levels 1 2 4 8 --duration 10 --out reports/worker-benchmark-new

target/competitors/docling-venv/bin/python -m unittest discover -s scripts -p 'test_*.py'
```

See [holdout provisioning](../holdout/README.md) for the external engines and documents. `engines.json` records the exact worker commands, settings and files to fingerprint. The controller uses `psutil` from the already provisioned environment; these benchmark tools are not runtime dependencies of BulkParse. The parser source must match `benchmarks/pdf-tables/freeze-v2.json`. Build-specific worker hashes and lockfiles are recorded separately. Only explicit `--engine` selections restrict the engine matrix.

## Measurement

- Record startup-to-ready for each worker and total sequential pool startup. Memory is monitored during startup too.
- Convert the simple prose case once per worker as untimed warmup. This does not warm every document or eliminate later allocator/cache growth.
- Repeatedly dispatch a fixed shuffled cycle of all 24 holdout documents. Continue until the requested duration, then finish a complete corpus cycle and drain outstanding jobs. Every completed configuration has equal attempt counts for each document. The max-job ceiling can end a fast run early; this is reported explicitly.
- Report successful documents/second, attempts/second, successful-request p50/p95 latency, all-attempt p95, CPU consumption and average busy cores. Failed conversions remain in the workload and take time, but do not count as successful throughput.
- Latency is dispatch-to-response, including input read, conversion, Markdown and JSONL IPC. There is no large queued backlog; this is closed-loop service latency, not externally scheduled arrival-to-completion latency. Nearest-rank p95 is used.
- Measure aggregate worker RSS every 20 ms, separately reporting the measured phase and startup/warmup. This is a **sampled peak**; shorter spikes may be missed. Shared pages can be counted in several processes. The controller is excluded from pool RSS/CPU. The adapters do not intentionally spawn subprocesses; descendant-process resource accounting is not implemented.
- Store first outputs for quality scoring outside the timed window. Compare repeated outputs for determinism. Keep gold, upstream and PDFKit silver references distinct; they do not form one overall accuracy score.

Repeated documents use warm filesystem caches, but the adapters do not cache conversion results. Very fast native conversions may be limited by the controller/IPC, and model-heavy profiles have different internal threading. Docling's four threads per process can oversubscribe the machine at high worker counts. Results are local observations, not a universal library-speed ranking, a leak test or production capacity guarantees.

## Budgets and cleanup

Defaults are 8 GiB aggregate sampled worker RSS, at least 2 GiB host memory available, 180 seconds for a worker to initialize and 60 seconds for a request. These are polling guards, not hard OS limits; overshoot is possible. A configuration that breaches a guard is stopped and reported as aborted, including the phase and partial observations. No throughput claim is made for an aborted, unbalanced run. Each owned worker process group is terminated and reaped, including startup failures and timeouts.

For repeatable server sizing, run on an otherwise idle dedicated machine with OS-enforced worker limits, representative private documents and longer replicated trials. A smaller memory allowance is a valid deployment constraint; do not hide resource-limited levels by omitting them from a report.

Reports contain hashes, settings, per-request timings and memory samples. Third-party document text, worker stderr and converted Markdown remain under ignored `target/worker-benchmarks`. Historical CLI and holdout reports remain unchanged. See [the worker results](../../docs/worker-benchmarks.md).
