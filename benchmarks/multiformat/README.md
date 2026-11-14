# Mixed-document adapter

The AnyDoc worker here is used by the eight-engine, 200-document comparison:

```sh
cargo build --release --locked --manifest-path benchmarks/multiformat/anydoc/Cargo.toml --target-dir target/multiformat-anydoc
```

Provision the benchmark-only checkout at `legacy/anydoc-main`. Follow the [mixed-document protocol](../realworld/README.md) for the frozen corpus, other engines and reproduction commands. The old Office-only experiment scripts have been consolidated into that runner. Retained manifests and checks describe development fixtures, not an independent quality leaderboard.
