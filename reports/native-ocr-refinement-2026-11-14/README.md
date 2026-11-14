# Native OCR refinement — 14 November 2026

General table repairs improve extraction without changing OCR models or adding
inference passes. Swedish crop experiments were rejected because they introduced
recognition regressions. The accepted build retains the previous recognition path.

## Speed and memory

Same Apple M3 Max (36 GiB RAM), one inference thread, 14 scans. Each build gets an
excluded full-corpus warm-up followed by one sequential measured pass:

| Metric | Previous build | Updated build |
| --- | ---: | ---: |
| Elapsed time | 102.61 s | 102.33 s |
| CPU time | 102.56 s | 102.23 s |
| Peak sampled process-tree RSS | 1.584 GiB | 1.572 GiB |

No observed speed, CPU-time or peak-memory regression. These sub-1% differences
are too small to claim a meaningful performance gain. Models, configuration and
worker code are identical; only the native addon changes. Separate baseline then
candidate blocks do not cancel run-order/cache effects. This is a diagnostic
one-pass comparison, not a stabilized performance guarantee.

[Raw performance data and accounting](performance.json). RSS is sampled over
warm-up and measurement, including host and parser child. CPU is user plus system
time; network/database/queue operations are excluded. All 14 service output hashes
match the Python-bound quality run.

## Quality and coverage

| Check | Before | After |
| --- | ---: | ---: |
| Scanned table TEDS | 0.9116 | **0.9461** |
| Scanned table structure-only TEDS | 0.9944 | **1.0000** |
| Annotated tables detected | 11/11 | 11/11 |
| Complex scans with output | 14/14 | 14/14 |
| Referenced OCR cases exact after whitespace normalization | 21/24 | 21/24 |
| Mixed documents with output | 200/200 | 200/200 |

No scored table page regressed. The wide grouped-header example improved from
0.9174 to 0.9986 TEDS; the two chemical-limit tables improved from a page average
of 0.8440 to 0.9936. All 200 mixed-document Markdown hashes match the preceding
service validation. All seven authored image/PDF variants match between Node and
Python. All 26 OCR profiles complete without OCR failures; the intentionally blank
case remains empty. These are development inputs, not held-out generalization data.
Structure-only 1.0000 applies to these eleven tables and does not mean perfect text.

[Per-table comparison](table-comparison.json), [full scan metrics](scanned-quality.json),
[OCR profiles](multilingual-quality.json), [mixed-document comparison](output-comparison.json),
[mixed-document results](service-mixed-200.json), [binding checks](binding-images.json).

## Accepted changes

- Reconstruct fragmented multiline metric headers only when terminal unit labels
  align and at least three complete numeric body rows independently establish the
  same columns. Preserve every recognized word and existing numeric-row recovery;
  reject inconsistent or incomplete evidence.
- Recover a dash in an otherwise empty table cell, or correct a recognized equals
  sign, only when a bounded interior pixel check finds one compact horizontal
  stroke. Blank cells, two-stroke equals signs and ruling lines are not dashes.

The rules use geometry, recognized text and image pixels. They contain no document
IDs, filenames, reference answers, language-specific substitutions or word lists.
They allocate no additional page-sized image buffers and run no additional models.
The header repair uses small, bounded table metadata; dash checks scan bounded
cell interiors. Measured resource use is reported separately below.

## Swedish investigation and remaining gaps

The Swedish rings are visible in the actual recognition crops. Changing crop
margins fixes some errors, demonstrating sensitivity to preprocessing, but a
full multilingual rerun introduces new skewed-Swedish and Turkish errors. That
candidate was rejected; the final recognition source is byte-identical to the
preceding build. See the [rejected experiment](rejected-crop-experiment.json).

The same three referenced Swedish cases remain imperfect: uppercase Å is read as
A in three cases, and one noisy lowercase å is read as à. We cannot isolate the
remaining errors to model weights alone. A larger model or extra recognition
passes were not introduced under the speed/memory constraint.

Some short table header words and symbols still contain OCR errors. Raw TEDS also
penalizes letter-case differences in X/x checkmarks and N/n headings. Reference
annotations and evaluator normalization were not changed to improve scores.

## Verification and reproduction

[Automated checks](checks.json) and [source/model identities](identity.json) record
the tested implementation. The existing TypeScript service worker is copied without
source changes into isolated benchmark folders; each resolves a different native
addon. Its source hash is identical in both folders. Production services, cloud
storage, databases and queues are not involved. The service's bundled native
source and addon have been refreshed; its 16 compatibility tests and type check
pass. Nothing was deployed.

```sh
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo build --release -p bulkparse-python -p bulkparse-node -p bulkparse-cli --features bulkparse-cli/ocr
# Install the built native libraries into their binding directories.
python scripts/check_native_scans.py --config target/native-ocr/provisioned/config.json \
  --out target/native-ocr/reproduction-scans.jsonl
python scripts/validate_native_ocr.py target/native-ocr/reproduction-scans.jsonl \
  --out target/native-ocr/reproduction-quality.json \
  --docling target/native-ocr/performance-matched/docling-cpu
python scripts/validate_native_profiles.py --config target/native-ocr/provisioned/config.json \
  --out target/native-ocr/reproduction-profiles
```

The scan evaluator needs the existing opendataloader evaluation environment;
profile and image-fixture checks use the fixture environment. The performance
harness needs psutil. Model and corpus provisioning is described in the
[preceding migration report](../native-ocr-migration-2026-11-14/README.md).

## Targeted Docling Swedish follow-up

A direct rerun on the same three PDF inputs, using Docling CPU/RapidOCR with the
same PP-OCRv6 detection and recognition weights, reproduced the uppercase Å → A
error in all three cases. The first two texts match BulkParse after whitespace
normalization. On the noisy case, Docling correctly reads `blå`, while BulkParse
reads `blà`: three character errors for Docling versus four for BulkParse across
these three inputs. Thus the errors are mostly shared, but not identical. This
applies to the tested configuration, not every OCR backend available in Docling.
[Inputs, settings and actual text outputs](docling-swedish/comparison.json).
