# Mixed-document comparison

## Overall engine comparison

**Tested on 200 real-world documents across 19 formats and eight engines.** One diagnostic pass on an Apple M3 Max with 36 GiB RAM, November 13, 2026. OCR was disabled.

BulkParse’s row and per-format results use its [current-build diagnostic pass](../biff-legacy-2026-11-13/diagnostic/summary.json), run later the same day on the identical corpus. The other engines use the original eight-engine comparison run. Each displayed measurement comes from one pass; these were separate runs, not a simultaneous retest of all engines.

| Engine | Nonempty outputs | Formats with output | Median ms¹ | Attempt time s¹ | CPU s² | Peak RSS MiB³ |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| BulkParse | 200/200 | 19/19 | 2.93 | 16.36 | 16.33 | 1,883.6 |
| AnyDoc | 185/200 | 15/19 | 5.27 | 13.72 | 13.67 | 2,615.8 |
| Docling | 191/200 | 15/19 | 722.35 | 800.28 | ≥ 724.97 | ≥ 3,278.8 |
| MarkItDown | 128/200 | 13/19 | 73.69 | 140.33 | 147.93 | 3,075.5 |
| Pandoc | 102/200 | 8/19 | 101.74 | 59.75 | 58.02 | 2,130.9 |
| Mammoth | 20/200 | 2/19 | 36.46 | 1.79 | 1.79 | 665.8 |
| LibreOffice | 154/200 | 15/19 | 877.57 | 582.57 | ≥ 310.34 | ≥ 4,701.0 |
| Unstructured | 158/200 | 15/19 | 252.40 | 332.84 | ≥ 241.03 | ≥ 976.2 |

Nonempty output counts use all 200 documents as the denominator. Formats with output counts extensions with at least one nonempty result.

¹ Unsupported means outside the configured adapter’s format profile, not the engine’s complete capability list. Failed attempts include unsupported variants within an attempted extension. Median is across nonempty outputs from one conversion per document. Attempt time includes failed attempts and excludes unsupported inputs. Each engine has a different successful subset: these totals and medians are **not an equal-workload ranking**. No full-corpus warm-up or repeated timing passes were run; a small adapter preflight preceded fresh workers. Worker startup is excluded, first-use work remains included, and required CLI launches are included. Engine order and OS cache effects are not averaged out in one pass.

² CPU includes the worker and completed child processes. “≥” means a killed worker prevented complete CPU accounting.

³ macOS `/usr/bin/time -l` reports high-water RSS, including initialization, output transport and waited-for child-process peaks. It is **not the simultaneous sum of the process tree**. “≥” means killed workers may lack memory samples. Different workloads and model imports affect these values; no whole-system memory winner is claimed.

**Quality review is pending.** Nonempty output measures coverage, not extraction accuracy. The planned assessment compares completeness, structure, formatting and cleanliness against source references on the same 40-document sample across engines. Overall scores will use equal format weights, with failed and unsupported inputs counted as zero; supported-only quality will be reported separately. This will be labelled assistant-reviewed, not independent blind judging. [Quality review protocol](../../benchmarks/realworld/README.md).

The corpus includes 112 development documents and 88 selected before conversion, so it is not an entirely unseen test set. Settings and raw records are linked below.

## Results by format

The same 200 documents and engine runs shown above underpin this breakdown. Cells show **nonempty outputs / documents · median successful conversion ms**; `—` means unsupported. Tiny samples and different successful subsets limit speed comparisons.

