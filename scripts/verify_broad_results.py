#!/usr/bin/env python3
"""Audit completed evidence independently of the benchmark's summary writer."""
import argparse
import hashlib
import json
import math
import statistics
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def read(path):
    return json.loads(path.read_text())


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("report", type=Path)
    report = parser.parse_args().report
    counts = {}
    source_maps = {}
    for mode, expected_engines, expected_inputs in [("pdf", 9, 200), ("office", 6, 92)]:
        folder = report / mode
        summary = read(folder / "summary.json")
        manifest = read(folder / "manifest.json")
        assert len(summary) == expected_engines
        assert len(manifest) == expected_inputs
        assert all(digest(ROOT / r["path"]) == r["sha256"] for r in manifest)
        source_maps[mode] = read(folder / "source-manifest.json")
        for name, expected in source_maps[mode].items():
            assert digest(folder / "source-snapshot" / name) == expected, name
            if name.startswith(("crates/", "bindings/")):
                assert digest(ROOT / name) == expected, name
        for engine, result in summary.items():
            rounds = [read(folder / f"{engine}-round-{i}.json") for i in range(6)]
            assert [r["index"] for r in rounds] == list(range(6))
            measured = []
            for i, run in enumerate(rounds):
                assert len(run["documents"]) == result["attempted"]
                predictions = folder / "prediction" / engine / f"round-{i}" / "markdown"
                for name, expected in run["prediction_sha256"].items():
                    assert digest(predictions / name) == expected, (engine, i, name)
                assert len(run["prediction_sha256"]) == result["attempted"]
                if mode == "office":
                    elapsed = sum(row["elapsed_ns"] for row in run["documents"])
                    assert all(row["ok"] or row["markdown"] == "" for row in run["documents"])
                else:
                    elapsed = run["elapsed_ns"]
                    for row in run["documents"]:
                        if not row["ok"]:
                            assert (predictions / (row["document_id"] + ".md")).read_text() == ""
                assert elapsed == run["corpus_elapsed_ns"]
                if i:
                    measured.append(elapsed / 1e9)
            assert measured == result["measured_seconds"]
            assert statistics.median(measured) == result["median_seconds"]
            assert sum(row["ok"] for row in rounds[-1]["documents"]) == result["returned"]
            assert result["resources"]["exit"] == 0
            identical = all(r["prediction_sha256"] == rounds[1]["prediction_sha256"] for r in rounds[1:])
            assert identical == result["deterministic"]
            if mode == "pdf":
                indexes = [5] if identical else list(range(1, 6))
                assert result["quality_rounds"] == indexes
                evaluations = [read(folder / "prediction" / engine / f"round-{i}" / "evaluation.json") for i in indexes]
                assert all(len(e["documents"]) == 200 for e in evaluations)
                for key, value in result["metrics"].items():
                    mean = statistics.mean(e["metrics"]["score"][key] for e in evaluations)
                    assert math.isclose(mean, value, abs_tol=1e-12)
        counts[mode] = {"engines_or_profiles": len(summary), "inputs": len(manifest), "passes_per_engine": 6}
    production = [n for n in source_maps["pdf"] if n.startswith(("crates/", "bindings/"))]
    assert all(source_maps["pdf"][n] == source_maps["office"][n] for n in production)
    prior = read(report / "office/prior-output-comparison.json")
    assert prior["previous_documents"] == 81 and not prior["changed"]
    prior_pdf = ROOT / "reports/same-machine-ocr-table-alignment-2026-11-11/prediction/bulkparse/round-5/markdown"
    current_pdf = report / "pdf/prediction/bulkparse/round-5/markdown"
    assert len(list(prior_pdf.glob("*.md"))) == 200
    assert all(digest(p) == digest(current_pdf / p.name) for p in prior_pdf.glob("*.md"))
    holdout = read(report / "fresh-w3c/results.json")
    for row in holdout:
        path = report / "fresh-w3c" / row["output_path"]
        assert digest(path) == row["output_sha256"]
        assert row["passed"] == sum(row["checks"].values())
    ocr = read(report / "ocr/summary.json")
    assert {r["profile"] for r in ocr} == {"rapid-small", "lite-tiny", "paddle-tiny", "tesseract-fast"}
    for result in ocr:
        assert not result.get("failed")
        records = [json.loads(line) for line in (report / "ocr" / result["profile"] / "records.jsonl").read_text().splitlines()]
        assert len(records) == 24 * 6
        assert all(len([r for r in records if r["run"] == i]) == 24 for i in range(6))
        wall = [sum(r["wall_seconds"] for r in records if r["run"] == i) for i in range(1, 6)]
        cpu = [sum(r["cpu_seconds"] for r in records if r["run"] == i) for i in range(1, 6)]
        assert statistics.median(wall) == result["median_wall_seconds"]
        assert statistics.median(cpu) == result["median_cpu_seconds"]
        assert not result["changed_text_across_runs"], "Score all measured OCR outputs if nondeterministic"
        first = [r for r in records if r["run"] == 1]
        for unit in ["character", "word"]:
            errors = sum(r["quality"][unit + "_errors"] for r in first)
            total = sum(r["quality"]["reference_" + unit + "s"] for r in first)
            assert errors / total == result["quality"][unit + "_error_rate"]
    audit = dict(passed=True, suites=counts, ocr_profiles=len(ocr), fresh_spot_check_outcomes=len(holdout), production_source_files_unchanged=len(production), prior_office_outputs_unchanged=81, prior_pdf_outputs_unchanged=200)
    (report / "verification.json").write_text(json.dumps(audit, indent=2) + "\n")
    print(json.dumps(audit, indent=2))


if __name__ == "__main__":
    main()
