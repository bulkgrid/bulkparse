# Optional bounded process pool

`bulkparse-pool` is a native Rust host component for Bulkgrid and other applications. It runs the same conversion API in persistent `bulkparse-worker` child processes. It adds no dependency to the portable core, CLI, Node.js, Python or WASM bindings. It is experimental; deployment-server and cross-platform validation remain outstanding.

## Using the pool

```sh
cargo build --release -p bulkparse-pool --bins --examples
cargo run --example parse -p bulkparse-pool -- \
  "$PWD/target/release/bulkparse-worker" fixtures/hello.docx
```

Rust hosts depend on the separate `bulkparse-pool` crate. Build and install the matching worker binary alongside the service; do not accept its executable or arguments from an upload request.

```rust
use bulkparse::Options;
use bulkparse_pool::{Pool, PoolConfig};
use std::time::Duration;

// Inside a function returning Result<_, Box<dyn std::error::Error>>:
let mut config = PoolConfig::new("/opt/bulkparse/bulkparse-worker");
config.workers = 2;
config.queue_capacity = 8;
let pool = Pool::new(config)?;

let job = pool.try_submit(
    "/srv/private-uploads/document.pdf",
    Options::default(),
    Duration::from_secs(30),
)?;
let cancellation = job.cancellation_handle(); // Can be cloned into another thread.
// cancellation.cancel();                    // For client disconnects, for example.
let report = job.wait()?;
// Inspect report.status, report.error and conversion diagnostics.
pool.shutdown();
```

Create a long-lived pool at service startup, not one pool per request. Workers start lazily on the first admitted jobs and remain available for reuse. Startup failures appear in job reports. The synchronous API can be called from a host's blocking executor; callers must also bound that executor and propagate admission failures to their upstream queue.

`try_submit` performs bounded request serialization but does not read the input or wait for queue capacity. It returns `QueueFull` immediately when the waiting queue is full. At most `workers + queue_capacity` jobs can be waiting or executing. The caller decides whether to return an overload response or retry admission later; retries must not create another unbounded queue.

## Job lifecycle

| Event | Behavior |
| --- | --- |
| Successful parse and rendering | `complete`, with the original `Conversion` payload |
| Known parse or Markdown rendering loss | `partial`, with content and diagnostics preserved |
| Unsupported, malformed, strict-mode or parser-limit error | `failed`, with the original conversion error code |
| Serialized result exceeds the response cap | `failed`, `conversion` error with code `output_limit` |
| Queue deadline expires | `deadline_exceeded`; removed without waiting for an active conversion |
| Active deadline expires | Kill and reap that child, then return `deadline_exceeded` |
| Queued cancellation | Remove the waiting job and return `cancelled` |
| Active cancellation | Kill and reap that child, then return `cancelled` |
| Worker exit, malformed reply, wrong ID or startup failure | Fail the current job with `worker`; replace the process for a later job |
| Pool shutdown/drop | Stop admission, return `shutdown` for outstanding jobs, reap children and join host threads |

The timeout begins at submission and includes queueing, lazy process startup, reading, parsing, rendering and transport. Polling checks occur about every 10 ms. Process termination, reaping, bounded response decoding and OS scheduling add overhead; this is not a real-time guarantee. Ordinary conversion errors leave the worker reusable. Crashed or timed-out jobs are **not automatically replayed**.

Cancellation is idempotent. Completion can win a concurrent cancellation race; the terminal report is authoritative. Dropping a job handle requests cancellation. `wait_timeout` only limits how long that call waits; it does not alter the job deadline. A successful wait consumes the one terminal report, so do not wait twice. Keep the input until the report arrives. `shutdown()` cancels outstanding work immediately; it does not implement graceful draining. To drain, stop submitting and collect outstanding reports first.

The service must handle shutdown signals and call `shutdown()` itself. Abrupt host termination, including an OOM kill, loses in-memory jobs and reports. A durable upstream queue with leases and an explicit retry policy is needed for recovery across host restarts.

