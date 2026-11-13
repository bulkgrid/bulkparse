#!/usr/bin/env python3
"""Benchmark the unmodified BulkParse PDF core with the cited upstream evaluator.

Run with the evaluation-only requirements in benchmarks/opendataloader.
Network is used only to provision the pinned upstream checkout and dependencies,
never by this runner, the parser, or the evaluator. A fresh output path is required.
"""
from __future__ import annotations

import argparse
import csv
import hashlib
import importlib.metadata
import json
import os
from pathlib import Path
import platform
import shutil
import statistics
import subprocess
import sys
from datetime import datetime, timezone

ROOT = Path(__file__).resolve().parents[1]
REVISION = "340f25d70f5b2dbc4bc1cc6b154769f74b1fc745"
BASELINES = ["pdf-inspector", "liteparse", "opendataloader", "pymupdf4llm", "markitdown"]
EVALUATOR_FILES = [
    "evaluator.py", "evaluator_heading_level.py", "evaluator_reading_order.py",
    "evaluator_table.py", "converter_markdown_table.py",
]


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write_json(path: Path, value) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")


def inventory(root: Path, paths) -> dict:
    return {str(path.relative_to(root)): sha256(path) for path in sorted(paths)}


def validate_corpus(upstream: Path) -> list[Path]:
    pdfs = sorted((upstream / "pdfs").glob("*.pdf"))
    references = sorted((upstream / "ground-truth/markdown").glob("*.md"))
    if len(pdfs) != 200 or {p.stem for p in pdfs} != {p.stem for p in references}:
        raise ValueError("Expected exactly 200 PDFs with matching reference Markdown IDs")
    for path in pdfs:
        with path.open("rb") as stream:
            if stream.read(5) != b"%PDF-":
                raise ValueError(f"Not a downloaded PDF (possibly an LFS pointer): {path}")
    return pdfs


def snapshot_sources(out: Path) -> dict:
    # Include workspace manifests for Cargo's inherited package configuration.
    paths = {ROOT / "Cargo.toml", ROOT / "Cargo.lock", ROOT / "LICENSE"}
    for name in ("THIRD_PARTY_NOTICES.md", "docs/pdf-layout-heading-fixes.md"):
        if (ROOT / name).exists():
            paths.add(ROOT / name)
    for directory in (ROOT / "crates", ROOT / "bindings"):
        paths.update(directory.glob("*/Cargo.toml"))
        paths.update(directory.glob("*/build.rs"))
        paths.update(directory.glob("*/src/**/*.rs"))
        paths.update(directory.glob("*/tests/**/*.rs"))
    paths.update((ROOT / "benchmarks/opendataloader").rglob("*.rs"))
    paths.update((ROOT / "benchmarks/opendataloader").glob("Cargo.*"))
    paths.add(ROOT / "benchmarks/opendataloader/requirements.txt")
    paths.add(Path(__file__).resolve())
    paths.add(ROOT / "scripts/check_opendataloader_results.py")
    paths.add(ROOT / "scripts/test_benchmark_opendataloader.py")
    # These small authored fixtures are included by the core's Rust tests.
    for name in ("native-and-empty.pdf", "holdout/scan-only.pdf", "holdout/native-and-scan.pdf"):
        paths.add(ROOT / "fixtures" / name)
    paths.update((ROOT / "crates/bulkparse/tests/fixtures/images").glob("*"))
    hashes = inventory(ROOT, paths)
    for path in paths:
        destination = out / "source-snapshot" / path.relative_to(ROOT)
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(path, destination)
    if inventory(ROOT, paths) != hashes or any(
        sha256(out / "source-snapshot" / name) != digest for name, digest in hashes.items()
    ):
        raise ValueError("Source changed while taking the snapshot; retry with a fresh output path")
    return hashes


