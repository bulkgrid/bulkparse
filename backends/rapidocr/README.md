# Optional RapidOCR backend

The Rust `bulkparse-ocr` crate supervises this explicitly installed Python helper.
PDFium renders only the requested PDF page; RapidOCR runs explicitly selected
detection/recognition models and a text-line orientation classifier using ONNX
Runtime on CPU. This is reuse of established engines, not a new OCR model written
in Rust. None of these packages or models is required by the default parser.

## Provision and run

Python 3.12 was used for local validation. Provisioning is explicit; conversion
never installs dependencies. The three main engine versions are pinned in
`requirements.txt`; `environment-macos-arm64.txt` records the complete environment
actually tested. Linux CI is configured separately and has not run remotely.

```sh
python3 -m venv target/ocr/venv
target/ocr/venv/bin/python -m pip install -r backends/rapidocr/requirements.txt
target/ocr/venv/bin/python backends/rapidocr/configure.py --output target/ocr/settings
cargo build --release -p bulkparse-cli --features ocr

target/release/bulkparse parse fixtures/holdout/scan-only.pdf \
  --ocr-config "$PWD/target/ocr/settings/config.json"
```

PDF conversions still return exit code **2** for partial coverage. Add `--to json`
for page provenance, coverage and diagnostics. `--strict` rejects these partial
results. Use `--ocr-max-pages 2` and `--ocr-max-output-bytes 1048576` to constrain
the document-level OCR budget. OCR options are only available in an OCR-enabled
CLI build, and the backend runs only when `--ocr-config` is supplied and an
eligible page is found. The CLI batch command does not enable OCR.

The configuration records a trusted executable, argument prefix, and absolute
model-manifest path. It defaults to 200 DPI, 8 million rendered pixels per page,
30 seconds per page and a 32 MiB PDF-input ceiling. The model manifest contains
three local filenames and SHA-256 fingerprints; each worker verifies them before
loading model sessions. Sessions are reused only within that provider. The bundled ONNX assets total 31,749,509 bytes. Models, virtual
environments and native runtime binaries are not redistributed in the source
bundle.

The helper accepts local PDF paths only. Model paths are explicit, runtime
download fallbacks are disabled, and ONNX telemetry is disabled before inference.
This is application behavior, not an OS network sandbox. Run with restricted
network/filesystem permissions when required by the host. Executable paths,
arguments, models and parser budgets are host configuration, never upload fields.

## Explicit recognition profiles

`configure.py` defaults to `--profile multi-small`, using the models bundled with
RapidOCR 3.10.0. Two opt-in profiles change only the recognition model:

| Profile | Recognition model | Intended evaluation scope |
| --- | --- | --- |
| `multi-small` | PP-OCRv6 small | Existing general-purpose default |
| `multi-medium` | PP-OCRv6 medium | Larger multilingual recognizer |
| `latin-mobile` | PP-OCRv5 Latin mobile | Latin-script documents, including Swedish |

Profiles are explicit host choices, not automatic language detection. A Latin
profile cannot be assumed to read Chinese or other scripts. Confidence scores
must not be used to select a model as if they were comparable accuracy estimates.
See [measured profile tradeoffs](../../docs/ocr-profiles.md).

The optional recognition model must already exist locally. For example, provision
the pinned Latin model explicitly, then create a separate configuration:

```sh
mkdir -p target/ocr/profile-models
curl --fail --location --max-time 180 \
  https://www.modelscope.cn/models/RapidAI/RapidOCR/resolve/v3.10.0/onnx/PP-OCRv5/rec/latin_PP-OCRv5_rec_mobile.onnx \
  --output target/ocr/profile-models/latin_PP-OCRv5_rec_mobile.onnx
target/ocr/venv/bin/python backends/rapidocr/configure.py \
  --profile latin-mobile \
  --recognition-model target/ocr/profile-models/latin_PP-OCRv5_rec_mobile.onnx \
  --output target/ocr/latin-settings
```

The [profile catalog](profiles.py) records pinned upstream URLs and SHA-256
values for all three recognizers. Use the medium entry's URL/path with
`--profile multi-medium` to provision that option. Configuration verifies all
three assets against the selected profile before creating settings and refuses
to overwrite existing files. There are no downloads in configuration or parsing.

