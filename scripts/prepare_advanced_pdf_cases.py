#!/usr/bin/env python3
"""Author three image-only reference PDFs before any model run (Pillow required).

These deliberately small synthetic cases are smoke tests, not a production
accuracy benchmark. Original content is covered by the project's MIT license.
"""
import argparse
import hashlib
import json
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont


def create(output, font_path):
    output = Path(output)
    if output.exists():
        raise ValueError("refusing to overwrite reference cases")
    output.mkdir(parents=True)
    font = lambda size: ImageFont.truetype(str(font_path), size)
    cases = []
    def page(title):
        image = Image.new("RGB", (1500, 2000), "white")
        draw = ImageDraw.Draw(image)
        draw.text((120, 100), title, font=font(58), fill="black")
        return image, draw
    def save(name, image, checks):
        path = output / (name + ".pdf")
        image.save(path, "PDF", resolution=180, title=name,
                   creationDate="D:20261113000000Z", modDate="D:20261113000000Z")
        # PNGs are authoring references; PDF render verification is separate.
        image.save(output / (name + ".png"))
        cases.append({"id": name, "path": path.name,
                      "sha256": hashlib.sha256(path.read_bytes()).hexdigest(), "checks": checks})
    image, draw = page("Quarterly Revenue")
    xs, ys = [120, 480, 900, 1320], [300, 440, 580, 720, 860]
    draw.rectangle((xs[0], ys[0], xs[-1], ys[-1]), outline="black", width=4)
    draw.line((xs[1], ys[0], xs[1], ys[-1]), fill="black", width=3)
    draw.line((xs[2], ys[1], xs[2], ys[-1]), fill="black", width=3)
    draw.line((xs[1], ys[1], xs[-1], ys[1]), fill="black", width=3)
    for y in ys[2:-1]: draw.line((xs[0], y, xs[-1], y), fill="black", width=3)
    for x, y, text in [(150, 400, "Quarter"), (760, 345, "Revenue"),
                        (520, 485, "Domestic"), (970, 485, "Export"),
                        (160, 625, "Q1"), (620, 625, "1250"), (1030, 625, "730"),
                        (160, 765, "Q2"), (620, 765, "1480"), (1030, 765, "910")]:
        draw.text((x, y), text, font=font(44), fill="black")
    draw.text((120, 980), "All amounts are in USD.", font=font(36), fill="black")
    def cell(text, col_span=1, row_span=1):
        return {"text": text, "col_span": col_span, "row_span": row_span}
    save("scanned-merged-table", image, [
        {"type": "text", "contains": "All amounts are in USD."},
        {"type": "table", "header_rows": 2, "rows": [
            [cell("Quarter", row_span=2), cell("Revenue", col_span=2)],
            [cell("Domestic"), cell("Export")],
            [cell("Q1"), cell("1250"), cell("730")],
            [cell("Q2"), cell("1480"), cell("910")]]}])
    image, draw = page("Energy and Ratios")
    draw.text((120, 260), "Mass and energy are related by", font=font(38), fill="black")
    draw.text((400, 440), "E = mc", font=font(96), fill="black")
    draw.text((400 + draw.textlength("E = mc", font=font(96)), 425), "2", font=font(58), fill="black")
    draw.text((120, 700), "A ratio can be written as", font=font(38), fill="black")
    draw.text((420, 880), "a + b", font=font(76), fill="black")
    draw.line((390, 982, 675, 982), fill="black", width=4)
    draw.text((510, 1010), "c", font=font(76), fill="black")
    draw.text((740, 945), "= d", font=font(76), fill="black")
    save("scanned-formulas", image, [
        {"type": "text", "contains": "Mass and energy are related by"},
        {"type": "formula", "tex": "E=mc^2"},
        {"type": "formula", "tex": r"\frac{a+b}{c}=d"}])
    image, draw = page("Operations Review")
    left = ["Maple team planted oak trees.", "Roots received fresh water.", "Work finished before lunch."]
    right = ["Willow team repaired fences.", "Posts were checked carefully.", "Tools returned to storage."]
    for x, lines in [(120, left), (820, right)]:
        for i, line in enumerate(lines):
            draw.text((x, 340 + i * 90), line, font=font(31), fill="black")
    save("scanned-columns", image, [{"type": "order", "anchors": left + right}])
    manifest = {"version": 1, "scope": "Three authored scan smoke tests; selected checks, not independent production accuracy.",
                "font_sha256": hashlib.sha256(Path(font_path).read_bytes()).hexdigest(), "cases": cases}
    (output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    return output / "manifest.json"


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--font", type=Path, required=True)
    args = parser.parse_args()
    print(create(args.output, args.font))
