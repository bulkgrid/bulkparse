# Persistent worker results — 2026-11-10

BulkParse's native PDF profile scaled from **428.4 to 3,130.3 returned results/second** between one and eight persistent processes on this 24-document workload. Sampled aggregate worker RSS rose from **38.9 to 328.4 MiB**. Outputs stayed identical to the previous CLI evaluation. This milestone adds benchmark adapters and measurements; production parser code is unchanged.

These are short, local measurements with different capability profiles, not a general speed ranking or production sizing guarantee. A returned result can be partial or empty. No OCR ran.

## Measured results

Each completed configuration attempted every document equally often. “Results/s” excludes conversion errors, while elapsed time includes their cost. Latency percentiles cover returned results and include file reading, conversion, Markdown serialization and JSONL communication. RSS is the sampled peak sum across worker processes, excluding the controller.

| Engine | Workers | Results/s | Attempts/s | p50 / p95 latency, ms | Peak pool RSS, MiB | Errors / attempts |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| BulkParse | 1 | 428.4 | 428.4 | 0.26 / 14.53 | 38.9 | 0 / 4,296 |
| BulkParse | 2 | 833.2 | 833.2 | 0.26 / 15.04 | 78.4 | 0 / 8,352 |
| BulkParse | 4 | 1,601.1 | 1,601.1 | 0.28 / 15.56 | 161.5 | 0 / 16,056 |
| BulkParse | 8 | 3,130.3 | 3,130.3 | 0.31 / 15.69 | 328.4 | 0 / 31,368 |
| AnyDoc | 1 | 54.2 | 62.0 | 0.76 / 156.87 | 53.1 | 78 / 624 |
| AnyDoc | 2 | 106.1 | 121.2 | 0.73 / 160.71 | 106.4 | 153 / 1,224 |
| AnyDoc | 4 | 205.0 | 234.3 | 0.79 / 165.48 | 207.2 | 297 / 2,376 |
| AnyDoc | 8 | 396.0 | 452.6 | 0.99 / 167.62 | 437.8 | 576 / 4,608 |
| Docling | 1 | 2.9 | 2.9 | 189.67 / 522.01 | 2,830.6 | 0 / 48 |
| Docling | 2 | 4.4 | 4.4 | 284.44 / 684.42 | 4,052.0 | 0 / 48 |
| Docling | 4 | 5.5 | 5.5 | 443.51 / 1,253.25 | 7,401.0 | 0 / 72 |
| Docling | 8 | Aborted | — | — | 8,203.1 | 10 attempts completed before abort |

Docling's eight-worker pool initialized and warmed up, then exceeded the **8 GiB sampled pool-RSS guard during measurement**. Its incomplete workload has no reported throughput or latency percentile. All workers were terminated and reaped. The other guard, at least 2 GiB available host memory, did not trigger. These polling guards permit overshoot and are not OS-enforced limits.

## Startup and CPU cost

Startup is excluded from the table above. Processes start sequentially, and Docling explicitly initializes its PDF pipeline before reporting readiness. Each worker then converts the simple prose document once outside the measured window.

| Engine | Total pool startup, seconds at 1 / 2 / 4 / 8 workers | Average busy CPU cores at 1 / 2 / 4 / 8 workers |
| --- | --- | --- |
| BulkParse | 0.003 / 0.006 / 0.011 / 0.023 | 0.96 / 1.93 / 3.85 / 7.62 |
| AnyDoc | 0.005 / 0.008 / 0.017 / 0.032 | 1.29 / 2.60 / 5.07 / 9.80 |
| Docling | 4.23 / 7.93 / 14.79 / 31.08 | 2.59 / 4.72 / 7.79 / not measured |

Average busy cores are summed worker user/system CPU seconds divided by measured wall time. Docling is configured for four CPU threads per worker; higher worker counts can oversubscribe this machine. This test does not compare alternative thread counts or shared-model designs.

## Quality and scope

- BulkParse uses the frozen ruled-table parser with optional Unicode expansion disabled. PDF results remain `partial`, including an empty scan-only result. Mixed native/scanned input retains only the extractable native content.
- AnyDoc 0.2.4 uses its native defaults with `pdf-inspector` 1.14.2. Three of 24 cases fail on every cycle: scan-only, mixed native/scanned input, and the external image-structure case. Those attempts remain in the denominator for attempts/s and consume measured time.
- Docling 2.150.0 / docling-parse 7.23.0 uses the pinned Heron learned-layout model, CPU, OCR off and table recognition off. It returns empty text for the scan-only document. Its learned layout still has a different capability and cost profile from native heuristics.
- Every completed configuration matches the previous CLI success/failure decisions and successful Markdown hashes for all 24 cases. Repeated outputs were deterministic. The aborted configuration matches the ten observed cases only.
- References comprise 12 authored cases, one upstream transcript and 11 PDFKit differential references. These tiers remain separate. Token overlap and table/order checks do not establish overall document fidelity. Existing Arabic, complex-table and scanned-content gaps remain; see the [quality evaluation](holdout-evaluation.md) and [table milestone](pdf-tables.md).

## Method and limitations

The host was Apple Silicon macOS 26.6.2, with 14 logical CPUs and 36 GiB physical memory. The controller used Python 3.12.3 and psutil 7.2.2. Engines ran sequentially, each at worker counts 1, 2, 4 and 8. Every process retained its engine and accepted one request at a time; no conversion-result cache or per-document process startup was used.

There was one measured run per configuration, with a minimum 10-second window followed by completion of a whole 24-case cycle and draining outstanding requests. Completed native runs lasted about 10 seconds; Docling runs lasted 10.98–16.70 seconds. Short trials and few Docling observations make tail estimates preliminary. There are no confidence intervals or independent repeated trials.

Inputs are repeatedly read from a small fixed corpus with warm filesystem caches. This is closed-loop dispatch-to-response latency, without an external arrival schedule or backlog waiting time. It does not measure overload behavior, binding overhead, network/storage latency, large-document memory growth, long-running leaks, OCR, or all supported formats. Native worker startup is warm-host startup, not cold-machine startup.

RSS is sampled every 20 ms and may miss shorter spikes. Shared pages can be counted repeatedly across processes. Controller and descendant-process memory/CPU are excluded; these adapters do not intentionally start child processes. A production service needs OS-enforced resource limits in addition to input limits.

## Implications for Bulkgrid

Persistent native workers are a promising low-resource path: eight workers delivered about 7.3 times the one-worker throughput in this run, with roughly linear pool memory growth. Keep a configurable bound on active conversions and queued work. Select the deployment worker count using the target machine, representative documents, and memory headroom; the observed 328 MiB is not a safe upper bound for arbitrary inputs.

The next deployment experiment should use longer repeated trials on the actual server, include large and malformed documents, and test cancellation and overload. A separate optional OCR/model pool should have its own capacity budget. No production queue, new OCR engine, or parser optimization was added in this milestone.

## Reproduction and verification

See [build and run instructions](../benchmarks/workers/README.md). The run metadata (historical artifact; not distributed) records executable, dependency, model, corpus and harness fingerprints. The summary (historical artifact; not distributed), per-request timing files, memory samples and exact harness snapshots preserve the measurements. Third-party documents and converted text are not redistributed.

Verification (historical artifact; not distributed) confirms unchanged parser source, engine binaries, lockfiles, local model, inputs and references; output parity; balanced completed workloads; and worker cleanup. All 16 Python harness tests pass, including reuse, failure accounting, timeout cleanup and memory-guard cleanup. The benchmark crates build with their committed lockfiles. CI was updated to install the benchmark test dependency; remote CI has not run.