New settings use model-manifest version 2 with a profile name. Version 1
manifests remain compatible with the original default parameters. Both versions
verify local asset fingerprints when starting a worker; version 2 additionally validates
the selected profile's pinned identities and rejects arbitrary model parameters.
The OCR response protocol and document schema remain unchanged.

## Boundaries

- Newly generated settings enable `reuse_process: true`. Each `ProcessOcr` owns
  one lazy worker, serializes its requests and reuses verified model sessions.
  Keep the provider alive across documents to get the service throughput benefit.
  A fresh one-page CLI invocation still pays model startup. No page/results are cached.
- Workers recycle after `max_pages_per_worker` requests (default 128), are killed
  after any failed request, and are killed/reaped when the provider is dropped.
  Queue waiting counts against the page deadline; cancelling a queued request
  does not interrupt another request already using the worker. Restart the
  provider after updating models; loaded sessions do not hot-reload files.
- Old JSON settings without `reuse_process` retain one-process-per-page behavior,
  including compatibility with custom commands. Set it to false to opt out.
  Custom persistent helpers must implement the versioned JSONL worker protocol.
- ONNX CPU memory arenas remain disabled. Generated settings request release of
  unused glibc heap pages after each response on Linux; other platforms skip it.
  This does not free live sessions or guarantee a hard RSS limit.
- Generated `multi-small` settings enable `--adaptive-detector`: first detect at a maximum
  side of 960 pixels; retry with the original detector settings if detection is
  empty or any detected box has a short side below 15 pixels at detection scale.
  The rule applies to sparse, dense and rotated text. Recognition runs only once
  using the final boxes and the same crop resolution as before. One detector
  session is reused for both resolutions. Remove this flag for legacy detection.
  A region missed completely can still escape this heuristic. The other two
  recognition profiles retain original detection until separately evaluated.
- `--detector-max-side 960` is a fixed-resolution experiment, not the default;
  it cannot be combined with `--adaptive-detector`. It missed sparse fine print
  in the new stress test. The adaptive fallback fixes that regression.
- See measured speed, memory and quality (historical artifact; not distributed).
  Small authored test sets do not establish production accuracy, and persistent
  workers can use more memory on dense pages even when ordinary pages use less.
- Rust checks cancellation and the page deadline while supervising the process,
  caps response reads, kills/reaps the child on failure, and removes its temporary
  PDF. A page timeout becomes `pdf_ocr_error`; already-extracted native text is
  preserved. Pixel/output/input budget failures propagate as resource-limit errors.
- The Python helper rejects oversized render dimensions before bitmap allocation
  and verifies the actual bitmap size afterward. PDF decoding and inference still
  need OS memory/CPU limits; pixel bounds are not a hard memory sandbox.
- The trusted helper must not fork descendants that inherit stdout. The bundled
  backend uses native library threads, not child processes. Do not attach it to
  an arbitrary wrapper that forks unrelated processes.
- Protocol version 2 returns line text, quadrilaterals, confidence and rendered
  image dimensions. Rust validates the evidence and applies bounded whitespace
  cuts for horizontal left-to-right reading order. Schema 0.2 retains evidence
  on each line block as `source.ocr`. Ambiguous/skewed regions keep engine order.
  No table reconstruction, paragraph merging, confidence-based correctness
  guarantee, spelling repair or custom vocabulary is added. Version 1 text-only
  responses remain accepted. Structured output is limited to 4096 lines/page.
- The inspector routes image/vector pages, broken text layers and sparse mixed
  pages to OCR. Mixed pages use whole-page recognition, not region fusion.
- Standalone PNG/JPEG input, binding-level OCR APIs and OCR integration into
  `bulkparse-pool` are not part of this milestone. The core/bindings remain portable.

See [layout evaluation and known quality gaps](../../docs/ocr-layout.md) and the
[Rust provider API](../../docs/pdf-ocr.md).

Upstream implementation references: [RapidOCR](https://github.com/RapidAI/RapidOCR),
[usage](https://rapidai.github.io/RapidOCRDocs/main/en/install_usage/rapidocr/usage/),
[PDFium wrapper](https://pypdfium2.readthedocs.io/en/stable/), and
[ONNX Runtime](https://onnxruntime.ai/). These dependencies and their model assets
retain their upstream licenses; BulkParse's MIT license does not replace them.
