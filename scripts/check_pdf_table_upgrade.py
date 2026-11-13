#!/usr/bin/env python3
"""Compare per-page native characters and inspection, plus authored table gold.

This is a regression check on previously observed fixtures, not an accuracy
benchmark. New table structures are allowed; lost or duplicated text is not.
"""
import argparse
import hashlib
import json
from pathlib import Path

from check_pdf_quality_fix import convert, page_characters
from validate_pdf_tables import compact, tables

ROOT = Path(__file__).resolve().parents[1]


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--before", type=Path, required=True)
    parser.add_argument("--after", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--pdf-dir", type=Path, help="Also check every PDF in this directory")
    args = parser.parse_args()
    if args.out.exists():
        raise ValueError("Use a fresh output path")
    inputs = {}
    for manifest in [ROOT / "reports/pdf-tables-corpus-2026-11-10-v2/manifest.jsonl",
                     ROOT / "benchmarks/holdout/manifest.jsonl"]:
        for row in map(json.loads, manifest.read_text().splitlines()):
            path = Path(row["source_path"])
            if path.suffix.lower() != ".pdf":
                continue
            assert sha(path) == row["source_sha256"], str(path)
            inputs.setdefault(row["source_sha256"], path)
    if args.pdf_dir:
        paths = sorted(args.pdf_dir.glob("*.pdf"))
        if not paths:
            raise ValueError("The supplied PDF directory contains no PDFs")
        for path in paths:
            inputs.setdefault(sha(path), path.resolve())
    records = []
    for digest, path in inputs.items():
        old_code, old = convert(args.before, path, True)
        new_code, new = convert(args.after, path, True)
        row = {"source": str(path), "sha256": digest, "before_exit": old_code, "after_exit": new_code}
        if old_code not in (0, 2):
            row["passed"] = new_code == old_code
            row["expected_failure"] = True
        else:
            row["characters_preserved"] = new is not None and page_characters(old["document"]) == page_characters(new["document"])
            row["inspection_unchanged"] = new is not None and old["pdf_inspection"] == new["pdf_inspection"]
            row["before_tables"] = len(tables(old))
            row["after_tables"] = len(tables(new)) if new is not None else None
            row["passed"] = new_code == old_code and row["characters_preserved"] and row["inspection_unchanged"]
        records.append(row)
    known = {row["id"]: row["sha256"] for row in json.loads((ROOT / "benchmarks/pdf-tables/fresh-manifest.json").read_text())}
    authored = []
    for case in json.loads((ROOT / "benchmarks/pdf-tables/fresh-spec.json").read_text()):
        path = ROOT / "fixtures/pdf-tables" / (case["id"] + ".pdf")
        assert sha(path) == known[case["id"]]
        code, result = convert(args.after, path)
        actual = tables(result) if result is not None else None
        authored.append({"id": case["id"], "sha256": sha(path),
                         "passed": code in (0, 2) and compact(actual) == compact(case["expected_tables"]),
                         "expected": case["expected_tables"], "actual": actual})
    passed = all(row["passed"] for row in records + authored)
    report = {"passed": passed, "scope": __doc__, "before_binary_sha256": sha(args.before),
              "after_binary_sha256": sha(args.after), "harness_sha256": sha(__file__),
              "pdf_cases": records, "authored_table_cases": authored}
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({"passed": passed, "pdf_cases": len(records), "authored_table_cases": len(authored),
                      "failures": [row for row in records + authored if not row["passed"]]}, indent=2))
    raise SystemExit(0 if passed else 1)


if __name__ == "__main__":
    main()
