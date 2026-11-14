# OCR recognition profile evaluation

This compares BulkParse with three explicitly chosen recognition models. It is
not a head-to-head benchmark against AnyDoc or Docling.

The frozen [protocol](protocol.json) specifies model choices, corpus tiers,
metrics and resource measurement before the valid inference run. Detection,
orientation, rendering DPI, filtering and Rust layout logic stay fixed. The
default remains `multi-small`; no automatic language detection, confidence-based
fallback or document-specific correction is introduced.

## Corpus

- Eight previously observed regression PDFs, including the original Swedish
  accent failure and column-order case.
- Sixteen original controlled scans from ten distinct source texts in nine
  languages. English and one Swedish text have clean, 72-DPI, 3-degree-skew and
  noisy/low-contrast variants. These variants are correlated, not independent
  documents. Other pages use 144-DPI sources. All final PDFs contain raster
  images only, without hidden text layers.
- Two optional upstream Docling PDFs are smoke tests only. They have no
  independently verified transcription and receive no accuracy score. Inputs,
  converted text and screenshots of those documents are not redistributed.

The original [expectations and perturbation plan](../../fixtures/ocr-stress/plan.json)
were written before rendering or OCR. The pages were visually reviewed for
legibility and clipping. They are authored stress cases, not real scanner
captures or a human-annotated production dataset. Do not regenerate them to
evaluate a model change. Use the committed bytes for repeatability; the one-time corpus generator has been retired.

## Run

Provision the [optional OCR backend](../../backends/rapidocr/README.md) and all
three local profiles first. Put their absolute config paths in a JSON map:

```json
{
  "multi-small": "/absolute/settings/small/config.json",
  "multi-medium": "/absolute/settings/medium/config.json",
  "latin-mobile": "/absolute/settings/latin/config.json"
}
```

The measurement harness requires Python 3.12, psutil 7.2.2 and POSIX process
groups. Use an isolated environment; the parser itself does not depend on
psutil. This run used the existing isolated comparison environment.

```sh
python -m pip install -r benchmarks/workers/requirements.txt
cargo build --locked --release -p bulkparse-cli --features ocr
python scripts/evaluate_ocr_profiles.py \
  --binary "$PWD/target/release/bulkparse" \
  --profiles /absolute/profiles.json \
  --out reports/my-ocr-profile-run
```

An output directory must be new. `--include-external` additionally requires the
two upstream files at the paths and exact SHA-256 values in `manifest.json`.
Otherwise only original project fixtures run. Source URLs locate the upstream
files; the hashes, not an unverified branch revision, identify tested bytes.

## Interpretation

NFC/whitespace normalization preserves case, accents and punctuation. CER and
WER are Levenshtein diagnostics; WER is not meaningful for comparing unspaced
Chinese with Latin text. Report per-language and per-tier results rather than a
single universal accuracy number. Failures remain in the sample count and score
as missing text; failed blank conversions cannot count as exact successes.

The harness first validates each profile on the existing English prose fixture.
Two timed repetitions then rotate profile order for each document. Every
conversion creates new CLI and OCR processes; timings include rendering, model
loading, inference, JSON transport and a benchmark-only Python PID-registration
wrapper. This is not steady-state inference throughput or cold-host latency.
Filesystem/model-file caches are warm. Two repetitions give preliminary
observations, not statistical confidence intervals.

RSS is the sampled sum for the CLI and its explicitly registered OCR child, at
10 ms intervals. There is no enumeration of unrelated host processes. The
trusted backend uses native threads; unregistered grandchildren would not be
counted. Shared pages can be counted twice and brief peaks missed. A 2-GiB
polling guard, 90-second deadline and output-file limits stop the process group;
these are not OS-enforced memory limits. Single-host values do not establish a
production capacity limit.

`run.json` records the model/config/source fingerprints. `samples.jsonl`
includes every attempt, returned status, OCR diagnostics, errors, text hashes,
quality and measured resources. `summary.json` keeps reference tiers and
languages separate. Raw JSON is saved only for original project fixtures.
Model files, virtual environments and third-party inputs are excluded from the
source bundle. Run `python scripts/check_ocr_profile_results.py REPORT_DIRECTORY`
to verify complete attempt accounting, output fingerprints and recomputed metrics.
Passing that check does not mean that every document was recognized correctly.

Tests:

```sh
python -m unittest discover -s scripts -p test_ocr_profiles.py
python -m unittest discover -s scripts -p test_evaluate_ocr_profiles.py
```
