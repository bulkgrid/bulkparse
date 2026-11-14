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


def _ocr_call(call):
    try:
        return json.loads(call())
    except ValueError as error:
        try:
            detail = json.loads(str(error))
        except (ValueError, TypeError):
            raise error
        raise BulkParseError(detail["code"], detail["message"]) from error


def _ocr_input(data, options):
    if not isinstance(data, (bytes, bytearray)):
        raise TypeError("data must be bytes or bytearray")
    cap = options.get("limits", {}).get("max_input_bytes", 33554432)
    if not isinstance(cap, int) or isinstance(cap, bool) or cap < 0:
        raise ValueError("max_input_bytes must be a nonnegative integer")
    if len(data) > cap:
        raise BulkParseError("resource_limit", "resource limit exceeded: max_input_bytes")
    return bytes(data)


def parse_with_ocr(data: bytes, evidence: dict[str, Any], *, filename: str | None = None,
                   budget: dict[str, int] | None = None, **options: Any) -> dict[str, Any]:
    """Validate and assemble host-provided OCR evidence without loading an OCR engine."""
    data = _ocr_input(data, options)
    return _ocr_call(lambda: _native.parse_with_ocr_json(data, json.dumps(evidence), filename, json.dumps(options), json.dumps(budget or {})))


class OcrParser:
    """Own reusable Rust OCR models. Configuration must come from the trusted host."""
    def __init__(self, config: dict[str, Any]):
        try:
            self._parser = _native.NativeOcrParser(json.dumps(config))
        except ValueError as error:
            detail = json.loads(str(error))
            raise BulkParseError(detail["code"], detail["message"]) from error

    def parse(self, data: bytes, *, filename: str | None = None,
              budget: dict[str, int] | None = None, **options: Any) -> dict[str, Any]:
        data = _ocr_input(data, options)
        return _ocr_call(lambda: self._parser.parse_json(data, filename, json.dumps(options), json.dumps(budget or {})))

    def parse_file(self, path: str | Path, *, budget: dict[str, int] | None = None, **options: Any) -> dict[str, Any]:
        path = Path(path)
        _ocr_input(b"", options)
        with path.open("rb") as stream:
            data = stream.read(options.get("limits", {}).get("max_input_bytes", 33554432) + 1)
        filename = options.pop("filename", path.name)
        return self.parse(data, filename=filename, budget=budget, **options)

    def close(self) -> None:
        self._parser.close()

    def __enter__(self):
        return self

    def __exit__(self, *_args):
        self.close()


__all__ += ["parse_with_ocr", "OcrParser"]
