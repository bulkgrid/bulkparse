# Linux CPU OCR alternatives

This benchmark compares the OCR stage, including image loading, with fixed multilingual
configuration. It does not alter BulkParse's production backend or model defaults.
The frozen [protocol](protocol.json) specifies timing, memory and scoring definitions.

The first run uses Docker Linux ARM64 on an Apple M3 Max. These are not x86 server
measurements. The 24 authored PDF cases (26 pages) cover nine languages, mostly Latin
scripts, and include correlated degraded variants. They do not establish real-world
multilingual accuracy or table/layout accuracy.

## Reproduce

Run from the BulkParse repository root, using Python 3.12 and Docker. Provisioning
downloads models explicitly; the measured workers have networking disabled. Downloads
are checked against the committed manifests, including native Paddle model files.
The Dockerfiles now pin the base image and every Python package to versions recorded
in the measured environments. The results retain the original build definitions and
image IDs; package locks were exported after image creation. Debian system packages
are installed from the distribution repository, so a later rebuild need not have an
identical image ID. For x86 production measurements, build and run on the actual x86
host with its native platform rather than timing emulation on this Mac.

```sh
python3 scripts/provision_ocr_alternatives.py --paddle
docker build --platform linux/arm64 \
  -t bulkparse-ocr-alternatives:2026-11-11 \
  -f benchmarks/ocr-alternatives/Dockerfile benchmarks/ocr-alternatives
docker build --platform linux/arm64 \
  -t bulkparse-ocr-paddle:2026-11-11 \
  -f benchmarks/ocr-alternatives/Dockerfile.paddle benchmarks/ocr-alternatives
```

Use the frozen `data/cases.json` and `data/images/` from `reports/broad-benchmark-2026-11-12/ocr/` under
`target/ocr-alternatives/data/`. Alternatively, regenerate from the project fixtures
with `scripts/prepare_ocr_alternatives.py` in an environment containing PDFium 5.15.2
and Pillow. The script refuses to overwrite an existing images directory. PNG encoding
versions can change file hashes; the distributed frozen inputs avoid that difference.

```sh
python3 -m unittest discover -s scripts -p test_benchmark_ocr_alternatives.py
python3 scripts/benchmark_ocr_alternatives.py --out reports/ocr-alternatives-new-run
```

Each worker stays alive for a complete warm-up plus five measured corpus runs. Only
one profile runs at a time. Profile order is seeded and recorded; runs are not
interleaved between profiles. Containers have a one-CPU quota and a 4 GiB memory
limit. Full environment/package versions, image IDs, model/input/source hashes,
options, per-document text/geometry, CPU/wall measurements and errors are retained.
Peak memory is the worker's lifetime peak resident set, not container memory usage.
Initialization and container launch-to-ready are reported separately once, with warm
filesystem caches; neither is included in the warm-throughput figures.

Tesseract loads all nine language packs for every document. No language is selected
using the answer key. RapidOCR's medium profile retains the small detector to match
the existing BulkParse profile. RapidOCR and RapidOCRLite use the existing orientation
classifier. Native Paddle's orientation/unwarping stages are disabled and MKLDNN is
disabled for ARM portability. These are explicit deployment configurations, not
claims that every engine is running its individually optimal setting.

The raw OCR text retains each engine's order. BulkParse's subsequent layout correction
is outside this test, so these scores must not be compared directly to the earlier
end-to-end OCR profile scores. Native PDF NID/TEDS/MHS scores are a separate benchmark.

## Sources

- [RapidOCR model routes](https://rapidai.github.io/RapidOCRDocs/main/en/model_list/)
- [PaddleOCR](https://github.com/PaddlePaddle/PaddleOCR)
- [Tesseract fast and best model differences](https://tesseract-ocr.github.io/tessdoc/Data-Files.html)
- [RapidOCRLite](https://github.com/RapidAI/RapidOCRLite): the measured 5.0.0a16 package
  is a prerelease. This benchmark selects PP-OCRv6 ONNX models explicitly; it does not
  select the separate bundled `lite`/Indra model.
