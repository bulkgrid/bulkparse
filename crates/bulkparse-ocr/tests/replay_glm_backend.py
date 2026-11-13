"""TEST ONLY: substitute an authored service response, retaining actual rendering.

This does not call a model or validate HTTP. The production adapter has no replay
mode. All input rendering, response normalization and framing run unchanged.
"""
import argparse
import json
from pathlib import Path
import sys

parser = argparse.ArgumentParser(add_help=False)
parser.add_argument("--replay-fixture", type=Path, required=True)
args, remaining = parser.parse_known_args()
sys.path.insert(0, str(Path(__file__).resolve().parents[3] / "backends/glmocr"))
import glmocr_backend as backend


def replay(png, size, settings, limit):
    assert png.startswith(b"\x89PNG\r\n\x1a\n"), "must render real PDF pixels"
    with args.replay_fixture.with_suffix(".calls").open("a") as stream:
        stream.write(json.dumps({"image_size": size, "png_bytes": len(png)}) + "\n")
    return backend.normalize_result(json.loads(args.replay_fixture.read_text()), size, limit)


backend.recognize_image = replay
sys.argv = [sys.argv[0], *remaining]
backend.main()