def evaluate(upstream: Path, prediction: Path, out: Path, name: str, summary: dict) -> dict:
    # Use the original per-document evaluator and aggregator. Fail closed rather
    # than using upstream's CLI loop, which logs and skips evaluation exceptions.
    from evaluator import _evaluate_single_document, _aggregate_document_scores

    documents = []
    for i, reference in enumerate(sorted((upstream / "ground-truth/markdown").glob("*.md"))):
        if not (prediction / reference.name).is_file():
            raise ValueError(f"Missing prediction: {prediction / reference.name}")
        documents.append(_evaluate_single_document(reference.stem, reference, prediction / reference.name))
        if (i + 1) % 25 == 0:
            print(f"Scored {name}: {i + 1}/200", flush=True)
    if len(documents) != 200:
        raise ValueError("Evaluation must retain all 200 documents")
    payload = {"summary": summary, "metrics": _aggregate_document_scores(documents),
               "documents": [document.to_json() for document in documents]}
    write_json(out / "evaluation.json", payload)
    with (out / "evaluation.csv").open("w", newline="", encoding="utf-8") as stream:
        fields = ["document_id", "overall", "nid", "nid_s", "teds", "teds_s", "mhs", "mhs_s"]
        writer = csv.DictWriter(stream, fieldnames=fields)
        writer.writeheader()
        writer.writerows({"document_id": d["document_id"], **d["scores"]} for d in payload["documents"])
    return payload


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--upstream", type=Path, default=ROOT / "target/opendataloader-bench")
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--processor", required=True, help="Verified hardware label, saved as supplied")
    parser.add_argument("--memory-bytes", type=int)
    args = parser.parse_args()
    upstream, out = args.upstream.resolve(), args.out.resolve()
    revision = subprocess.check_output(["git", "-C", str(upstream), "rev-parse", "HEAD"], text=True).strip()
    if revision != REVISION:
        raise ValueError(f"Expected cited benchmark revision {REVISION}, got {revision}")
    if subprocess.check_output(["git", "-C", str(upstream), "status", "--porcelain", "--untracked-files=no"], text=True):
        raise ValueError("Upstream tracked files have changes; use the clean pinned checkout")
    pdfs = validate_corpus(upstream)
    if out.exists():
        raise ValueError(f"Refusing to overwrite an existing run: {out}")
    out.mkdir(parents=True)
    source_hashes = snapshot_sources(out)
    write_json(out / "source-manifest.json", source_hashes)
    # Keep Cargo's incremental fingerprints separate for different source roots.
    # A shared target directory can otherwise retain another snapshot's binary.
    build_dir = ROOT / "target/opendataloader-build" / out.name
    if build_dir.exists():
        raise ValueError("Build directory already exists; choose a unique report basename")
    build_env = dict(os.environ, CARGO_TARGET_DIR=str(build_dir))
    subprocess.run(["cargo", "build", "--manifest-path", str(out / "source-snapshot/benchmarks/opendataloader/Cargo.toml"),
                    "--release", "--locked", "--offline"], env=build_env, check=True)
    binary = build_dir / "release/bulkparse-opendataloader-bench"
    manifest_paths = pdfs + sorted((upstream / "ground-truth/markdown").glob("*.md"))
    manifest_paths += [upstream / "src" / name for name in EVALUATOR_FILES]
    manifest_paths += [upstream / "uv.lock", upstream / "prediction/benchmark_runs.json"]
    for engine in BASELINES:
        manifest_paths.append(upstream / "prediction" / engine / "evaluation.json")
    write_json(out / "corpus-manifest.json", {
        "repository": "https://github.com/firecrawl/opendataloader-bench",
        "branch": "abi/pdf-parser-benchmark-results", "revision": revision,
        "document_ids": [p.stem for p in pdfs], "sha256": inventory(upstream, manifest_paths),
    })
    for name in EVALUATOR_FILES:
        destination = out / "evaluator" / name
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(upstream / "src" / name, destination)
    for name in ("LICENSE", "THIRD_PARTY_LICENSES.txt", "THIRD_PARTY_NOTICES.md"):
        shutil.copy2(upstream / name, out / "evaluator" / name)
    shutil.copy2(upstream / "prediction/benchmark_runs.json", out / "baseline-timings.json")
    environment = {
        "started_utc": datetime.now(timezone.utc).isoformat(),
        "platform": platform.platform(), "processor": args.processor,
        "processor_source": "--processor supplied from host hardware query",
        "memory_bytes": args.memory_bytes, "logical_cpus": os.cpu_count(),
        "python": sys.version, "rustc": subprocess.check_output(["rustc", "--version"], text=True).strip(),
        "cargo": subprocess.check_output(["cargo", "--version"], text=True).strip(),
        "binary_sha256": sha256(binary), "bulkparse_version": "0.1.0",
        "bulkparse_revision": None, "source_identity": "source-manifest.json (workspace has no Git metadata)",
        "dependencies": {name: importlib.metadata.version(name) for name in
                         ("apted", "beautifulsoup4", "lxml", "rapidfuzz", "soupsieve", "typing-extensions")},
        "command": sys.argv,
        "protocol": {
            "warmup_full_corpus_runs": 1, "measured_full_corpus_runs": 5,
            "documents_per_run": 200, "persistent_parser_processes": 1,
            "document_parallelism": 1, "document_order": "lexicographic filename",
            "engine_rotation": "not applicable: only BulkParse rerun; competitors are historical",
            "ocr": False, "model_parsing": False,
            "parser_options": "Actual runtime options recorded in runs.json; copied here after execution",
            "timed": "input file reads, native parse, Markdown render, output writes, per-document diagnostics bookkeeping",
            "excluded": "build, process startup, request/response JSON, hashing, evaluation; warmup excluded from median",
            "cache": "warm filesystem cache; no fsync, no cache flush, no CPU pinning",
        },
    }
    write_json(out / "environment.json", environment)
    prediction_dir = out / "prediction/bulkparse/markdown"
    prediction_dir.mkdir(parents=True)
    request = json.dumps({"documents": [str(p) for p in pdfs], "output_dir": str(prediction_dir)}) + "\n"
    runs = []
    with (out / "worker.stderr.log").open("w") as stderr:
        process = subprocess.Popen([str(binary)], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                   stderr=stderr, text=True, env=dict(os.environ, RAYON_NUM_THREADS="1"))
        try:
            for index in range(6):
                process.stdin.write(request)
                process.stdin.flush()
                line = process.stdout.readline()
                if not line:
                    raise RuntimeError(f"Worker exited early: {process.poll()}; see worker.stderr.log")
                run = json.loads(line)
                if [d["document_id"] for d in run["documents"]] != [p.stem for p in pdfs]:
                    raise ValueError("Worker did not return all 200 documents in order")
                run.update(index=index, warmup=index == 0,
                           prediction_sha256=inventory(prediction_dir, prediction_dir.glob("*.md")))
                runs.append(run)
                write_json(out / "runs.json", runs)
                print(f"{'Warmup' if index == 0 else f'Run {index}/5'}: {run['elapsed_ns']/1e9:.6f}s; "
                      f"{sum(d['ok'] for d in run['documents'])}/200 parser returns", flush=True)
            process.stdin.close()
            if process.wait(timeout=30) != 0:
                raise RuntimeError("Worker exited unsuccessfully")
        finally:
            if process.poll() is None:
                process.kill()
                process.wait()
            process.stdout.close()
    if any(r["prediction_sha256"] != runs[0]["prediction_sha256"] for r in runs[1:]):
        raise ValueError("Predictions changed across runs; cannot report one representative evaluation")
    if any(sha256(out / "source-snapshot" / name) != digest for name, digest in source_hashes.items()):
        raise ValueError("Frozen source snapshot changed during the run")
    environment["protocol"]["parser_options"] = runs[0]["options"]
    environment["protocol"]["unicode_normalization"] = runs[0]["options"]["pdf_normalize_unicode"]
    write_json(out / "environment.json", environment)
    timing = statistics.median(r["elapsed_ns"] for r in runs[1:]) / 1e9
    summary = {"engine_name": "bulkparse", "engine_version": "0.1.0", "processor": args.processor,
               "document_count": 200, "total_elapsed": timing, "elapsed_per_doc": timing / 200,
               "date": datetime.now(timezone.utc).date().isoformat()}
    write_json(out / "prediction/bulkparse/summary.json", summary)
    sys.path.insert(0, str(out / "evaluator"))
    result = evaluate(upstream, prediction_dir, out / "prediction/bulkparse", "bulkparse", summary)
    baseline_rows = []
    for engine in BASELINES:
        data = json.loads((upstream / "prediction" / engine / "evaluation.json").read_text())
        write_json(out / "baseline" / engine / "evaluation.json", data)
        baseline_rows.append({"engine": engine, "kind": "historical 2026-07-31; not rerun",
                              **data["summary"], **data["metrics"]["score"]})
    reference = json.loads((upstream / "prediction/pdf-inspector/evaluation.json").read_text())
    rescored = evaluate(upstream, upstream / "prediction/pdf-inspector/markdown",
                        out / "baseline/pdf-inspector", "saved pdf-inspector baseline", reference["summary"])
    deltas = {k: rescored["metrics"]["score"][k] - v for k, v in reference["metrics"]["score"].items()}
    equal_documents = rescored["documents"] == reference["documents"]
    write_json(out / "baseline-verification.json", {"score_deltas": deltas,
               "all_document_scores_exactly_equal": equal_documents,
               "note": "Rescoring archived predictions only; historical parser and timing were not rerun."})
    if any(abs(delta) > 1e-12 for delta in deltas.values()) or not equal_documents:
        raise ValueError("Evaluator did not exactly reproduce archived baseline document scores")
    write_json(out / "comparison.json", [{"engine": "bulkparse", "kind": "measured locally",
               **summary, **result["metrics"]["score"]}] + baseline_rows)
    print(json.dumps({"median_seconds": timing, "metrics": result["metrics"]}, indent=2), flush=True)


if __name__ == "__main__":
    main()
