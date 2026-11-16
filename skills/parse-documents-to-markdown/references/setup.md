# CLI setup and fallback

Use an existing `bulkparse` executable or `npx -y @bulkgrid/bulkparse` first.
Installing the skill does not install the CLI or OCR models. Normal conversion
needs no source checkout, Rust compiler or OCR setup.

## Install for repeated use

With Node.js 20+, install into a fresh tools directory outside the inputs and
outputs. Set `BULKPARSE_TOOLS` to its absolute path:

```sh
npm install --prefix "$BULKPARSE_TOOLS" --no-save --package-lock=false --include=optional @bulkgrid/bulkparse
BULKPARSE_BIN="$BULKPARSE_TOOLS/node_modules/.bin/bulkparse"
"$BULKPARSE_BIN" capabilities
"$BULKPARSE_BIN" parse --help
```

Use this executable in place of `npx -y @bulkgrid/bulkparse` in the main workflow.
Do not put a multiword shell command in `BULKPARSE_BIN`. These examples use a
POSIX shell; on Windows use the corresponding `.cmd` launcher.

Prebuilt packages cover Linux glibc x64/ARM64, macOS x64/ARM64 and Windows x64.
Optional dependencies contain the binary and must stay enabled. If installation
fails, report the actual error before choosing a fallback.

## Rust or source installation

For a Rust environment:

```sh
cargo install bulkparse-cli --locked --features ocr
```

Otherwise, use an existing BulkParse checkout or clone
`https://github.com/bulkgrid/bulkparse` into an unused task directory. Set
`BULKPARSE_ROOT` to its absolute path:

```sh
cargo build --release -p bulkparse-cli --locked --manifest-path "$BULKPARSE_ROOT/Cargo.toml"
BULKPARSE_BIN="$BULKPARSE_ROOT/target/release/bulkparse"
```

Adjust the binary path for a custom Cargo target directory. Add `--features ocr`
when OCR is needed. Check `capabilities` and `parse --help` on the resulting binary.
If neither a compatible package nor a source build is available, report the
missing prerequisite instead of claiming conversion succeeded.

The installed skill folder is separate from a source checkout. Resolve checkout
paths explicitly; do not assume repository files exist relative to the skill.