## Limits and resources

| Setting | Default |
| --- | --- |
| Worker processes | 1; increase explicitly after measuring representative documents |
| Waiting jobs | 32 |
| Worker startup timeout | 10 seconds, also constrained by the job deadline |
| Input bytes | 32 MiB host ceiling; smaller per-job limits are honored |
| Serialized request | 16 KiB |
| Serialized response | 1 GiB, configurable from 1 KiB to 1 GiB |
| Attempts before process recycling | 1,000 |
| Per-job timeout | Required at submission |

Frames use a four-byte length prefix and versioned JSON protocol. Receivers reject oversized frames before decoding their bodies. The worker stages bounded responses in a 64 KiB memory spool that spills to disk, then releases its model before transmission. The host decodes directly into typed objects, avoiding a complete wire-byte buffer and generic JSON tree. Input reads remain capped. These controls do not impose a hard RSS/CPU cap on third-party parsers or allocations made while building the document model. Response decoding also allocates in the host. Memory retained by completed job handles belongs to the caller; continuously drain or drop them.

Use immutable regular files in private host-managed storage. Paths must be absolute UTF-8 and are not a tenant authorization boundary. Bulkgrid must validate upload ownership, constrain parser options, retain files until completion, and remove them afterward. The worker has the service account's filesystem permissions. It is process isolation, not a filesystem or network sandbox.

Deploy with OS/container memory, CPU and process limits, and leave headroom for the service and queued/completed results. This crate does not implement per-worker cgroups, Windows Job Objects, or a hard memory limiter. The bundled worker spawns no descendants. A trusted launcher must `exec` the worker; forked descendants that inherit its pipes are outside this implementation's cleanup guarantee. Container-level limits can affect the entire service, so verify OOM behavior on the actual deployment platform.

The pool uses one supervisor thread per slot, one I/O thread per live child and one queue-maintenance thread. No async runtime, HTTP listener, durable queue, OCR provider or model is installed. Node/Python services can continue using the existing bindings or integrate a separately hosted Rust service; this milestone does not add pool APIs to those bindings.

## Metrics

Each `JobReport` records queue, execution and total milliseconds. Execution includes process startup and IPC when needed. The outer status includes known Markdown rendering loss; the underlying conversion payload is unchanged. Preserve parser and renderer diagnostics when storing results.

`pool.metrics()` exposes admission/rejection counters, queue and active gauges with high-water marks, complete/partial/failed counts, cancellation/deadline/shutdown/worker-failure counts, worker starts/stops, cumulative queue/execution time and known worker PIDs. Counters include abandoned handles and terminal failures. After shutdown, accepted jobs equal complete plus partial plus failed, active/queued are zero and worker starts equal stops. PIDs may refer to an idle child that exited before its next health check; monitoring must handle disappeared processes.

Collect RSS/CPU from these PIDs through the deployment platform. The local exercise uses `psutil` in the benchmark environment, not as a runtime dependency. It samples host-plus-worker RSS every 20 ms using reported PIDs and records available host memory. Shared pages may be counted more than once; startup before the first PID report and short spikes may be missed. CPU samples cover currently live processes and reset when workers recycle, so they are not cumulative CPU accounting.

## Validation and reproduction

```sh
cargo test -p bulkparse-pool
cargo clippy -p bulkparse-pool --all-targets -- -D warnings
# With psutil installed in the comparison environment:
target/competitors/docling-venv/bin/python scripts/exercise_pool.py \
  --seconds 60 --workers 1 4 --out reports/worker-pool-new
```

The nine integration tests exercise conversion parity for all seven formats, strict PDF rejection, input/output caps, overload, queued and active cancellation/deadlines, dropped handles, worker crashes, invalid/oversized frames, wrong IDs, startup failures, concurrent admission, recycling and shutdown. Tests require `python3` for the fault peer; the library and worker binary do not. Fault markers are confined to that test-only peer; production code does not recognize document-specific markers.

