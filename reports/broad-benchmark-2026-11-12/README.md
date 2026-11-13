# PDF and OCR comparisons

November 12, 2026 · Apple M3 Max · 36 GiB RAM. These measurements describe the recorded builds and settings; they are not universal performance or quality claims.

## PDF: 200 documents, OCR disabled

| Engine / profile | Overall | NID | TEDS | MHS | Seconds / 200 PDFs | Peak RSS MiB | Returned |
| --- | --- | --- | --- | --- | --- | --- | --- |
| BulkParse 0.1.0 | 0.901 | 0.919 | 0.887 | 0.860 | 0.440 | 169.0 | 200/200 |
| LiteParse 2.18.0 | 0.886 | 0.916 | 0.818 | 0.824 | 0.733 | 64.2 | 200/200 |
| pdf-inspector 1.25.2 | 0.870 | 0.911 | 0.800 | 0.777 | 0.870 | 169.4 | 200/200 |
| AnyDoc 0.2.4 | 0.867 | 0.906 | 0.814 | 0.772 | 1.217 | 174.1 | 197/200 |
| OpenDataLoader 2.5.22 | 0.843 | 0.913 | 0.483 | 0.757 | 1.588 | 724.7 | 200/200 |
| MarkItDown 0.1.9 | 0.589 | 0.844 | 0.273 | 0.000 | 15.403 | 297.8 | 200/200 |
| PyMuPDF4LLM 2.1 — native | 0.735 | 0.885 | 0.403 | 0.423 | 15.668 | 476.4 | 200/200 |
| PyMuPDF4LLM 2.1 — learned layout | 0.866 | 0.906 | 0.773 | 0.782 | 17.319 | 739.8 | 200/200 |
| Docling 2.150.0 | 0.887 | 0.902 | 0.879 | 0.836 | 64.490 | 1589.3 | 200/200 |

Native profiles use no learned PDF layout. Docling uses Heron/TableFormer with four configured CPU threads; the PyMuPDF4LLM learned-layout row uses its default layout mode with OCR disabled. Other profiles request one thread. MarkItDown uses a format-detection model, not an OCR/layout model.

All 200 inputs remain in scoring, including empty predictions for failed attempts. NID covers 200 documents, TEDS 42 and MHS 107. Overall averages available metrics per document. All BulkParse results declare partial coverage. The corpus has informed parser development; these scores are not an unseen generalization estimate.

| PDF engine | Five measured seconds | Range seconds | Outputs identical across measured passes |
| --- | --- | --- | --- |
| BulkParse 0.1.0 | 0.456, 0.427, 0.440, 0.461, 0.387 | 0.387–0.461 | True |
| AnyDoc 0.2.4 | 1.254, 1.217, 1.207, 1.217, 1.202 | 1.202–1.254 | True |
| pdf-inspector 1.25.2 | 0.884, 0.909, 0.867, 0.868, 0.870 | 0.867–0.909 | True |
| LiteParse 2.18.0 | 0.733, 0.734, 0.730, 0.733, 0.732 | 0.730–0.734 | True |
| OpenDataLoader 2.5.22 | 1.687, 1.588, 1.525, 1.433, 1.738 | 1.433–1.738 | True |
| PyMuPDF4LLM 2.1 — native | 15.775, 15.734, 15.626, 15.668, 15.628 | 15.626–15.775 | False |
| PyMuPDF4LLM 2.1 — learned layout | 17.319, 17.194, 17.362, 17.845, 17.265 | 17.194–17.845 | True |
| MarkItDown 0.1.9 | 15.984, 15.300, 15.403, 15.812, 15.244 | 15.244–15.984 | False |
| Docling 2.150.0 | 64.531, 64.210, 64.490, 64.049, 65.350 | 64.049–65.350 | True |

Memory includes initialization, warm-up and all measured passes. Startup/model loading is excluded from warm timings. See [protocol](../../benchmarks/broad/README.md) and [effective options](pdf/effective-options.json).

## OCR engine profiles, evaluated separately

| OCR profile | CER ↓ | WER ↓ | Warm seconds / 24 cases | CPU seconds | Peak RSS MiB | Errors |
| --- | --- | --- | --- | --- | --- | --- |
| tesseract-fast | 5.27% | 17.77% | 5.49 | 5.48 | 259.3 | 0 |
| lite-tiny | 3.73% | 8.29% | 30.41 | 30.37 | 345.9 | 0 |
| paddle-tiny | 4.02% | 9.72% | 33.06 | 33.02 | 1367.0 | 0 |
| rapid-small | 3.19% | 4.74% | 35.64 | 35.55 | 805.1 | 0 |

Linux ARM64 Docker containers on this same Mac, one CPU quota and 4 GiB memory limit, network disabled. One warm-up and five full sequential passes per profile; profiles run one at a time. CER/WER are micro error rates, lower is better, with case/accent/punctuation preserved after Unicode/whitespace normalization. Errors remain empty outputs.

These four deployment profiles cover RapidOCR (small), RapidOCRLite (tiny), native PaddleOCR (tiny) and Tesseract (fast language data). Model sizes differ, so the result compares configurations, not inference-framework efficiency on identical models. The existing 24 authored cases/26 pages cover nine languages and correlated degradations; they do not represent production multilingual documents. This OCR-stage test excludes PDF rendering and BulkParse layout reconstruction, and does not measure the current integrated adaptive detector.

Warm wall/CPU times exclude initialization, container startup and IPC. Memory includes initialization and all passes. Raw startup measurements and per-language quality remain in [OCR summary](ocr/summary.json). These are ARM64 guest measurements, not x86 production-server results. See [OCR protocol](../../benchmarks/ocr-alternatives/README.md).

## Evidence and reproduction

- [PDF protocol and commands](../../benchmarks/broad/README.md)
- [PDF scores and timing ranges](pdf/summary.json), [per-document timings](pdf/), [evaluator results](pdf/prediction/)
- [PDF settings](pdf/effective-options.json), [input hashes](pdf/manifest.json), [source hashes](pdf/source-manifest.json), [machine and commands](pdf/identity.json)
- [OCR protocol](../../benchmarks/ocr-alternatives/README.md), [OCR measurements](ocr/), [authored OCR inputs](ocr/data/)

Only measurements, settings, hashes and authored OCR inputs are committed. Third-party PDFs, converted text, model weights, executable binaries and historical source snapshots are excluded. The evaluator outputs contain scores, not document predictions. Rerunning the harness regenerates predictions and measures the current checkout; matching historical source hashes requires the corresponding historical builds.
