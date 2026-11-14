# Native OCR migration — 14 November 2026

The file-converter service now uses TypeScript and BulkParse's Rust OCR provider.
The production image has no Python interpreter/worker. Nothing was deployed by
this validation. [Full scope, tests and known gaps](../../docs/ocr-bindings-validation.md).

## Performance

Same Apple M3 Max, 36 GiB RAM, CPU inference with one thread per engine, 14 complex
scans. One measured sequential corpus pass after an excluded full-corpus warm-up:

| Pipeline | Elapsed | CPU time | Peak sampled process-tree RSS |
| --- | ---: | ---: | ---: |
| Rust service parser | 103.2 s | 103.2 s | 1.58 GiB |
| Docling CPU pipeline | 178.9 s | 181.6 s | 3.50 GiB |

The final Rust path uses **42% less elapsed time, 43% less CPU time and 55% less
peak sampled memory** in this run. Both pipelines complete all 14 inputs. Models
are reused; native measurement includes the Node host and supervised addon child.
This is a one-pass development comparison, not a stabilized production forecast.
Text detection/recognition weights match, while layout, table and orientation
models differ. The results measure the combined pipeline changes and cannot be
attributed to the implementation language alone. This rerun supersedes the earlier
109.6-second / 1.67-GiB candidate measurement.

See [raw measurements and accounting](performance.json),
[comparator versions](comparator-versions.json), [model/source identity](identity.json)
and [native binary identity](native-binary.json). Mac performance and Linux ARM64
runtime validation are separate checks; no Linux x64 speed measurement is claimed.

## Coverage and quality

| Check | Result |
| --- | --- |
| Mixed-document service path | 200/200 nonempty outputs |
| Existing mixed-document output hashes | 199 identical; one previously empty output now nonempty |
| Complex development scans | 14/14 nonempty outputs |
| Annotated scanned tables | 11/11 detected by both pipelines |
| Table TEDS | Rust 0.9116; Docling 0.9045 |
| Structure-only TEDS | Rust 0.9944; Docling 0.9925 |
| OCR profile cases | 26 processed without OCR failure; one intentionally blank |
| Referenced OCR text cases | 21/24 exact after whitespace normalization |
| Native Node/Python image results | Identical across seven authored format/frame variants |
| Linux ARM64 runtime | HTTP health, JPEG, PDF and multipage TIFF pass with network disabled and Python absent |

Artifacts: [table/page metrics](scanned-quality.json),
[multilingual/degraded fixtures](multilingual-quality.json),
[image binding checks](binding-images.json), [mixed-document results](service-mixed-200.json),
[output comparison](service-output-comparison.json), and [Linux runtime check](linux-runtime.json).
Quality-run timing fields are diagnostic only; other checks ran concurrently.
Use the dedicated performance run above for timing comparisons.

Remaining errors include Swedish accents and complex table headers/cell assignments.
These development samples do not establish general accuracy or perfect extraction.
The default pipeline does not recognize LaTeX or describe figures; the previous
service did not enable formula enrichment. WASM accepts OCR evidence but does not
load native inference libraries. Production database/queue behavior is covered by
compatibility tests with fake transports, not a live deployment test.

## Reproduce

Provision models with `scripts/provision_native_ocr.mjs`. Build the Node/Python
bindings and the TypeScript service using its README. For performance, set
`BULKPARSE_SERVICE_ROOT` and `BULKPARSE_OCR_CONFIG` and run:

```sh
python scripts/benchmark_native_migration.py \
  --node-worker scripts/native_service_bench_worker.mjs \
  --out target/native-ocr/performance-reproduction
```

The performance harness requires psutil and the existing Docling comparison
environment/model cache; it forbids model downloads during conversion. Obtain and
prepare the deterministic 14 scan inputs using `scripts/check_scanned_layouts.py`
and the existing ICDAR/DocLayNet corpus setup. Input hashes are verified before
measurement. Conversion references never enter the parsers. See the validation
guide for native-only smoke tests and the 26-case profile check.