The local exercise rotates through twelve original inputs, including a 1 MiB text file, a 10,000-row CSV, a 100-page PDF, all seven supported formats and three deliberately malformed files. It retains at most one outstanding job per worker, counts complete/partial/failed separately, and records startup, full structured JSON and Markdown transport. It is a sustained lifecycle/resource exercise, not the earlier Markdown-only competitor benchmark or a new fidelity evaluation. Input hashes and executable/source hashes are recorded; generated files can be reproduced by the script.

See recorded exercise metadata (historical artifact; not distributed) and results (historical artifact; not distributed). The earlier v2 exercise waited in submission order and left workers idle behind slow documents; v3 collects whichever result is ready while keeping the same bounded number of handles. Both records are retained, and they are not independent benchmark repetitions.

### Local resource observations

The final completion-order exercises ran on Apple Silicon macOS with 14 logical CPUs and 36 GiB physical memory. The runner enforced a sampled **1 GiB host-plus-worker RSS guard** and a 2 GiB available-host-memory floor. These are polling guards in the exercise runner, not resource limits implemented by the library.

| Workers | Outcome | Attempts | Complete / partial / failed | Sampled peak host + workers RSS |
| --- | --- | ---: | --- | ---: |
| 1 | Completed 60.04 seconds | 7,483 | 4,367 / 1,247 / 1,869 | 331.1 MiB |
| 2 | Completed 60.04 seconds | 14,436 | 8,421 / 2,406 / 3,609 | 632.3 MiB |
| 4 | Aborted by RSS guard | No final accounting | No final accounting | 1,025.7 MiB |

The two-worker follow-up is recorded in v4 (historical artifact; not distributed). The only implementation change between v3 and v4 was making the default worker count one; these exercises explicitly set their worker counts. Their measured attempt rates were 124.6/s and 240.4/s for one and two workers, including expected malformed-input failures. Total-job p95 latency was 54.76 ms and 56.50 ms. These are single local runs with warm file caches and a polling load driver, not stable capacity estimates.

Only the three deliberately malformed inputs failed in completed runs. Both PDF inputs remained partial; the other seven inputs returned complete. All accepted jobs in completed runs received terminal reports, with zero worker failures or deadline expirations. Recycling and shutdown reaped all eight processes started by the one-worker run and all sixteen started by the two-worker run. The externally aborted four-worker run left no observed surviving processes, but it has no terminal accounting or throughput claim.

The larger documents and full structured results use substantially more memory than the earlier small-PDF Markdown-only benchmark. The default is now one worker, with concurrency explicitly configured after measuring the actual workload. Two workers fitting this test budget does not establish a safe universal limit. Queue saturation and cancellation were exercised by integration tests; this load driver does not simulate an external overload arrival schedule.

Source, input, result-accounting and cleanup checks are recorded in verification (historical artifact; not distributed).

The new workspace member changes the historical full-workspace freeze hash even though parser files and dependency versions are unchanged. Reproducing the historical competitor benchmark requires its saved source snapshot or an explicitly recorded new freeze; do not overwrite historical freeze files to bypass the check.

Longer replicated trials on the deployment server, per-worker OS limits, overload arrival schedules, async service integration, and Linux/Windows execution remain to be validated. Nothing has been deployed or connected to a live Bulkgrid queue.

## Large structured responses

Default parser settings now handle the 112-document corpus without a special flag.
The default response ceiling is 1 GiB. Responses spill after 64 KiB of staging; the
worker releases its document before transmission and the host decodes a bounded
frame directly into typed objects. Compact cell arrays and shared worksheet names
avoid excess allocation during decoding. Custom smaller frame limits still fail
explicitly, and deadlines/cancellation still replace interrupted workers. The full
returned document remains in memory; the response ceiling is not a process-memory
guarantee. See measurements (historical artifact; not distributed).
