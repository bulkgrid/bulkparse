import argparse
import tempfile
import unittest
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFont
from rapidocr_backend import BudgetError, image_pixels
from tables import regions_from_grid


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


class Images(unittest.TestCase):
    def test_formats_transparency_and_all_frames(self):
        with tempfile.TemporaryDirectory() as directory:
            for ext in ['png', 'jpg', 'gif', 'webp', 'tiff']:
                path = Path(directory) / ('input.' + ext)
                Image.new('RGB', (100, 80), 'white').save(path)
                args = argparse.Namespace(input=path, page=1, max_pixels=8000)
                pixels, count = image_pixels(args, np)
                self.assertEqual((count, pixels.shape), (1, (80, 100, 3)))
            path = Path(directory) / 'frames.tiff'
            Image.new('RGB', (100, 80), 'white').save(path, save_all=True, append_images=[Image.new('RGB', (120, 70), 'black')])
            args = argparse.Namespace(input=path, page=2, max_pixels=8400)
            pixels, count = image_pixels(args, np)
            self.assertEqual((count, pixels.shape, int(pixels.max())), (2, (70, 120, 3), 0))
            args.max_pixels = 8300
            with self.assertRaises(BudgetError): image_pixels(args, np)
            path = Path(directory) / 'transparent.png'
            Image.new('RGBA', (100, 80), (0, 0, 0, 0)).save(path)
            pixels, _ = image_pixels(argparse.Namespace(input=path, page=1, max_pixels=8000), np)
            self.assertEqual(int(pixels.min()), 255)

    def test_merged_table_retains_every_value(self):
        image, lines = sample_table()
        regions = regions_from_grid(np.asarray(image)[:, :, ::-1].copy(), lines)
        tables = [r['content'] for r in regions if r['content']['type'] == 'table']
        self.assertEqual(len(tables), 1)
        self.assertEqual(tables[0]['rows'][0], [{'text': 'Sales report', 'row_span': 1, 'col_span': 2}])
        self.assertEqual([c['text'] for row in tables[0]['rows'] for c in row], [l['text'] for l in lines])

    def test_unruled_text_and_boxed_caption_are_not_tables(self):
        image, lines = sample_table()
        draw = ImageDraw.Draw(image)
        draw.rectangle((95, 95, 905, 405), fill='white')
        self.assertIsNone(regions_from_grid(np.asarray(image).copy(), lines))
        draw.rectangle((100, 100, 900, 400), outline='black', width=3)
        self.assertIsNone(regions_from_grid(np.asarray(image).copy(), lines))

    def test_uncertain_grid_retains_line_output(self):
        image, lines = sample_table()
        # A line crossing a cell boundary must not be assigned arbitrarily.
        lines[1]['quad'][1][0] = lines[1]['quad'][2][0] = 550
        self.assertIsNone(regions_from_grid(np.asarray(image).copy(), lines))


if __name__ == '__main__': unittest.main()
