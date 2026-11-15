# Mixed-corpus benchmark without warm-up

November 15, 2026. Apple M3 Max, 36 GiB RAM. AC power was confirmed in all nine samples taken before, during and after the run. OCR disabled.

The original diagnostic settings were restored: the same 200 documents in the same order, one sequential pass per fresh worker, no warm-up, ordinary structured parsing followed by Markdown rendering, and no JSON verification within the timed run. The pre-optimization build ran first. RAYON, OMP, OPENBLAS and MKL thread controls were each set to one. The original harness was checked against its recorded SHA-256 to verify these settings.

| Run | Outputs | Median ms | Corpus seconds | CPU seconds | Peak worker MiB |
| --- | --- | ---: | ---: | ---: | ---: |
| Historical published run, November 13 | 200/200 | 2.93 | 16.359 | 16.326 | 1,883.6 |
| Pre-optimization build, November 15 | 200/200 | 3.04 | 16.380 | 16.213 | 2,142.7 |
| Optimized build, November 15 | 200/200 | 3.05 | 13.892 | 13.713 | 1,976.4 |

The README uses the optimized build’s complete 200-document pass, including all 19 formats. Each format’s time is its median successful document time from that same pass. The separate spreadsheet-only measurements are not substituted into this table.

## Memory and output checks

The optimized build measured **7.8% lower peak RSS** than the pre-optimization build in this AC-powered pair. Its peak remains **4.9% above the historical published measurement**. These single-pass results do not isolate the cause of that historical difference or demonstrate a persistent memory regression. Allocation patterns, allocator retention and machine state are possible contributors, not proven explanations. The original November 13 executable was not retained, so the historical row uses its preserved measurements; both builds in the new pair were re-executed.

All **200 Markdown outputs, statuses, diagnostics and image metadata** match both the pre-optimization build and the historical published output. All input and executable hashes were checked. No parser code changed for this rerun. Equality establishes no detected output regression, not perfect extraction fidelity.

Timing includes reads, parsing and Markdown rendering, excluding worker startup and response transport. RSS is macOS `/usr/bin/time -l` lifetime high-water memory, including initialization and transport; it is not parser-only live heap. No warm-up memory is included. OS file caches were not flushed. Other providers retain their separate November 13 measurements.

Two preliminary no-warm-up checks were made before AC power was restored and monitored. They are retained in [preliminary-checks.json](preliminary-checks.json), with their resource logs and different build order/settings disclosed. Power consistency was not established, so neither supplies the README values. The final run was selected by verified power and configuration, not by memory outcome.

## Evidence and reproduction

[Per-document measurements](measurements.json) · [Summary](summary.json) · [Build and run identity](identity.json) · [Power observations](power.json) · [Baseline resource log](baseline-resources.log) · [Candidate resource log](candidate-resources.log) · [Frozen corpus](../../mixed-200-2026-11-13/corpus-manifest.json) · [Original diagnostic settings](../../biff-legacy-2026-11-13/diagnostic/identity.json). Diagnostics and image metadata are stored as complete-object hashes; document text and executable binaries are not distributed.

On AC power, preserve the baseline `office_bench` from revision `1f758df`, then build the candidate. The prior [build identity](../identity.json) records parser source and executable hashes. Run:

```sh
cargo build --release --locked -p bulkparse --example office_bench
pmset -g batt
python3 scripts/benchmark_regressions.py \
  --baseline-binary /absolute/path/to/baseline-office-bench \
  --candidate-binary target/release/examples/office_bench \
  --modes default --reverse-order \
  --out reports/new-no-warmup-check
pmset -g batt
```

Do not add `--warmup`. The default is one measured pass. Keep input ordering unchanged and avoid concurrent performance jobs. Record AC power throughout the run when possible; this report sampled it every five seconds. Reproduction requires macOS for the recorded worker resource counters.
