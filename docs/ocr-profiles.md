# OCR model profiles and scan stress evaluation — 2026-11-10

This report preserves the original profile comparison. The later
model-reuse and adaptive-detection study (historical artifact; not distributed)
changes the default small-model runtime and measures its Linux CPU tradeoffs.
The historical timings below describe the earlier one-process-per-page adapter.

BulkParse now supports explicit `multi-small`, `multi-medium` and `latin-mobile`
recognition profiles in its optional RapidOCR backend. The larger multilingual
model fixes the original `Åtta` error and a Turkish capital-letter error in the
authored tests. It still misses a Swedish accent in low-resolution and noisy
scans. The existing small model remains the default.

This milestone changes backend configuration and evaluation tooling. It does not
change the Rust parser, document schema, bindings, detection model, orientation
model, layout rules, recognition threshold or render DPI. Profiles select a
model explicitly; there is no language guessing, confidence-based fallback,
spelling repair, fixture-name check or document-specific substitution.

## Results

Each of three profiles converted the same 26 PDFs twice, rotating profile order:
156 timed attempts after a functional preflight. Eight PDFs were existing
regressions; sixteen were new controlled scans from ten original texts in nine
languages. Two upstream scans were separately labeled, unscored smoke tests.
All attempts returned partial results without backend or harness errors. Repeated
JSON outputs were identical for every case/profile pair.

Exact means NFC/whitespace-normalized text with case, accents and punctuation
retained. The counts below are distinct PDFs, not doubled repetition counts.

| Profile | Existing regressions exact | New stress cases exact | New stress character errors |
| --- | ---: | ---: | ---: |
| `multi-small` (default) | 7 / 8 | 13 / 16 | 3 / 1,940 |
| `multi-medium` | 8 / 8 | 14 / 16 | 2 / 1,940 |
| `latin-mobile` | 7 / 8 | 7 / 16 | 79 / 1,940 |

The larger model recovers `Åtta` in the existing scan and `İpek` in the new Turkish
page. Both multilingual models read `Årets` as `Arets` in the 72-DPI and noisy
Swedish variants. Those two errors are retained; source text is not used to repair
recognition output. Both multilingual models match all words in the Chinese case.
The Latin profile loses most Chinese content and also makes more accent errors
on Swedish, Danish, Polish and Turkish examples. It is an experimental option,
not a recommended replacement based on these results.

The new set uses English, Swedish, French, German, Spanish, Danish, Polish,
Turkish and Chinese. English and one Swedish text have resolution, skew and
noise variants; these are correlated copies, not independent documents. The
original regression set also includes a blank page and reused multipage text.
The pages were visually checked, but this is not a human-annotated real-world
scan dataset or a per-language accuracy guarantee. The upstream documents have
no independent transcription, so their successful conversions receive no
accuracy score. This comparison does not run competitor engines.

## Resource observations

| Profile | Total model assets | Median document time | Maximum sampled CLI + OCR RSS |
| --- | ---: | ---: | ---: |
| `multi-small` | 30.3 MiB | 1.90 s | 947.6 MiB |
| `multi-medium` | 83.1 MiB | 2.61 s | 985.2 MiB |
| `latin-mobile` | 17.6 MiB | 1.86 s | 950.8 MiB |

These are local Apple Silicon measurements over the 26-document mixture, with
two repetitions per profile. Every conversion starts fresh CLI and OCR processes;
time includes model loading, rendering, inference and transport. A small
benchmark-only Python wrapper registers the OCR PID before executing the normal
helper; its startup cost is included. Filesystem/model-file caches are warm.
The timings are not steady-state throughput, cold-host latency or statistically
established speed rankings.

RSS samples every 10 ms include Rust and the explicitly registered direct OCR
process. Unrelated host processes are not enumerated. Shared pages can be counted
twice and short spikes missed; unregistered grandchildren would be excluded, but
this backend uses native threads. The 2-GiB polling guard is not an OS memory cap.
The smaller Latin model does not yield a materially lower observed peak memory
footprint here: model-file size is not runtime memory consumption.

**OCR currently needs much more memory than native parsing.** The earlier native
worker figures cannot be used to size OCR workers. Keeping OCR optional preserves
the lightweight default parser, but this OCR path still needs separate capacity
limits and target-server measurements. No model cache, persistent inference
worker or memory optimization is claimed in this milestone.

## Configuration and compatibility

Use the [setup guide](../backends/rapidocr/README.md) to provision a selected
recognizer locally and create separate settings with `configure.py --profile`.
The configuration command never downloads models. It verifies SHA-256 values
against the pinned RapidOCR 3.10.0 catalog, records absolute paths and refuses to
overwrite existing settings. Runtime downloads remain disabled.

New model manifests use version 2 and name the profile. Legacy version 1 manifests
continue to use the old parameter defaults. All eight original regression JSON
outputs match the prior milestone exactly with both legacy settings and explicit
`multi-small` settings. OCR response protocol 2 and document schema 0.2 are unchanged.
Profile selection is host configuration through the Rust provider/CLI; binding
OCR APIs and browser inference remain future work.

Twenty-seven Python tests pass, including eleven new profile/measurement tests.
They cover asset mismatches, missing models, legacy compatibility, overwrite
protection, unknown settings, accent-sensitive scoring, failure accounting,
process cleanup and output/memory guards. Real-engine legacy regressions and
complete attempt/metric checks pass. No Rust source or dependencies changed;
the prior Rust validation remains applicable. Remote CI has not run.

## Reproduction and remaining work

The [protocol and instructions](../benchmarks/ocr-profiles/README.md),
[manifest](../benchmarks/ocr-profiles/manifest.json),
run metadata (historical artifact; not distributed),
comparison (historical artifact; not distributed) and
verification (historical artifact; not distributed) retain settings,
hashes, attempts and measured tradeoffs. Original inputs and their raw converted
JSON are included. External PDFs, external converted text, model weights and
installed runtimes are excluded from the source bundle.

Further quality work needs independent real scans with human transcriptions,
especially degraded accents, handwriting and complex scripts/layouts. Paragraphs,
scanned tables, bad hidden OCR layers and mixed native/scanned regions remain
unresolved. Performance work should separately address repeated model startup
and the measured OCR memory footprint before choosing production concurrency.

Upstream references: [RapidOCR model routes](https://rapidai.github.io/RapidOCRDocs/main/en/model_list/),
[model catalog](https://github.com/RapidAI/RapidOCR/blob/main/python/rapidocr/default_models.yaml),
and [PP-OCRv5 language documentation](https://paddlepaddle.github.io/PaddleOCR/latest/en/version3.x/algorithm/PP-OCRv5/PP-OCRv5_multi_languages.html).
Local model fingerprints identify the actual tested assets; upstream language
support lists are not BulkParse quality guarantees.

## Linux CPU alternatives

The Linux ARM64 comparison (historical artifact; not distributed)
measures ten configurations across native PaddleOCR, RapidOCR, RapidOCRLite and
Tesseract, including warm wall time, CPU seconds, peak resident memory and text
quality. It uses the same authored cases as raw OCR-stage inputs, with one warm-up
and five measured passes. These scores exclude BulkParse's layout reconstruction
and should not be compared directly with the end-to-end scores above. Production
OCR defaults remain unchanged; server architecture and independent scan quality
still need validation before changing them.
