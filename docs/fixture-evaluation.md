# First upstream fixture evaluation

**Follow-up:** the baseline below is preserved. See [implemented fixes and same-corpus rerun](fixture-fixes.md) for the current results.

Run date: 2026-11-10. BulkParse 0.1.0, release CLI, Apple Silicon macOS.

**BulkParse produces output for most inputs, but is not ready to promise high-fidelity conversion. The highest-priority finding is silent DOCX content loss despite `complete` status and strict mode.** This evaluation preserves the current implementation as a baseline; it does not change parser behavior.

## Scope and results

The evaluator found 173 files with supported extensions in the existing AnyDoc and Docling checkouts. It distinguishes 100 primary quality fixtures, 13 malformed/resource/encryption cases, and 60 supplemental files (including PDF figures, companion text, documentation and ODF PDF exports). The 60 supplemental files are not counted as 60 independent document-quality tests.

| Primary quality fixtures | Files | Reported complete | Reported partial | Failed |
| --- | ---: | ---: | ---: | ---: |
| DOCX | 47 | 10 | 37 | 0 |
| PDF, including OCR fixtures | 29 | 0 | 29 | 0 |
| CSV | 10 | 8 | 0 | 2 |
| Markdown | 14 | 14 | 0 | 0 |
| **Total** | **100** | **32** | **66** | **2** |

“Complete” is the engine's own status, not a correctness verdict. Nine primary fixtures produced empty or whitespace-only Markdown, all marked partial: six Docling OCR PDFs, one scanned AnyDoc PDF, one Docling form/table PDF, and a DOCX containing DrawingML content. All PDFs are deliberately marked partial by the current text-only implementation, even when text extraction works.

| Separate track | Files | Results |
| --- | ---: | --- |
| Malformed | 9 | 8 rejected; 1 partial |
| Resource abuse | 3 | 2 rejected by resource limits; 1 partial because the image was omitted |
| Encrypted PDF | 1 | Rejected with `encrypted` |
| Supplemental | 60 | 10 complete; 50 partial |

Across all 173 inputs: 42 complete, 118 partial and 13 rejected. No observed crashes, invalid JSON, renderer failures or harness stops. “Rejected” in the robustness tracks is often the intended outcome, not a quality regression.

## Reviewed findings and priorities

### 1. Release blocker: content controls silently drop DOCX content

Fixture: `docling/docx/sources/docx_sdt_table_cells.docx`.

BulkParse reports `complete`, with no diagnostics. Direct inspection of `word/document.xml` confirms three content controls wrapping source text. The output loses the standalone value and two table-cell values. The saved Docling output retains them and has a three-column table; BulkParse's populated table has two columns. Re-running with `--strict --to json` still returns `complete`.

Cause confirmed in the current implementation: table traversal only visits direct `w:tr` and `w:tc` children, bypassing row/cell content-control wrappers. Treating the document as complete hides a real omission.

Next: traverse structural wrappers at the correct table level, preserve row/cell boundaries, and report unsupported wrappers until their content can be preserved. Add an original synthetic regression fixture and assert both content and table shape, including strict-mode behavior.

### 2. Release blocker: inherited list numbering is not diagnosed

Fixture: `docling/docx/sources/docx_list_resumed_after_table.docx`.

The output retains paragraph text but has no list blocks or numbering and reports `complete`. The source uses a paragraph style with numbering; the current warning checks direct paragraph numbering only. Saved Docling output preserves the list around the intervening table. The more explicit AnyDoc numbering fixture is marked partial, demonstrating inconsistent detection of the same missing capability.

Next: resolve numbering through paragraph-style inheritance, continuation and level overrides. Until supported, inherited numbering should trigger a content-gap diagnostic too.

### 3. CSV dialect and encoding coverage need an explicit contract

Docling's `csv-tab.csv`, `csv-pipe.csv` and `csv-semicolon.csv` each become a one-column table rather than the reference's 12 columns, with `complete` status. All three have 100% token overlap under the evaluator's simple metric: preserving words does not preserve table structure.

AnyDoc's semicolon fixture fails because comma parsing gives inconsistent row widths; its UTF-16 fixture fails the UTF-8 check. Those are the only two failures among the 100 primary quality fixtures. BulkParse currently supports comma CSV and UTF-8 by design; a `.csv` extension alone does not express the source dialect.

Next: add explicit delimiter/encoding options, then carefully bounded dialect detection with ambiguity reporting. Do not promote word retention to evidence of a correct table. Header conventions also differ: BulkParse keeps the first row as data unless requested, while reference outputs often treat it as a header. Header-only differences are configuration differences, not dropped content.

### 4. DOCX style semantics are lost even when all words survive

Fixtures: AnyDoc `handmade-blockstyle.docx` and Docling `docx_code_blocks.docx`.

