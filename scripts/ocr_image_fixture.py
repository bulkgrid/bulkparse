"""Authored image fixture; Pillow is a test-only dependency."""
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont

def sample_table():
    image = Image.new('RGB', (1000, 600), 'white')
    draw = ImageDraw.Draw(image)
    fonts = [Path('/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf'), Path('/System/Library/Fonts/Helvetica.ttc')]
    font_path = next((font for font in fonts if font.exists()), None)
    font = ImageFont.truetype(str(font_path), 26) if font_path else ImageFont.load_default(size=26)
    for y in [100, 200, 300, 400]:
        draw.line((100, y, 900, y), fill='black', width=3)
    for x in [100, 900]:
        draw.line((x, 100, x, 400), fill='black', width=3)
    draw.line((500, 200, 500, 400), fill='black', width=3)
    lines = []
    for text, x, y in [('Sales report', 130, 130), ('Product', 130, 230), ('Amount', 530, 230), ('Apples', 130, 330), ('125', 530, 330)]:
        draw.text((x, y), text, fill='black', font=font)
        l, t, r, b = draw.textbbox((x, y), text, font=font)
        lines.append({'text': text, 'quad': [[l, t], [r, t], [r, b], [l, b]], 'confidence': .99})
    return image, lines
