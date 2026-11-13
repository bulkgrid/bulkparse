#!/usr/bin/env python3
"""Restore the five public W3C inputs from a frozen, hash-verified manifest."""
import argparse
import hashlib
import json
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest", type=Path, required=True)
    rows = json.loads(parser.parse_args().manifest.read_text())
    destination = ROOT / "target/broad/fresh-w3c"
    for row in rows:
        path = (ROOT / row["path"]).resolve()
        if not path.is_relative_to(destination) or not row["url"].startswith("https://www.w3.org/"):
            raise ValueError("Expected a W3C input inside target/broad/fresh-w3c")
        if path.exists():
            data = path.read_bytes()
        else:
            with urllib.request.urlopen(row["url"], timeout=60) as response:
                data = response.read()
        if hashlib.sha256(data).hexdigest() != row["sha256"]:
            raise ValueError(f"Input changed: {row['url']}")
        path.parent.mkdir(parents=True, exist_ok=True)
        if not path.exists():
            path.write_bytes(data)
    (destination / "manifest.json").write_text(json.dumps(rows, indent=2) + "\n")
    print(f"Verified {len(rows)} inputs")


if __name__ == "__main__":
    main()
