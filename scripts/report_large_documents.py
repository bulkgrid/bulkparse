#!/usr/bin/env python3
"""Publish bounded-recovery evidence, clearly separating partial from full output."""
import json,re
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];OUT=ROOT/'reports/large-documents-2026-11-12'
def main():
 s=json.loads((OUT/'benchmark-final/summary.json').read_text());audit=json.loads((OUT/'regression-checks.json').read_text());assert not s['conversion_regressions'] and not s['default_output_changes'] and not s['previously_successful_outputs_changed']
 assert all(not c['issues'] for c in audit['source_checks']) and all(r['unchanged'] for rs in audit['development'].values() for r in rs)
 versions=s['versions'];names={'before':'Previous defaults','default':'Current defaults','bounded':'Current + bounded recovery'}
 table=['| Profile | Returned / 112 | Resource-limit recoveries | Median seconds / 112 attempts | Peak RSS MiB |','| --- | ---: | ---: | ---: | ---: |']
 for key in names:
  r=versions[key];table.append(f'| {names[key]} | {r["returned_all_passes"]}/112 | {r["resource_recovered_count"]} | {r["median_corpus_seconds"]:.3f} | {r["lifetime_peak_rss_bytes"]/2**20:.1f} |')
 records=sum(c['checked'] for c in audit['source_checks'] if c['document_id'].startswith('csv-'));cells=sum(c['checked'] for c in audit['source_checks'] if not c['document_id'].startswith('csv-'))
 lines=['# Large-document handling — November 12, 2026','',
 '**All 15 resource-limited inputs now return useful partial data when `partial_on_limit` is enabled. Defaults still return 97/112; bounded recovery returns 112/112. This is not 112 complete conversions.**','',
 'The original limits remain unchanged: 32 MiB input, 32 MiB text, 200,000 nodes, depth 64, 100,000 table cells, 2,000 pages, 10,000 archive entries and 64 MiB expanded data. The feature is opt-in; strict mode never accepts recovered partial output.','',
 '## What changed','',
 '- **CSV:** recover a prefix of complete records that fits the existing cell, text and node budgets. Never return a half-built row.',
 '- **XLS:** bound the record index, reserve node budget for output, and keep only completed worksheets before a limit is reached. Discard the unfinished worksheet; do not silently label it complete.',
 '- **XLSX/XLSM and ODS:** stream XML from capped ZIP member reads instead of building a full recovery DOM. Return populated cells as three-column Row/Column/Value records with original coordinates. This avoids expanding sparse sheets into large empty rectangles. ODS repeated rows/cells stop at the output budget; blank repeated ranges advance coordinates without allocation.',
 '- **Diagnostics:** every recovered result has `status: partial` and `resource_limit_partial`. XML previews additionally report `spreadsheet_cell_preview`, including retained cell count and the last retained source coordinate. CSV and XLS report their truncation boundaries. Malformed content encountered in the consumed prefix, invalid archive paths, DTDs, depth violations and cancellation remain errors.','',
 'BulkParse owns this extraction code. The optional Office feature adds quick-xml for streaming XML tokenization; it does not wrap another document converter. CSV recovery also works with all optional parser features disabled.','',
 '## Measurements','',*table,'',
 '**Apple M3 Max, 36 GiB RAM.** One excluded full warm-up and five measured sequential full-corpus passes for each profile, with rotating profile order. Each profile has one persistent worker. OCR is off. Reads, parsing and Markdown export are timed; startup and response serialization are excluded. Peak RSS is worker-lifetime memory including warm-up, not a hard cap or aggregate controller/process-tree memory.','',
 'Default time is essentially unchanged. Recovery performs additional work and returns more content, so its total is not an equal-output speed comparison. Peak RSS varies across these persistent runs and remains approximately 1.1–1.2 GiB; **no memory reduction is established**. Parser budgets do not impose a hard process-memory ceiling.','',
 'Recovery is a second attempt after a normal resource-limit failure. The failed attempt drops its intermediate structures; the second attempt uses the original caps, not larger ones. Total work can therefore exceed one attempt’s work. Input-size rejection is not bypassed.','',
 'Outcomes and Markdown hashes are deterministic across all five measured passes. Current defaults preserve the previous success/failure outcomes and all output hashes. Bounded mode preserves all 97 previously returned Markdown outputs. There are 11 complete-status outputs and 101 partial-status outputs in bounded mode; 15 of those partials are resource recoveries.','',
 '[Raw summary and timing samples](benchmark-final/summary.json) · [Binary identities, options and hardware](benchmark-final/identity.json) · [Source snapshot](benchmark-final/source-snapshot/) · [Per-document outputs](benchmark-final/predictions/)','',
 '## Regression and source validation','',
 '- 203 Rust core/CLI tests pass, plus six regression tests with optional parser features disabled.',
 '- All five Node binding tests pass, including explicit partial recovery and strict-mode rejection. A CLI smoke test returns partial JSON, truncation diagnostics and the expected exit code 2.',
 '- New tests cover complete-record boundaries, strict-mode behavior, completed-sheet rollback, UTF-8/shared-string preservation, phonetic annotations, repeated-cell amplification, expanded-byte limits, duplicate coordinates, DTD/malformed XML rejection, depth limits and empty-result rejection.',
 '- All 97 previously successful real-world outputs are unchanged with defaults and bounded recovery. All 81 separate Office development fixtures are unchanged under both modes.',
 f'- All **{cells:,} retained XLSX/XLSM/ODS source cells** match their original coordinates and source values after whitespace normalization. All **{records:,} retained CSV records** match all source columns. No quality score is inferred from these checks.',
 '- The four XLS recoveries have synthetic BIFF boundary/regression coverage; their individual source cells were not independently audited in this run.','',
 '[Detailed source and regression checks](regression-checks.json) · [Rust tests](tests.log) · [Minimal-feature tests](minimal-tests.log)','',
 '## Remaining limitations','',
 'The 15 inputs still cannot be fully extracted into the current in-memory document model within the default budgets. Recovery deliberately omits content. XML cell previews do not reconstruct grid layout, merged cells, styling, comments, hyperlinks or charts. XLSX previews retain raw cached values, so numeric/date display formatting is omitted and formulas are not evaluated. A shared-string budget can stop extraction before a referenced value is available; it never invents a replacement value.','',
 'Unexamined tails and incomplete ZIP-member checksums are not certified by a partial result. Use the normal path when complete validation is required. Malformed data encountered before the stopping point remains an error. Limits and cancellation are retained, but a production worker deadline/process memory policy is still needed for a hard operating-system boundary.','',
 'This corpus has informed these changes and is now regression/development evidence, not an unseen holdout. Full streaming output for arbitrarily large spreadsheets, richer preview fidelity, and lower process memory remain further work. The earlier eight-converter comparison and 37-document assistant quality review remain unfinished; no new competitor ranking or paid judging result is claimed.','',
 '## Use and reproduce','',
 'Rust: `Options { partial_on_limit: true, ..Default::default() }`. Node/WASM options: `{ partial_on_limit: true }`. Python: `parse(data, filename="large.xlsx", partial_on_limit=True)`. Rebuild the native/WASM binding to use the updated core.','',
 'CLI: `bulkparse parse large.xlsx --partial-on-limit --to json`. This flag conflicts with `--strict`; partial output exits with code 2. The JSON output preserves the partial status and diagnostics. Markdown alone cannot communicate every omitted region.','',
 'From the repository root, with the frozen 112 inputs restored:','',
 '```sh','cargo test -p bulkparse -p bulkparse-cli --offline','cargo test -p bulkparse --no-default-features --test regressions --offline','cargo build -p bulkparse --example office_bench --release --offline','python3 scripts/benchmark_large_documents.py --out reports/large-documents-rerun','target/competitors/docling-venv/bin/python scripts/validate_large_documents.py','```','',
 'The validation script checks the recorded `benchmark-final` artifacts and verifies the current executable hash; adjust its run path explicitly to validate a separate rerun. `before-office-bench` and the previous source snapshot are retained here. The frozen source manifest records every input URL and hash; no corpus file was removed or selected because recovery succeeded.','']
 (OUT/'README.md').write_text('\n'.join(lines))
 section=['<!-- realworld-benchmark:start -->','### Real-world large-document handling','',
 '**112 public documents across 13 formats exposed limits the smaller fixtures missed.** Legacy encoding, XLS chart and RTF fixes first increased returned outputs from 85 to 97. An optional bounded-recovery mode now returns partial data from the 15 remaining inputs without increasing the configured limits.','',*table,'',
 '**The 15 recoveries are explicitly partial—not full conversions.** Default behavior is unchanged. Recovery retains complete CSV records, completed XLS sheets, or coordinate/value previews for XLSX/ODS. Formatting, later content and unexamined tails may be omitted.','',
 'One excluded warm-up and five rotating sequential passes per profile. Recovery completes more work, so these totals are not an equal-output speed comparison. The 97 previously returned outputs and 81 separate Office fixtures are unchanged. Peak memory remains high; no memory-reduction claim is made.','',
 'This corpus has informed fixes and is now regression evidence. The broader eight-converter comparison and assistant quality review remain unfinished; no quality leaderboard is claimed.','',
 '[Results, source checks and how to enable recovery](reports/large-documents-2026-11-12/README.md) · [Earlier fixes](reports/realworld-fixes-2026-11-12/README.md) · [Corpus and study status](benchmarks/realworld/README.md)','<!-- realworld-benchmark:end -->']
 p=ROOT/'README.md';text=p.read_text();a=text.index('<!-- realworld-benchmark:start -->');b=text.index('<!-- realworld-benchmark:end -->')+len('<!-- realworld-benchmark:end -->');p.write_text(text[:a]+'\n'.join(section)+text[b:])
if __name__=='__main__':main()
