# Publishing BulkParse

The release workflow is `.github/workflows/release.yml`. It builds and tests the
actual packages before publishing. Manual runs default to **build artifacts only**;
pushing a matching stable `vX.Y.Z` tag enables publication. A manual run can also
publish when its `publish` input is checked and its ref is that version tag.
Attempting to publish from a branch fails before building. Nothing has been
published merely by adding this workflow.

This follows the existing Bulkgrid SDK/CLI publishing setup: Node 24, npm
11.12.1, `--access public`, and GitHub OIDC trusted publishing with no npm token.
The Python path follows the Python SDK's separate build/publish jobs and `pypi`
environment. Unlike the monorepo's `sdk-v*`, `cli-v*` and `python-sdk-v*` tags,
this single-product repository uses one `vX.Y.Z` tag for all bindings and binaries.
There is no separate CLI version; it ships with `@bulkgrid/bulkparse`.

## Distribution

| Channel | Package / artifact | Installation after publication |
| --- | --- | --- |
| npm / Node / TypeScript | `@bulkgrid/bulkparse` | `npm install @bulkgrid/bulkparse` |
| npm CLI | Same package | `npx @bulkgrid/bulkparse parse report.docx` |
| npm / browser WASM | `@bulkgrid/bulkparse-wasm` | `npm install @bulkgrid/bulkparse-wasm` |
| PyPI | `bulkparse` | `pip install bulkparse` |
| crates.io | `bulkparse` | `cargo add bulkparse` |
| Rust CLI | `bulkparse-cli` | `cargo install bulkparse-cli --features ocr` |
| GitHub Releases | Five CLI archives and `SHA256SUMS` | Extract the matching archive |

The Rust release also publishes `bulkparse-ocr`, `bulkparse-ocr-native`, and
`bulkparse-pool`, in dependency order. Python imports remain `import bulkparse`.
PyPI and crates.io names are independent of npm scopes; their availability and
your account's rights must be confirmed before the first publication.

## The `@bulkgrid` namespace

You must own or have publishing rights in the **npm** user/organization named
`bulkgrid`. The GitHub organization of the same name does not grant npm rights.
Public scoped npm packages use `--access public`; the pipeline sets this explicitly.

There are seven npm packages to configure:

- `@bulkgrid/bulkparse`
- `@bulkgrid/bulkparse-wasm`
- `@bulkgrid/bulkparse-linux-x64-gnu`
- `@bulkgrid/bulkparse-linux-arm64-gnu`
- `@bulkgrid/bulkparse-darwin-x64`
- `@bulkgrid/bulkparse-darwin-arm64`
- `@bulkgrid/bulkparse-win32-x64-msvc`

The main package pins each platform package to the same version as an optional
dependency. npm installs the matching platform; no Rust compiler, post-install
build, or binary download script is needed. The wrapper rejects mismatched versions.
Platform packages contain the addon and CLI, not OCR model weights.

## One-time account configuration

Create GitHub environments `pypi` and `crates` in
`bulkgrid/bulkparse`. Restrict release tags/branches as appropriate for your team.
Publication jobs run only for matching tags in that repository, never branches or
forks. The npm job has no GitHub environment, matching the SDK and CLI workflows.

1. **npm:** secure the `bulkgrid` scope and publishing access. Configure each of
   the seven packages' trusted publisher with GitHub owner `bulkgrid`, repository
   `bulkparse`, workflow `release.yml`, and leave the environment field blank.
   Each package needs its own trusted publisher: the SDK and CLI's configuration
   does not authorize a different package/repository automatically. If npm
   requires the first version to exist before configuring trust, publish the
   tested npm tarballs from a dry run once with your authenticated npm account,
   then configure trusted publishing. The pipeline does not accept `NPM_TOKEN`
   or copy credentials from the monorepo. npm generates provenance automatically
   when supported for the trusted publication.
2. **PyPI:** register a pending publisher for project `bulkparse` (or configure its
   existing project), owner `bulkgrid`, repository `bulkparse`, workflow
   `release.yml`, environment `pypi`. No PyPI token is used by this pipeline.
3. **crates.io:** verify access to the five crate names. For first publication,
   provide a suitably scoped `CARGO_REGISTRY_TOKEN` in the `crates` environment.
   Then configure each crate's trusted publisher for the same repository/workflow,
   environment `crates`, and remove the bootstrap token. The workflow switches to
   `rust-lang/crates-io-auth-action` for short-lived credentials.
