# Frozen holdout v1

This corpus was created after the positioned-PDF parser was frozen on 2026-11-10. The Rust source and release executable fingerprints are in `freeze.json`. The parser was not changed in response to these results. This version is now an observed evaluation set; it must not be presented as unseen again after tuning against it.

## Composition and independence

- Twelve external PDFs come from PDFPlumber revision `4c64b92d5caccd71c645e98e0fabb0c4dba7ff45`. Four cases were selected for transcript, table, rotation and column coverage; eight were selected by a seeded path hash. `selection.json` records the rule and paths before conversions. These include test fixtures as well as real documents; they are not a representative random sample of production uploads.
- Twelve developer-authored stress cases cover two/three columns, a spanning title, a narrow gutter, a ruled table, Arabic/Latin mixing, font subsets, rotation, multiple pages, and raster-only/mixed raster pages. CoreText/CoreGraphics generated the PDFs from `synthetic-spec.json`; expected text was authored before rendering. They are synthetic, not an independent human-curated benchmark. The original bytes are bundled in `fixtures/holdout` because CoreGraphics IDs and metadata can change when regenerated.
- No input SHA-256 matches any of the previous 173 development inputs. This establishes absence of byte-identical overlap, not absence of semantically duplicated content. The two NICS rotation variants intentionally share content.
- The initially investigated arXiv benchmark download server could not be reached (HTTPS connection reset; HTTP 502). No results from that corpus are claimed.

## Reference tiers

| Tier | Count | Meaning |
|---|---:|---|
| `authored_gold` | 12 | Original logical text and intended block order, written before generation |
| `upstream_reference` | 1 | PDFPlumber's saved full-page transcript text |
| `silver_pdfkit` | 11 | Apple PDFKit text extracted independently of the three evaluated engines |

Do not combine these into an overall accuracy score. PDFKit is another imperfect extractor. Its text can disagree with valid output, and legitimate removal of footers, annotations or other content affects overlap. The Arabic generator also exposes ambiguous glyph mappings: PDFKit reproduces the same presentation forms and some Persian yeh characters. The authored text remains the target, but this discrepancy cannot be attributed solely to BulkParse.

The main metric remains literal token recall, with adjacent-token overlap as a limited order signal. A supplemental NFKC token-recall diagnostic was added after the first Arabic result to distinguish compatibility forms from lost text; literal scores are retained. NFKC does not repair incorrect character identity or direction. Authored block-pair recall penalizes missing blocks; it is not a generic reading-order estimator. The table check requires exact cells in Markdown table rows, not merely the presence of every word.

## Restore and run

Run commands from the repository root. Third-party PDFs, raw references, converted text, model files and installed engines stay under ignored `target/`. Public download URLs and content hashes are recorded in `downloads.json`. Third-party redistribution rights have not been assessed, so their documents and converted text are excluded from the source bundle.

```sh
# Requires curl and macOS Swift/PDFKit for the silver references.
# Existing mismatched files cause an error; expected hashes are never relaxed.
python3 scripts/restore_holdout.py

# Provision the pinned AnyDoc checkout separately, then build in isolation:
CARGO_HOME="$PWD/target/competitors/cargo-home" cargo build --locked \
  --manifest-path anydoc-main/Cargo.toml --release --example convert \
  --target-dir target/competitors/anydoc

# Python 3.12 was used for the measured Docling environment.
python3.12 -m venv target/competitors/docling-venv
target/competitors/docling-venv/bin/python -m pip install \
  -r benchmarks/holdout/docling-environment.txt

# After provisioning the pinned local Heron model (see below):
HF_HOME="$PWD/target/competitors/hf-cache" python3 scripts/evaluate_holdout.py \
  --engines benchmarks/holdout/engines-all.json --repeats 3 \
  --out reports/holdout-comparison-new --timeout 120
```

The Docling profile uses version 2.150.0, docling-parse 7.23.0, Heron learned layout on CPU with four configured threads, OCR off, and table recognition off. AnyDoc uses its native default pipeline, including its table heuristics. BulkParse has native text only, without table reconstruction. These are explicit capability profiles, not interchangeable features. The saved pip package list pins versions, but is not a wheel-hash lock or a claim of a hermetic installation.

Download `*.json` and `*.safetensors` from Hugging Face repository `docling-project/docling-layout-heron` at revision `8f39ad3c0b4c58e9c2d2c84a38465abf757272d8` into `target/competitors/models/docling-project--docling-layout-heron`. Record that repository/revision in `target/competitors/docling-model.json` as `{"repo":"docling-project/docling-layout-heron","revision":"8f39ad3c0b4c58e9c2d2c84a38465abf757272d8"}`. The evaluation fingerprints the model files and never downloads models. Offline flags are not an OS network sandbox.

The local AnyDoc checkout has no verified Git revision; its source-tree fingerprint, lockfile and actual executable hash are retained with the results. A matching version string alone does not establish identical bytes. Docling was smoke-tested on the original native-and-empty PDF; AnyDoc's working conversion was verified on the generated prose case. AnyDoc rejects the original native-and-empty fixture with an OCR-needed error for its empty second page.

The evaluator checks the frozen Rust source and executable. On another machine, rebuild the same source, verify its source digest against `freeze.json`, and explicitly create a new freeze file recording that build's executable hash; pass it with `--freeze`. Do not silently replace the original freeze or claim byte-identical reproduction. PDFKit reference differences between OS versions similarly require a separate corpus version.

## Measurement limits

Three full process runs per engine per input are measured, rotating engine order. There are no timed warmups; first-load effects are included. Wall time includes Python imports, model loading and Markdown serialization. This strongly affects Docling and is **not** a persistent-worker/library-throughput comparison. CPU and peak RSS are measured per subprocess. There is no hard memory cap on macOS. Failures remain in the quality denominator and are excluded from successful-conversion timing medians. Nondeterministic outputs are flagged instead of assigned a single quality score.

The first Docling harness attempt incorrectly resolved the virtual environment's Python symlink, losing its installed packages. That infrastructure run was retained under `target/provisioning`, excluded from comparison, and corrected before the final run. A regression test now checks virtual environment preservation. Raw preliminary runs remain local; the final report includes harness source snapshots, hashes and all recorded samples.

See [results and next priorities](../../docs/holdout-evaluation.md).
