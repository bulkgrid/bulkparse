#!/usr/bin/env python3
"""Create opt-in settings for a provisioned GLM-OCR SDK layout server."""
import argparse
import json
from pathlib import Path
import sys
from glmocr_backend import endpoint, load_settings


def configure(output, url, api_key_env=None):
    output = Path(output).resolve()
    settings = output / "service.json"
    config = output / "config.json"
    if config.exists() or settings.exists():
        raise ValueError("refusing to overwrite existing GLM settings")
    data = {"version": 1, "endpoint": endpoint(url), "api_key_env": api_key_env,
            "request_timeout_seconds": 120, "max_response_bytes": 8 * 1024 * 1024}
    output.mkdir(parents=True, exist_ok=True)
    with settings.open("x") as stream:
        json.dump(data, stream, indent=2)
    try:
        load_settings(settings)
        data = {"executable": str(Path(sys.executable).absolute()),
                "args": [str(Path(__file__).resolve().with_name("glmocr_backend.py"))],
                "models": str(settings), "dpi": 200, "max_pixels": 8_000_000,
                "page_timeout_ms": 150_000, "max_input_bytes": 32 * 1024 * 1024,
                "reuse_process": True, "max_pages_per_worker": 128}
        with config.open("x") as stream:
            json.dump(data, stream, indent=2)
    except Exception:
        settings.unlink()
        raise
    return config


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--endpoint", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--api-key-env", help="name of an environment variable; never the key itself")
    args = parser.parse_args()
    try:
        print(configure(args.output, args.endpoint, args.api_key_env))
    except (ValueError, OSError) as error:
        parser.exit(1, str(error) + "\n")
