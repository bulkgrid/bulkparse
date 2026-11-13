"""Reproducible, original fixtures; no third-party documents or dependencies."""
from pathlib import Path
import json
import zipfile

ROOT = Path(__file__).resolve().parents[1] / "fixtures"
ROOT.mkdir(exist_ok=True)
(ROOT / "hello.md").write_text("# Hello BulkParse\n\nA **shared** document.\n\n- First\n- Second\n", encoding="utf-8")
(ROOT / "table.csv").write_text('name,value\nalpha,"line one\nline two"\nbeta,42\n', encoding="utf-8")
(ROOT / "notebook.ipynb").write_text(json.dumps({
    "nbformat": 4, "nbformat_minor": 5,
    "metadata": {"language_info": {"name": "python"}},
    "cells": [
        {"cell_type": "markdown", "metadata": {}, "source": ["# Hello BulkParse"]},
        {"cell_type": "code", "metadata": {}, "execution_count": 1,
         "source": ["print('hello')"],
         "outputs": [{"output_type": "stream", "name": "stdout", "text": ["hello\n"]}]}
    ]}, indent=2) + "\n", encoding="utf-8")

parts = {
    "[Content_Types].xml": '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/></Types>',
    "_rels/.rels": '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="r1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>',
    "word/styles.xml": '<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:pPr><w:outlineLvl w:val="0"/></w:pPr></w:style></w:styles>',
    "word/_rels/document.xml.rels": '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="s1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>',
    "word/document.xml": '<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Hello BulkParse</w:t></w:r></w:p><w:p><w:r><w:rPr><w:b/></w:rPr><w:t>Shared content</w:t></w:r></w:p></w:body></w:document>'
}
with zipfile.ZipFile(ROOT / "hello.docx", "w") as archive:
    for name, text in parts.items():
        info = zipfile.ZipInfo(name, date_time=(2026, 1, 1, 0, 0, 0))
        info.compress_type = zipfile.ZIP_DEFLATED
        archive.writestr(info, text)

content = b"BT /F1 12 Tf 72 720 Td (Hello BulkParse) Tj ET\n"
objects = [
    b"<< /Type /Catalog /Pages 2 0 R >>",
    b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>",
    b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 6 0 R >>",
    b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << >> /Contents 7 0 R >>",
    b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    b"<< /Length " + str(len(content)).encode() + b" >>\nstream\n" + content + b"endstream",
    b"<< /Length 0 >>\nstream\nendstream",
]
pdf = bytearray(b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n")
offsets = [0]
for number, obj in enumerate(objects, 1):
    offsets.append(len(pdf))
    pdf.extend(f"{number} 0 obj\n".encode() + obj + b"\nendobj\n")
xref = len(pdf)
pdf.extend(f"xref\n0 {len(offsets)}\n0000000000 65535 f \n".encode())
for offset in offsets[1:]:
    pdf.extend(f"{offset:010d} 00000 n \n".encode())
pdf.extend(f"trailer\n<< /Size {len(offsets)} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n".encode())
(ROOT / "native-and-empty.pdf").write_bytes(pdf)
