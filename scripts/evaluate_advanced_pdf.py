#!/usr/bin/env python3
"""Evaluate OCR against frozen, human-authored references; no Firecrawl required.

The driver is the production Rust parser with a persistent ProcessOcr provider.
Checks measure selected semantics, not whole-document accuracy. A failed input
stays in the denominator. Model responses are not reference answers.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess


def sha256(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def normalize(text):
    return " ".join(text.split())


def tex_normalize(text):
    # Only whitespace and redundant braces around one character; not a symbolic
    # equivalence judge. Human review is still required for equivalent TeX.
    return re.sub(r"\{([A-Za-z0-9])\}", r"\1", re.sub(r"\s+", "", text))


def inline_text(inline):
    return inline.get("text", inline.get("tex", inline.get("alt", ""))) + "".join(inline_text(c) for c in inline.get("children", []))


def block_text(block):
    if "inlines" in block:
        return "".join(inline_text(i) for i in block["inlines"])
    if "rows" in block:
        return "\n".join("\t".join(" ".join(block_text(b) for b in c["blocks"]) for c in row) for row in block["rows"])
    if "items" in block:
        return "\n".join(block_text(b) for item in block["items"] for b in item)
    return block.get("text", "") + "\n".join(block_text(b) for b in block.get("blocks", []))


def walk(blocks):
    for block in blocks:
        yield block
        yield from walk(block.get("blocks", []))
        for item in block.get("items", []):
            yield from walk(item)
        for row in block.get("rows", []):
            for cell in row:
                yield from walk(cell["blocks"])


def check_result(result, check):
    blocks = result["document"]["blocks"]
    if "page" in check:
        blocks = [b for b in blocks if b["source"].get("page") == check["page"]]
    text = normalize("\n".join(block_text(b) for b in blocks))
    kind = check["type"]
    if kind == "text":
        return normalize(check["contains"]) in text
    if kind == "order":
        position = 0
        for anchor in check["anchors"]:
            found = text.find(normalize(anchor), position)
            if found < 0:
                return False
            position = found + len(normalize(anchor))
        return True
    if kind == "formula":
        return any(tex_normalize(i["tex"]) == tex_normalize(check["tex"])
                   for b in walk(blocks) for i in b.get("inlines", []) if i["type"] == "math")
    if kind == "table":
        for block in walk(blocks):
            if block["type"] != "table":
                continue
            rows = [[{"text": normalize(" ".join(block_text(b) for b in cell["blocks"])),
                      "col_span": cell["col_span"], "row_span": cell["row_span"]} for cell in row] for row in block["rows"]]
            if rows == check["rows"] and block["header_rows"] == check["header_rows"]:
                return True
        return False
    raise ValueError(f"unknown reference check type: {kind}")


def evaluate(manifest, config, driver, output, timeout=180):
    manifest, config, driver, output = map(lambda p: Path(p).resolve(), (manifest, config, driver, output))
    spec = json.loads(manifest.read_text())
    if spec.get("version") != 1 or not spec.get("cases"):
        raise ValueError("expected a version 1 reference manifest with cases")
    requests, seen = [], set()
    for case in spec["cases"]:
        if not re.fullmatch(r"[A-Za-z0-9_-]+", case["id"]) or case["id"] in seen or not case.get("checks"):
            raise ValueError("case IDs must be unique and have reference checks")
        seen.add(case["id"])
        path = (manifest.parent / case["path"]).resolve()
        if sha256(path) != case["sha256"]:
            raise ValueError(f"input hash mismatch: {case['id']}")
        for check in case["checks"]:
            if check.get("type") not in {"text", "order", "formula", "table"}:
                raise ValueError("unsupported reference check")
        requests.append({"id": case["id"], "path": str(path)})
    if output.exists():
        raise ValueError("refusing to overwrite evaluation output")
    output.mkdir(parents=True)
    payload = "".join(json.dumps(request) + "\n" for request in requests)
    try:
        run = subprocess.run([str(driver), str(config)], input=payload, text=True,
                             capture_output=True, timeout=timeout * len(requests))
        stdout, stderr, exit_code = run.stdout, run.stderr, run.returncode
    except subprocess.TimeoutExpired as error:
        stdout, stderr, exit_code = error.stdout or b"", error.stderr or b"", "timeout"
        if isinstance(stdout, bytes): stdout = stdout.decode(errors="replace")
        if isinstance(stderr, bytes): stderr = stderr.decode(errors="replace")
    (output / "responses.jsonl").write_text(stdout)
    (output / "driver.stderr").write_text(stderr)
    responses = {}
    for line in stdout.splitlines():
        try: response = json.loads(line)
        except json.JSONDecodeError: continue
        if response.get("id") in seen:
            responses[response["id"]] = response
    rows = []
    for case in spec["cases"]:
        response = responses.get(case["id"], {"error": "driver produced no result"})
        result = response.get("result")
        checks = [{"type": check["type"], "passed": bool(result is not None and check_result(result, check))} for check in case["checks"]]
        rows.append({"id": case["id"], "checks": checks, "error": response.get("error"),
                     "status": result.get("status") if result else None,
                     "diagnostics": sorted({d["code"] for d in result["diagnostics"]}) if result else [],
                     "wall_seconds": response.get("wall_seconds")})
    report = {"version": 1, "manifest_sha256": sha256(manifest), "config_sha256": sha256(config),
              "driver_sha256": sha256(driver), "driver_exit": exit_code, "cases": rows,
              "checks_passed": sum(c["passed"] for r in rows for c in r["checks"]),
              "checks_total": sum(len(r["checks"]) for r in rows),
              "note": "Selected reference checks; not whole-document accuracy or a Fire-PDF comparison. Timing is one pass, includes lazy backend startup, excludes driver startup. Failed inputs remain in the denominator."}
    (output / "summary.json").write_text(json.dumps(report, indent=2) + "\n")
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--driver", type=Path, default=Path("target/debug/examples/ocr_bench"))
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    report = evaluate(args.manifest, args.config, args.driver, args.output)
    print(json.dumps({k: report[k] for k in ("checks_passed", "checks_total", "driver_exit")}, indent=2))
    raise SystemExit(0 if report["checks_passed"] == report["checks_total"] and report["driver_exit"] == 0 else 2)
