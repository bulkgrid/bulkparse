# PDF corpus adapter

This directory contains the AnyDoc JSONL worker used by the current nine-profile PDF benchmark. Provision its benchmark-only source checkout at `legacy/anydoc-main` and build from the repository root:

```sh
cargo build --release --locked --manifest-path benchmarks/same-machine/anydoc/Cargo.toml --target-dir target/same-machine-benchmark/anydoc-build
```

The former three-engine runner has been consolidated into `scripts/benchmark_broad.py`. Follow the [current PDF protocol](../broad/README.md) for the pinned 200-PDF corpus, model setup, effective settings, warm-up, five measured passes and scoring. All failed attempts remain in the scoring denominator.