| Format (documents) | BulkParse | AnyDoc | Docling | MarkItDown | Pandoc | Mammoth | LibreOffice | Unstructured |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| csv (19) | 19/19 · 0.30 | 19/19 · 0.81 | 18/19 · 11.49 | 17/19 · 5.15 | 19/19 · 54.74 | — | 18/19 · 647.37 | 7/19 · 58.08 |
| doc (8) | 8/8 · 0.90 | 8/8 · 1.00 | 8/8 · 2,984.76 | — | — | — | 8/8 · 792.97 | 8/8 · 727.35 |
| docm (1) | 1/1 · 1.97 | 1/1 · 3.20 | 0/1 · — | 1/1 · 73.62 | — | 1/1 · 36.04 | 1/1 · 707.77 | 0/1 · — |
| docx (19) | 19/19 · 2.48 | 19/19 · 2.67 | 19/19 · 151.28 | 19/19 · 117.17 | 19/19 · 89.49 | 19/19 · 36.89 | 19/19 · 783.20 | 19/19 · 46.38 |
| epub (16) | 16/16 · 9.10 | 16/16 · 16.12 | 16/16 · 723.73 | 16/16 · 131.81 | 16/16 · 1,328.67 | — | — | 16/16 · 7,143.06 |
| ipynb (4) | 4/4 · 0.20 | — | — | 4/4 · 3.60 | 4/4 · 77.10 | — | — | — |
| md (5) | 5/5 · 0.12 | — | 5/5 · 11.41 | 5/5 · 3.95 | 5/5 · 68.74 | — | — | 5/5 · 23.85 |
| odp (4) | 4/4 · 8.37 | 4/4 · 12.63 | 4/4 · 174.90 | — | — | — | 4/4 · 1,511.46 | — |
| ods (19) | 19/19 · 52.02 | 16/19 · 15.30 | 18/19 · 2,322.19 | — | — | — | 19/19 · 2,047.78 | — |
| odt (19) | 19/19 · 1.02 | 19/19 · 1.83 | 19/19 · 66.49 | — | 19/19 · 69.59 | — | 19/19 · 704.17 | 19/19 · 242.39 |
| pdf (18) | 18/18 · 7.52 | 18/18 · 14.55 | 18/18 · 1,421.60 | 18/18 · 199.58 | — | — | — | 18/18 · 231.64 |
| ppt (1) | 1/1 · 0.78 | 1/1 · 0.57 | 1/1 · 901.47 | — | — | — | 1/1 · 1,352.87 | 1/1 · 816.31 |
| pptx (4) | 4/4 · 8.28 | 4/4 · 9.16 | 4/4 · 2,517.39 | 4/4 · 46.74 | — | — | 4/4 · 5,809.23 | 4/4 · 127.74 |
| rtf (19) | 19/19 · 5.62 | 19/19 · 12.14 | 19/19 · 1,642.36 | — | 19/19 · 252.28 | — | 19/19 · 939.77 | 19/19 · 429.80 |
| tsv (1) | 1/1 · 1.34 | — | — | 1/1 · 5.08 | 1/1 · 188.32 | — | 1/1 · 1,106.93 | 1/1 · 333.08 |
| txt (1) | 1/1 · 1.64 | — | — | 1/1 · 10.89 | — | — | 1/1 · 929.16 | 1/1 · 3,793.67 |
| xls (18) | 18/18 · 0.76 | 18/18 · 1.73 | 18/18 · 784.82 | 18/18 · 39.43 | — | — | 18/18 · 843.45 | 17/18 · 74.63 |
| xlsm (6) | 6/6 · 73.60 | 5/6 · 39.02 | 6/6 · 1,872.60 | 6/6 · 3,006.90 | — | — | 5/6 · 6,245.45 | 5/6 · 1,924.59 |
| xlsx (18) | 18/18 · 3.99 | 18/18 · 7.39 | 18/18 · 80.82 | 18/18 · 139.09 | — | — | 17/18 · 1,146.64 | 18/18 · 216.88 |

[Identical-input comparisons and exact document IDs](../mixed-200-2026-11-13/README.md#identical-input-comparison) · [Detailed conversion outcomes](../mixed-200-2026-11-13/published-outcomes.json)

## Identical-input comparison

[Common-input measurements](common-inputs.json) record exact document IDs and totals for each engine paired with the current BulkParse run. Each pair has a different successful subset; these are not a single ranking. Equal input sets do not establish equal extraction quality.

## Evidence

- [Published overview values](published-results.json)
- [Frozen input manifest](corpus-manifest.json), [selection and provenance](corpus-freeze.json)
- [Original single-pass records and settings](run/), [engine versions](versions.json), [model hashes](model-manifest.json)
- [Current BulkParse records](../biff-legacy-2026-11-13/diagnostic/after-0.json)
- [Memory accounting](memory-accounting.json)

The original run records retain their original measurements, including the earlier BulkParse build. `published-results.json` identifies the current row used by the README. Extracted third-party document text, binaries and duplicate source snapshots are not distributed. Input and output hashes remain in the records. Paths in historical machine metadata describe the measured host and must be adjusted on another machine.

## Reproduce

See the [setup and quality protocol](../../benchmarks/realworld/README.md). On macOS, after provisioning the comparator environments:

```sh
python3 scripts/fetch_mixed200.py
cargo build --release --locked -p bulkparse --example office_bench
python3 scripts/benchmark_mixed200.py \
  --manifest reports/mixed-200-2026-11-13/corpus-manifest.json \
  --out reports/new-mixed-run \
  --bulkparse-binary target/release/examples/office_bench
```

The harness uses macOS resource counters. Use a new output directory. Run engines sequentially; do not run other benchmarks concurrently. This measures the checked-out implementation, not an unavailable historical binary. The original measurements were one pass; repeated validation can use `--final-validation --passes 5 --warmup`.
