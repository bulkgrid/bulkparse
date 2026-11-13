# Validation

Run from the repository root:

```sh
cargo fmt --all --check
cargo test --locked --workspace --all-features
cargo test --locked -p bulkparse --no-default-features
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
python3 scripts/run_harness_tests.py
```

Python harness tests require the dependencies in `benchmarks/requirements-test.txt`. OCR integration additionally requires the backend environment and provisioned models. See the [CI workflow](../.github/workflows/ci.yml) for platform jobs and the binding READMEs for their build and smoke tests.

The authored Rust tests cover parsing, streaming, limits, malformed inputs, OCR provider contracts and worker lifecycle. These tests are not a whole-document accuracy score. The published benchmarks (historical artifact; not distributed) separately measure document coverage, quality where reference annotations exist, speed and memory.

## Platform coverage

Native bindings have been exercised on Apple Silicon macOS. The CI workflow defines Linux, Windows and macOS jobs; a checked-in workflow is not evidence those jobs have passed. Broader browser and production-server validation remains necessary. Extended Node/WASM PDF parity has required `--liftoff-only` because of an optimizing-compiler memory issue.

External corpora, downloaded converters and model weights are not committed. Historical regression scripts may require their own regenerated baseline or input set; obsolete run directories are not part of the release.

## Initial release preparation

On November 13, 2026, the all-feature workspace passed 315 Rust tests. The isolated Python harness ran 89 tests without failures; two historical-artifact checks were explicitly skipped because their old runs are not distributed. Formatting passed. Strict workspace Clippy now passes with all targets/features and warnings treated as errors. The lint fixes move test modules, name internal types, group table-output arguments and simplify equivalent expressions. No parser feature or test was removed. Remote CI results must be checked separately.
