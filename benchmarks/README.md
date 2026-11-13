# Benchmarks and regression tools

The published comparisons cover three workloads:

| Workload | Protocol | Evidence |
| --- | --- | --- |
| 200 mixed documents, 19 formats, eight engines | [Mixed documents](realworld/README.md) | [Results](../reports/mixed-200-2026-11-13/README.md) |
| 200 annotated PDFs, OCR disabled | [PDF comparison](broad/README.md) | [Results](../reports/broad-benchmark-2026-11-12/README.md#pdf-200-documents-ocr-disabled) |
| Multilingual OCR, 24 authored cases | [OCR comparison](ocr-alternatives/README.md) | [Results](../reports/broad-benchmark-2026-11-12/README.md#ocr-engine-profiles-evaluated-separately) |

All converters are benchmark-only dependencies. Provision them explicitly; they are not required to use BulkParse. Record hardware, settings, versions and input hashes, and run engines sequentially on otherwise idle hardware.

## Regression tools

The other directories retain adapters, frozen manifests, authored fixtures and regression specifications for focused PDF, table, OCR, streaming and worker checks. The [27 difficult real-world cases](realworld-failures/README.md) remain in the current mixed corpus. These are development inputs, not unseen holdouts.

Older measurement directories and saved executable baselines are not distributed. Historical scripts may require a freshly generated baseline or an external reference checkout. Use the current protocols above for the README comparisons.

## Harness tests

```sh
python3 -m pip install -r benchmarks/requirements-test.txt
python3 scripts/run_harness_tests.py
```

Test modules run in separate processes so independently configured backends cannot shadow each other's modules. Tests requiring external corpora or historical artifacts explicitly skip when those inputs are absent. To enable ICDAR scorer tests, provision the pinned evaluator and dependencies described in [opendataloader](opendataloader/README.md).
