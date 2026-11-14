//! Conservative ruled-table recovery from raster lines and contained OCR boxes.
use bulkparse::{PdfOcrCell, PdfOcrLine, PdfOcrRegion, PdfOcrRegionContent};
use image::RgbImage;
use std::collections::BTreeMap;

struct Rules {
    width: usize,
    height: usize,
    horizontal: Vec<u8>,
    vertical: Vec<u8>,
}
impl Rules {
    fn detect(image: &RgbImage) -> Self {
        let (width, height) = (image.width() as usize, image.height() as usize);
        let gray: Vec<u8> = image
            .pixels()
            .map(|p| {
                ((u32::from(p[0]) * 299 + u32::from(p[1]) * 587 + u32::from(p[2]) * 114) / 1000)
                    as u8
            })
            .collect();
        let threshold = otsu(&gray);
        let mut horizontal = vec![0; gray.len()];
        let mut vertical = vec![0; gray.len()];
        for y in 0..height {
            let mut start = 0;
            for x in 0..=width {
                if x < width && gray[y * width + x] <= threshold {
                    continue;
                }
                if x - start >= 24.max(width / 40) {
                    horizontal[y * width + start..y * width + x].fill(1);
                }
                start = x + 1;
            }
        }
        for x in 0..width {
            let mut start = 0;
            for y in 0..=height {
                if y < height && gray[y * width + x] <= threshold {
                    continue;
                }
                if y - start >= 24.max(height / 40) {
                    for row in start..y {
                        vertical[row * width + x] = 1;
                    }
                }
                start = y + 1;
            }
        }
        Self {
            width,
            height,
            horizontal,
            vertical,
        }
    }

    fn boxes(&self) -> Vec<[usize; 4]> {
        let mut mask: Vec<_> = self
            .horizontal
            .iter()
            .zip(&self.vertical)
            .map(|(a, b)| a | b)
            .collect();
        let mut boxes = Vec::new();
        let mut stack = Vec::new();
        for index in 0..mask.len() {
            if mask[index] == 0 {
                continue;
            }
            let (x, y) = (index % self.width, index / self.width);
            let mut bbox = [x, y, x, y];
            mask[index] = 0;
            stack.push(index);
            while let Some(index) = stack.pop() {
                let (x, y) = (index % self.width, index / self.width);
                bbox[0] = bbox[0].min(x);
                bbox[1] = bbox[1].min(y);
                bbox[2] = bbox[2].max(x);
                bbox[3] = bbox[3].max(y);
                for yy in y.saturating_sub(1)..=(y + 1).min(self.height - 1) {
                    for xx in x.saturating_sub(1)..=(x + 1).min(self.width - 1) {
                        let next = yy * self.width + xx;
                        if mask[next] != 0 {
                            mask[next] = 0;
                            stack.push(next);
                        }
                    }
                }
            }
            if bbox[2] - bbox[0] >= 80 && bbox[3] - bbox[1] >= 60 {
                boxes.push(bbox);
            }
        }
        boxes.sort_by_key(|b| std::cmp::Reverse((b[2] - b[0]) * (b[3] - b[1])));
        boxes.truncate(128);
        boxes
    }

    fn wall(&self, vertical: bool, at: usize, start: usize, end: usize) -> f64 {
        let mask = if vertical {
            &self.vertical
        } else {
            &self.horizontal
        };
        let across = if vertical { self.width } else { self.height };
        let mut count = 0;
        for along in start + 3..end - 2 {
            if (at.saturating_sub(2)..=(at + 2).min(across - 1)).any(|cross| {
                mask[if vertical {
                    along * self.width + cross
                } else {
                    cross * self.width + along
                }] != 0
            }) {
                count += 1;
            }
        }
        count as f64 / (end - start - 5) as f64
    }