Both report complete and have 100% token overlap, but code-styled paragraphs are ordinary paragraphs; the AnyDoc fixture also loses quotation semantics. Code punctuation is consequently escaped as prose. The AnyDoc numbering fixture additionally shows missing inherited bold/italic semantics, although its numbering warning already marks the document partial.

Next: model inherited paragraph/run styles and recognized code/quote semantics. Separate textual coverage from structural fidelity in diagnostics and evaluation.

### 5. The PDF backend needs more than OCR

All six primary Docling OCR PDFs produce empty output with `pdf_no_text`; the scanned AnyDoc PDF does too. That is a known missing capability, correctly surfaced as partial rather than fabricated text.

Native-text PDFs also expose problems. Inspection of `newspaper-00.pdf` output shows concatenated words and mixed reading order compared with the saved reference. Its token-recall proxy is only 2.7%. Arabic in `right_to_left_01.pdf` is reversed relative to the saved reference; the proxy is 11.4%. `elsevier-00.pdf` has 2.6% proxy recall. These are triage signals, not visual correctness scores; this pass did not render pages or independently annotate the PDFs.

Next: evaluate positioned text extraction, spacing, Unicode/bidirectional ordering and page-level reading order. Add OCR selectively for pages without usable text. Tables, forms and figures need their own structural checks.

### 6. Robustness is promising but recovery differs

AnyDoc's deep-XML and ZIP-expansion fixtures are rejected with `max_depth` and `max_expanded_bytes`. Its image-expansion fixture returns partial because images are not decoded; this does **not** validate an image decoder's resource limits.

Empty and truncated DOCX files and all three ragged CSVs are rejected. AnyDoc's two recoverable malformed-XML fixtures and its corrupt-optional-styles fixture are also rejected; their reference snapshots contain output. The missing-styles fixture returns partial output.

Next: distinguish optional-part damage from a damaged main document. Consider narrowly defined recovery for optional styles, with diagnostics; do not relax archive/XML limits to match another parser's recovery behavior.

## Comparison method and practical limits

Each accepted input is converted twice through the release CLI: JSON, then Markdown. Partial JSON or renderer diagnostics make the combined outcome partial. Default parser options are used throughout. Competitor engines were **not** run: this is comparison against their committed snapshots/groundtruth, not a live speed or quality competition.

References were found for 92 primary fixtures. Two corresponding inputs failed, and two AnyDoc PDF snapshots represent errors rather than Markdown, leaving 88 primary text comparisons. There are 96 text comparisons across all tracks; only two are whitespace-normalized exact Markdown matches (the tiny Strict and Transitional DOCX fixtures). Exact equality is intentionally strict and affected by escaping, headers and formatting policy; it is not the pass rate.

The lexical diagnostic lowercases Unicode word tokens, removes simple Markdown link destinations and HTML tags, and computes multiset overlap. It ignores ordering and cannot validate tables, lists, equations, code formatting or visual layout. Manual review compared saved Markdown/JSON and, for the critical DOCX cases, the package XML. No human visual-quality grading or full independent gold annotation was performed.

Per-conversion budgets: 10-second wall timeout, 8-second soft CPU limit, 16 MiB output-file limit, and the parser's existing default limits. The harness uses a separate process for every conversion and `wait4` for peak RSS. Linux additionally applies a 512 MiB address-space bound; macOS has no hard memory cap in this harness. The highest observed single-process RSS was 57,616 KiB (about 56.3 MiB) for `elsevier-00.pdf`. These small-corpus observations and timings include process startup/polling effects and establish no comparative performance claim.

## Reproduction and provenance

```sh
cargo build --offline --locked --release -p bulkparse-cli
python3 -m unittest discover -s scripts -p test_evaluate_fixtures.py -v
python3 scripts/evaluate_fixtures.py --out reports/my-new-evaluation
```

The output directory must be new. Override `--anydoc`, `--docling`, or `--binary` for different local paths. The script does not download inputs or execute document code. Supported-extension selection and group classification are explicit in the script.

The saved run is in reports/fixture-evaluation-2026-11-10-run2 (historical artifact; not distributed):

- `manifest.jsonl`: each input/reference path, source URL, byte size and SHA-256.
- `results.jsonl`: per-file outcomes, diagnostic codes, model counts, lexical comparisons and child-process measurements.
- `summary.json`: totals by project, track and format.
- `run.json`: binary/source/harness hashes, environment, options and budgets.
- `provenance.json`: license-file hashes and verification that selected input/reference hashes stayed unchanged.

The downloaded checkouts have no verified Git revision; the report makes no commit-pinning claim. File hashes identify the evaluated bytes. Raw conversion outputs remain under ignored `target/fixture-evaluations/fixture-evaluation-2026-11-10-run2/` for local inspection. Original documents and full converted text are not copied into the shared report/source archive. Individual fixture redistribution rights remain unreviewed; repository-level license files are not treated as proof that every embedded third-party document can be redistributed.

The next implementation milestone should fix unreported DOCX omissions and CSV dialect ambiguity, then rerun this exact manifest before expanding format count.
