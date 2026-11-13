#!/usr/bin/env python3
"""Independently verify run accounting, saved scores, medians and artifact hashes."""
import argparse
import hashlib
import json
import math
from pathlib import Path
from statistics import fmean, median


def read(path):
    return json.loads(path.read_text())


def check(out: Path):
    manifest = read(out / "corpus-manifest.json")
    ids = manifest["document_ids"]
    assert len(ids) == len(set(ids)) == 200
    runs = read(out / "runs.json")
    assert len(runs) == 6
    assert [run["warmup"] for run in runs] == [True] + [False] * 5
    expected_files = {doc_id + ".md" for doc_id in ids}
    environment = read(out / "environment.json")
    assert environment["protocol"]["parser_options"] == runs[0]["options"]
    assert environment["protocol"]["unicode_normalization"] == runs[0]["options"]["pdf_normalize_unicode"]
    for index, run in enumerate(runs):
        assert run["index"] == index
        assert [d["document_id"] for d in run["documents"]] == ids
        assert run["elapsed_ns"] > 0
        assert sum(d["elapsed_ns"] for d in run["documents"]) <= run["elapsed_ns"]
        assert set(run["prediction_sha256"]) == expected_files
        assert run["prediction_sha256"] == runs[0]["prediction_sha256"]
        assert run["options"] == runs[0]["options"]
        for document in run["documents"]:
            assert document["ok"] == (document["error"] is None)
    for filename, digest in runs[-1]["prediction_sha256"].items():
        path = out / "prediction/bulkparse/markdown" / filename
        assert hashlib.sha256(path.read_bytes()).hexdigest() == digest, filename
    evaluation = read(out / "prediction/bulkparse/evaluation.json")
    assert [d["document_id"] for d in evaluation["documents"]] == ids
    assert all(d["prediction_available"] for d in evaluation["documents"])
    assert evaluation["metrics"]["missing_predictions"] == 0
    for document in evaluation["documents"]:
        scores = document["scores"]
        values = [scores[k] for k in ("nid", "teds", "mhs") if scores[k] is not None]
        assert math.isclose(scores["overall"], fmean(values), abs_tol=1e-12)
    for key, actual in evaluation["metrics"]["score"].items():
        metric = key.removesuffix("_mean")
        values = [d["scores"][metric] for d in evaluation["documents"] if d["scores"][metric] is not None]
        assert all(0 <= value <= 1 for value in values)
        assert math.isclose(actual, fmean(values), abs_tol=1e-12)
        if metric in ("nid", "teds", "mhs"):
            assert evaluation["metrics"][metric + "_count"] == len(values)
    measured_median = median(r["elapsed_ns"] for r in runs[1:]) / 1e9
    assert evaluation["summary"]["total_elapsed"] == measured_median
    assert evaluation["summary"]["elapsed_per_doc"] == measured_median / 200
    comparison = read(out / "comparison.json")
    assert comparison[0]["total_elapsed"] == measured_median
    for metric, value in evaluation["metrics"]["score"].items():
        assert comparison[0][metric] == value
    baseline = read(out / "baseline-verification.json")
    assert baseline["all_document_scores_exactly_equal"]
    assert all(abs(delta) <= 1e-12 for delta in baseline["score_deltas"].values())
    for filename, digest in read(out / "source-manifest.json").items():
        assert hashlib.sha256((out / "source-snapshot" / filename).read_bytes()).hexdigest() == digest
    for filename, digest in manifest["sha256"].items():
        if filename.startswith("src/"):
            assert hashlib.sha256((out / "evaluator" / Path(filename).name).read_bytes()).hexdigest() == digest
    report = {
        "documents": 200, "attempts_including_warmup": 1200, "measured_attempts": 1000,
        "parser_errors_including_warmup": sum(not d["ok"] for r in runs for d in r["documents"]),
        "partial_documents_final_pass": sum(d["status"] == "partial" for d in runs[-1]["documents"]),
        "deterministic_predictions": True, "median_seconds": measured_median,
        "baseline_scores_reproduced_exactly": True, "artifact_hashes_verified": True,
    }
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("report", type=Path)
    args = parser.parse_args()
    print(json.dumps(check(args.report), indent=2))
