"""Thin Python interface to the same Rust engine used by Bulkgrid."""
from __future__ import annotations
import json
from pathlib import Path
from typing import Any
from . import _native


class BulkParseError(Exception):
    def __init__(self, code: str, message: str):
        super().__init__(message)
        self.code = code


def parse(data: bytes, *, filename: str | None = None, **options: Any) -> dict[str, Any]:
    """Convert bytes into the shared JSON model and Markdown. No code is executed."""
    return _run(data, filename, options, False)


def inspect_pdf(data: bytes, **options: Any) -> dict[str, Any]:
    """Inspect every PDF page without running OCR or reconstructing layout."""
    return _run(data, None, options, True)


def _run(data, filename, options, inspect):
    if not isinstance(data, (bytes, bytearray)):
        raise TypeError("data must be bytes or bytearray")
    cap = options.get("limits", {}).get("max_input_bytes", 33554432)
    if not isinstance(cap, int) or isinstance(cap, bool) or cap < 0:
        raise ValueError("max_input_bytes must be a nonnegative integer")
    if len(data) > cap:
        raise BulkParseError("resource_limit", "resource limit exceeded: max_input_bytes")
    try:
        return json.loads(_native.inspect_pdf_json(bytes(data), json.dumps(options)) if inspect else _native.parse_json(bytes(data), filename, json.dumps(options)))
    except ValueError as error:
        try:
            detail = json.loads(str(error))
        except (ValueError, TypeError):
            raise error
        raise BulkParseError(detail["code"], detail["message"]) from error


def parse_file(path: str | Path, **options: Any) -> dict[str, Any]:
    path = Path(path)
    cap = options.get("limits", {}).get("max_input_bytes", 33554432)
    if not isinstance(cap, int) or isinstance(cap, bool) or cap < 0:
        raise ValueError("max_input_bytes must be a nonnegative integer")
    with path.open("rb") as file:
        data = file.read(cap + 1)
    filename = options.pop("filename", path.name)
    return parse(data, filename=filename, **options)


def capabilities() -> list[dict[str, Any]]:
    return json.loads(_native.capabilities_json())


def parse_file_to_markdown(input: str | Path, output: str | Path, **options: Any) -> dict[str, Any]:
    """Write Markdown without overwriting; export_images=True saves Office/EPUB assets without changing the Markdown."""
    if "filename" in options:
        raise ValueError("parse_file_to_markdown uses the input path")
    try:
        return json.loads(_native.parse_file_to_markdown_json(str(input), str(output), json.dumps(options)))
    except ValueError as error:
        try:
            detail = json.loads(str(error))
        except (ValueError, TypeError):
            raise error
        raise BulkParseError(detail["code"], detail["message"]) from error


__all__ = ["parse_file_to_markdown", "inspect_pdf", "parse", "parse_file", "capabilities", "BulkParseError"]