4. **GitHub Releases:** the job uses its built-in `GITHUB_TOKEN`; no personal token
   is needed. It creates the release only after all registry jobs succeed.

Do not put credentials into source files or chat. Public release setup does not
require publishing the private Bulkgrid application repository.

Official setup references:
[npm scopes](https://docs.npmjs.com/about-scopes/),
[npm trusted publishing](https://docs.npmjs.com/trusted-publishers/),
[PyPI pending publishers](https://docs.pypi.org/trusted-publishers/creating-a-project-through-oidc/),
[crates.io authentication action](https://github.com/rust-lang/crates-io-auth-action).

## Build, inspect and release

1. Update the workspace version and `workspace.dependencies.bulkparse.version`
   in `Cargo.toml`, the CLI's native-OCR dependency version, and the Node/WASM
   `package.json` versions together. Regenerate `Cargo.lock`. Python derives its
   version from the Rust workspace. `python3 scripts/release/check.py` checks this.
   This pipeline accepts stable `X.Y.Z` versions; prerelease support is not enabled.
2. Commit the intended source and run **Release packages → Run workflow**, leaving
   **publish unchecked**. This
   builds five platform packages, five Python ABI3 wheels, a Python source
   archive, WASM, and five standalone CLI archives. It does not publish.
3. Inspect the `release` Actions artifact and checksums. Build jobs test the
   installed Node packages/CLI and Python wheels; the source archive is rebuilt
   independently; WASM is installed and exercised from its packed archive.
4. Once account setup and validation pass, tag that reviewed commit and push:

   ```sh
   git tag v0.1.0
   git push origin v0.1.0
   ```

5. Downloadable binaries and checksums appear on the GitHub Release after
   registry publication succeeds. Versioned packages cannot be overwritten.
   Rerun failed jobs to finish a partial publication; npm checks matching archive
   integrity, PyPI skips existing distributions, and Cargo skips existing versions.
   Never move a published tag or use the same version for changed source.

To publish manually instead of relying on the tag-push trigger, select the
existing `vX.Y.Z` tag as the workflow ref and check **publish**. For example:

```sh
gh workflow run release.yml --repo bulkgrid/bulkparse --ref v0.1.0 -f publish=true
```

For a manual build without publishing, select a branch and leave the input false.

The main npm package is published after its platform dependencies. A registry
failure cannot undo a successful publication to another registry; fix the account
configuration and rerun failed jobs using the original tag/artifacts.

## Platform and OCR boundaries

- Node/CLI: Linux x64 and ARM64 built on Ubuntu 22.04 (glibc 2.35+), macOS x64 and
  ARM64 with deployment target 11, and Windows x64. Other targets are not advertised.
- Python: CPython 3.10+ stable-ABI wheels for the same architectures; Linux wheels
  target manylinux 2.28. Free-threaded Python and musl wheels are not built.
- OCR remains Rust inference. Models, ONNX Runtime and PDFium are installed
  explicitly with `bulkparse-setup-ocr --output DIRECTORY` (npm) or
  `node setup-ocr.mjs --output DIRECTORY` from a CLI archive. Standard parsing
  needs none of those downloads. Setup currently pins Linux x64/ARM64 and macOS
  ARM64; Windows and Intel macOS require manual OCR library provisioning. Python
  users can run the same setup utility from a CLI archive or provide their own
  matching native configuration. See [OCR configuration](ocr-bindings.md).
- WASM has a separate package and does not include native OCR model inference.

The package tests confirm loading and document conversion on the release runners.
They do not replace the OCR accuracy benchmark or production service validation.

## Development checks without publishing

```sh
python3 scripts/release/check.py
npm run build --prefix bindings/node
cargo build --locked --release -p bulkparse-cli --features ocr
npm run build --prefix bindings/wasm
npm run release:pack --prefix bindings/node -- native
npm run release:pack --prefix bindings/node -- shared
npm run release:test --prefix bindings/node
```

Outputs go to ignored `dist/release/` and `target/release-packages/`. This builds
only the current host's native package; GitHub's matrix supplies the other hosts.

Implementation validation on macOS ARM64: packed Node addon/CLI installation,
eleven Node tests, packed WASM installation, Python wheel installation and eight
binding tests, independent source-archive rebuild and the same eight tests, Cargo
core-package verification, ten release-gate tests, and Actionlint validation of
both workflows passed. No registry uploads or remote workflow runs were performed;
the first manual workflow run must validate the remaining platform builds.