    fn grid(&self, bbox: [usize; 4], lines: &[PdfOcrLine]) -> Option<(PdfOcrRegion, Vec<usize>)> {
        let [left, top, right, bottom] = bbox;
        let xs = centers((left..=right).filter(|&x| {
            (top..=bottom)
                .filter(|&y| self.vertical[y * self.width + x] != 0)
                .count()
                * 4
                > bottom - top + 1
        }));
        let ys = centers((top..=bottom).filter(|&y| {
            (left..=right)
                .filter(|&x| self.horizontal[y * self.width + x] != 0)
                .count()
                * 4
                > right - left + 1
        }));
        if !(3..=65).contains(&xs.len()) || !(3..=129).contains(&ys.len()) {
            return None;
        }
        let (columns, rows) = (xs.len() - 1, ys.len() - 1);
        if columns * rows > 4096 || xs.windows(2).chain(ys.windows(2)).any(|v| v[1] - v[0] < 12) {
            return None;
        }
        for r in 0..rows {
            if self.wall(true, xs[0], ys[r], ys[r + 1]) < 0.85
                || self.wall(true, xs[columns], ys[r], ys[r + 1]) < 0.85
            {
                return None;
            }
        }
        for c in 0..columns {
            if self.wall(false, ys[0], xs[c], xs[c + 1]) < 0.85
                || self.wall(false, ys[rows], xs[c], xs[c + 1]) < 0.85
            {
                return None;
            }
        }
        let mut parents: Vec<_> = (0..rows * columns).collect();
        for r in 0..rows {
            for c in 0..columns {
                for (exists, other, strength) in [
                    (
                        c + 1 < columns,
                        r * columns + c + 1,
                        self.wall(true, xs[c + 1], ys[r], ys[r + 1]),
                    ),
                    (
                        r + 1 < rows,
                        (r + 1) * columns + c,
                        self.wall(false, ys[r + 1], xs[c], xs[c + 1]),
                    ),
                ] {
                    if !exists {
                        continue;
                    }
                    if strength > 0.2 && strength < 0.85 {
                        return None;
                    }
                    if strength <= 0.2 {
                        let a = root(&parents, r * columns + c);
                        let b = root(&parents, other);
                        parents[b] = a;
                    }
                }
            }
        }
        let mut groups: BTreeMap<usize, Vec<(usize, usize)>> = BTreeMap::new();
        for r in 0..rows {
            for c in 0..columns {
                groups
                    .entry(root(&parents, r * columns + c))
                    .or_default()
                    .push((r, c));
            }
        }
        let mut cells = Vec::new();
        for group in groups.values() {
            let top = group.iter().map(|p| p.0).min()?;
            let bottom = group.iter().map(|p| p.0).max()? + 1;
            let left = group.iter().map(|p| p.1).min()?;
            let right = group.iter().map(|p| p.1).max()? + 1;
            if group.len() != (bottom - top) * (right - left) {
                return None;
            }
            cells.push((top, left, bottom, right, Vec::<usize>::new()));
        }
        let mut claimed = Vec::new();
        for (index, line) in lines.iter().enumerate() {
            let [l, t, r, b] = bounds(line);
            if r <= xs[0] as f64
                || l >= xs[columns] as f64
                || b <= ys[0] as f64
                || t >= ys[rows] as f64
            {
                continue;
            }
            let mut matches = cells.iter_mut().filter(|cell| {
                l >= xs[cell.1] as f64 - 2.
                    && r <= xs[cell.3] as f64 + 2.
                    && t >= ys[cell.0] as f64 - 2.
                    && b <= ys[cell.2] as f64 + 2.
            });
            let cell = matches.next()?;
            if matches.next().is_some() {
                return None;
            }
            cell.4.push(index);
            claimed.push(index);
        }
        if claimed.len() < 2 {
            return None;
        }
        cells.sort_by_key(|c| (c.0, c.1));
        let mut output = vec![Vec::new(); rows];
        for (top, left, bottom, right, mut text) in cells {
            text.sort_by(|&a, &b| {
                let a = bounds(&lines[a]);
                let b = bounds(&lines[b]);
                a[1].total_cmp(&b[1]).then(a[0].total_cmp(&b[0]))
            });
            output[top].push(PdfOcrCell {
                text: text
                    .iter()
                    .map(|&i| lines[i].text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n"),
                row_span: (bottom - top) as u32,
                col_span: (right - left) as u32,
            });
        }
        Some((
            PdfOcrRegion {
                order: 0,
                bbox: [
                    xs[0] as f64,
                    ys[0] as f64,
                    xs[columns] as f64,
                    ys[rows] as f64,
                ],
                label: "ruled_table".into(),
                confidence: None,
                content: PdfOcrRegionContent::Table {
                    rows: output,
                    header_rows: 0,
                },
            },
            claimed,
        ))
    }
}

pub(crate) fn regions(image: &RgbImage, lines: &[PdfOcrLine]) -> Option<Vec<PdfOcrRegion>> {
    let rules = Rules::detect(image);
    let mut regions = Vec::new();
    let mut used = vec![false; lines.len()];
    for bbox in rules.boxes() {
        let Some((region, claimed)) = rules.grid(bbox, lines) else {
            continue;
        };
        if claimed.iter().any(|&i| used[i]) {
            continue;
        }
        for index in claimed {
            used[index] = true;
        }
        regions.push(region);
    }
    if regions.is_empty() {
        return None;
    }
    for (index, line) in lines.iter().enumerate() {
        if used[index] {
            continue;
        }
        let bbox = bounds(line);
        // Preserve the core's column ordering when prose runs beside a table.
        if regions
            .iter()
            .any(|r| bbox[1] < r.bbox[3] && bbox[3] > r.bbox[1])
        {
            return None;
        }
    }
    for (index, line) in lines.iter().enumerate() {
        if !used[index] {
            regions.push(PdfOcrRegion {
                order: 0,
                bbox: bounds(line),
                label: "text".into(),
                confidence: Some(line.confidence),
                content: PdfOcrRegionContent::Text {
                    text: line.text.clone(),
                },
            });
        }
    }
    if regions.len() > 1024 {
        return None;
    }
    regions.sort_by(|a, b| {
        a.bbox[1]
            .total_cmp(&b.bbox[1])
            .then(a.bbox[0].total_cmp(&b.bbox[0]))
    });
    for (index, region) in regions.iter_mut().enumerate() {
        region.order = index as u32;
    }
    Some(regions)
}

fn bounds(line: &PdfOcrLine) -> [f64; 4] {
    let mut b = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for [x, y] in line.quad {
        b[0] = b[0].min(x);
        b[1] = b[1].min(y);
        b[2] = b[2].max(x);
        b[3] = b[3].max(y);
    }
    b
}
fn root(parents: &[usize], mut index: usize) -> usize {
    while parents[index] != index {
        index = parents[index];
    }
    index
}
fn centers(values: impl Iterator<Item = usize>) -> Vec<usize> {
    let mut groups = Vec::new();
    let mut start: Option<usize> = None;
    let mut last = 0;
    for value in values {
        if start.is_some() && value != last + 1 {
            groups.push((start.take().unwrap() + last).div_ceil(2));
        }
        start.get_or_insert(value);
        last = value;
    }
    if let Some(start) = start {
        groups.push((start + last).div_ceil(2));
    }
    groups
}
fn otsu(gray: &[u8]) -> u8 {
    let mut hist = [0u64; 256];
    for &v in gray {
        hist[v as usize] += 1;
    }
    let total = gray.len() as f64;
    let sum = hist
        .iter()
        .enumerate()
        .map(|(v, &n)| v as f64 * n as f64)
        .sum::<f64>();
    let (mut count, mut partial, mut best, mut threshold) = (0., 0., -1., 0);
    for (v, &n) in hist.iter().enumerate() {
        count += n as f64;
        partial += v as f64 * n as f64;
        if count == 0. || count == total {
            continue;
        }
        let distance = partial / count - (sum - partial) / (total - count);
        let variance = count * (total - count) * distance * distance;
        if variance > best {
            best = variance;
            threshold = v as u8;
        }
    }
    threshold
}
