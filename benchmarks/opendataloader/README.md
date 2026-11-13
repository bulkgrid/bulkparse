# BulkParse on opendataloader-bench

This adapter measures BulkParse's integrated native PDF conversion, including its
inspection, layout reconstruction and Markdown rendering. `inspect_pdf` alone
cannot produce the text, headings and tables required by this benchmark. No OCR
provider, learned parser, external service or model is used. The parser itself is
not modified by this benchmark.

See the [published PDF comparison](../../reports/broad-benchmark-2026-11-12/README.md) for the current README evidence.

## Provision and run

From the BulkParse project root, with Rust and Python 3.13+ installed:

```sh
git clone --single-branch --branch abi/pdf-parser-benchmark-results \
  https://github.com/firecrawl/opendataloader-bench.git target/opendataloader-bench
git -C target/opendataloader-bench checkout 340f25d70f5b2dbc4bc1cc6b154769f74b1fc745
python3 -m venv target/opendataloader-venv
target/opendataloader-venv/bin/python -m pip install -r benchmarks/opendataloader/requirements.txt
cargo fetch --manifest-path benchmarks/opendataloader/Cargo.toml --locked
target/opendataloader-venv/bin/python scripts/benchmark_opendataloader.py \
  --processor 'YOUR VERIFIED CPU MODEL' --out reports/opendataloader-repeat
python3 scripts/check_opendataloader_results.py reports/opendataloader-repeat
BULKPARSE_BENCH_WORKER="$PWD/target/opendataloader-build/opendataloader-repeat/release/bulkparse-opendataloader-bench" \
  python3 -m unittest discover -s scripts -p test_benchmark_opendataloader.py -v
```

Do not substitute the current upstream `main`: the cited results use the pinned
Firecrawl results branch, whose evaluator and predictions are retained here.
The script requires a clean checkout at the exact revision, 200 downloaded PDFs
with matching reference IDs, and a fresh output directory. It rejects Git LFS
pointers. The pinned checkout contains the actual PDFs; if an alternative clone
leaves pointers, download its LFS objects before running.

Only six small evaluation packages from the upstream lockfile are installed.
The upstream project's complete dependency list includes OCR packages; these are
unnecessary and excluded. Provisioning may use the network. Compilation uses
`--locked --offline`, and parsing and scoring use local data only.

Reruns build the current checkout. Historical executable binaries and source snapshots are not distributed.

## Measurement boundary

One persistent Rust process receives six full-corpus requests: one excluded warmup
and five measured passes. Each pass processes the same 200 filenames in sorted
order, sequentially, then returns its timing. There is no document worker pool.
The release build uses thin LTO and the PDF-only core dependency.

The full-corpus timer includes input file reads, parsing, Markdown rendering,
writing each prediction, and small diagnostic-record bookkeeping. Compilation,
process startup, JSON request/response transport, prediction hashing and scoring
are outside the timer. This follows the cited Python adapter's file-to-file
boundary; it is not the older local inspector benchmark's parse-only timer.
Files are buffered normally: no fsync, cache flush or CPU affinity is applied.

Only BulkParse is rerun, so there is no cross-engine rotation. The five competitor
rows are historical measurements, not new local timings. Different hardware and
Rust-versus-Python adapter overhead prevent interpreting the displayed times as
a controlled relative speedup.

## Scoring and verification

The original evaluator modules are copied without modification, with their
licenses, into the report. The runner invokes their per-document scorer and
aggregator directly. Unlike the upstream CLI's catch-and-continue loop, any
evaluation exception aborts this runner. Missing corpus inputs also abort.
Parser errors produce explicitly empty predictions and remain in accounting.

Every pass records all 200 outcomes and prediction hashes. All six passes must
produce identical Markdown. Actual parser options are recorded, not inferred.
Each new report uses its own build directory; use a unique report basename to
avoid reusing Cargo fingerprints from another source root. Source snapshots now
include the core's Rust tests and their three small embedded PDF fixtures.
The independent checker recomputes the five-run median, metric means, per-document
overall values, denominators, attempt counts and saved artifact hashes.

The runner also rescores the branch's saved pdf-inspector predictions and requires
every per-document score to match exactly. This checks evaluator compatibility;
it does not rerun the historical parser or validate its speed on this machine.

The published overall is the mean of **per-document** means over applicable
NID/TEDS/MHS values. It is not the average of the three displayed aggregate scores.
NID covers 200 documents, TEDS 42 and MHS 107. Missing reference structures yield
null and are excluded for that metric, exactly as upstream specifies.
