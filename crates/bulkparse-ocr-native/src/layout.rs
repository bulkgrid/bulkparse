//! Model-assisted layout and table grids. Text always comes from OCR evidence;
//! uncertain structures retain their source lines instead of discarding content.
use crate::{bad, Artifact};
use bulkparse::{Error, PdfOcrCell, PdfOcrLine, PdfOcrOutput, PdfOcrRegion, PdfOcrRegionContent};
use image::RgbImage;
use oar_ocr::{
    core::config::onnx::OrtSessionConfig,
    predictors::{LayoutDetectionPredictor, TableStructureRecognitionPredictor},
};
use quick_xml::{events::Event, Reader};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Config {
    pub detector: Artifact,
    pub table: Artifact,
    pub dictionary: Artifact,
}
pub(crate) struct Layout {
    detector: LayoutDetectionPredictor,
    table: TableStructureRecognitionPredictor,
}
impl Layout {
    pub fn load(config: Config, ort: OrtSessionConfig) -> Result<Self, Error> {
        config.detector.verify()?;
        config.table.verify()?;
        config.dictionary.verify()?;
        Ok(Self {
            detector: LayoutDetectionPredictor::builder()
                .model_name("pp_doclayout_plus_l")
                .score_threshold(0.5)
                .with_ort_config(ort.clone())
                .build(config.detector.path)
                .map_err(bad)?,
            table: TableStructureRecognitionPredictor::builder()
                .model_name("SLANet_plus")
                .dict_path(config.dictionary.path)
                .with_ort_config(ort)
                .build(config.table.path)
                .map_err(bad)?,
        })
    }

    pub fn regions(
        &self,
        image: &RgbImage,
        lines: &[PdfOcrLine],
        seeds: Vec<PdfOcrRegion>,
        check: &impl Fn() -> Result<(), Error>,
    ) -> Result<Option<Vec<PdfOcrRegion>>, Error> {
        check()?;
        let layout = self.detector.predict(vec![image.clone()]).map_err(bad)?;
        check()?;
        let Some(elements) = layout.elements.into_iter().next() else {
            return Ok(None);
        };
        let mut claimed: Vec<_> = lines
            .iter()
            .map(|line| {
                seeds
                    .iter()
                    .any(|region| contains_center(region.bbox, bounds(line)))
            })
            .collect();
        let mut regions = seeds;
        // Reserve tables first so overlapping caption/text detections cannot steal cells.
        let mut elements: Vec<_> = elements.into_iter().enumerate().collect();
        elements.sort_by_key(|(order, e)| (e.element_type != "table", *order));
        for (order, element) in elements {
            check()?;
            let bbox = [
                f64::from(element.bbox.x_min()).clamp(0., image.width() as f64),
                f64::from(element.bbox.y_min()).clamp(0., image.height() as f64),
                f64::from(element.bbox.x_max()).clamp(0., image.width() as f64),
                f64::from(element.bbox.y_max()).clamp(0., image.height() as f64),
            ];
            if bbox.iter().any(|v| !v.is_finite())
                || bbox[2] - bbox[0] < 1.
                || bbox[3] - bbox[1] < 1.
            {
                continue;
            }
            // A reliable ruled grid keeps its cells. Still inspect the rest of
            // the page so one detected grid cannot hide another borderless table.
            if element.element_type == "table"
                && regions.iter().any(|region| {
                    region.label == "ruled_table" && contains_center(bbox, region.bbox)
                })
            {
                continue;
            }
            let selected: Vec<_> = lines
                .iter()
                .enumerate()
                .filter(|(i, line)| !claimed[*i] && contains_center(bbox, bounds(line)))
                .map(|(i, _)| i)
                .collect();
            if selected.is_empty() {
                continue;
            }
            let content = if element.element_type == "table" {
                let l = bbox[0].floor() as u32;
                let t = bbox[1].floor() as u32;
                let r = bbox[2].ceil() as u32;
                let b = bbox[3].ceil() as u32;
                let crop = image::imageops::crop_imm(image, l, t, r - l, b - t).to_image();
                let shifted: Vec<_> = selected
                    .iter()
                    .map(|&i| {
                        let mut line = lines[i].clone();
                        for point in &mut line.quad {
                            point[0] -= l as f64;
                            point[1] -= t as f64;
                        }
                        line
                    })
                    .collect();
                let result = self.table.predict(vec![crop]).map_err(bad)?;
                check()?;
                let (Some(tokens), Some(boxes)) =
                    (result.structures.first(), result.bboxes.first())
                else {
                    continue;
                };
                let Some(content) = table_content(tokens, boxes, &shifted, Some((image, l, t)))
                else {
                    continue;
                };
                content
            } else {
                let mut selected = selected.clone();
                selected.sort_by(|&a, &b| {
                    let a = bounds(&lines[a]);
                    let b = bounds(&lines[b]);
                    a[1].total_cmp(&b[1]).then(a[0].total_cmp(&b[0]))
                });
                let text = selected
                    .iter()
                    .map(|&i| lines[i].text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n");
                match element.element_type.as_str() {
                    "doc_title" | "paragraph_title" => PdfOcrRegionContent::Heading {
                        level: if element.element_type == "doc_title" {
                            1
                        } else {
                            2
                        },
                        text,
                    },
                    _ => PdfOcrRegionContent::Text { text },
                }
            };
            // Bounds encompass the OCR evidence assigned by center, including imperfect detections.
            let mut bbox = bbox;
            for &i in &selected {
                let b = bounds(&lines[i]);
                bbox[0] = bbox[0].min(b[0]);
                bbox[1] = bbox[1].min(b[1]);
                bbox[2] = bbox[2].max(b[2]);
                bbox[3] = bbox[3].max(b[3]);
            }
            let region = PdfOcrRegion {
                order: order as u32,
                bbox,
                label: element.element_type,
                confidence: Some(element.score as f64),
                content,
            };
            if (PdfOcrOutput::Regions {
                image_size: [image.width(), image.height()],
                regions: vec![region.clone()],
            })
            .validate(usize::MAX)
            .is_err()
            {
                continue;
            }
            for &i in &selected {
                claimed[i] = true;
            }
            regions.push(region);
        }
        // No OCR line disappears when a model misses an area or predicts an invalid grid.
        for (index, line) in lines.iter().enumerate() {
            if !claimed[index] {
                regions.push(PdfOcrRegion {
                    order: u32::MAX,
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
            return Ok(None);
        }
        order_regions(&mut regions);
        Ok(Some(regions))
    }
}

fn contains_center(outer: [f64; 4], inner: [f64; 4]) -> bool {
    let x = (inner[0] + inner[2]) / 2.;
    let y = (inner[1] + inner[3]) / 2.;
    x >= outer[0] && x <= outer[2] && y >= outer[1] && y <= outer[3]
}
pub(crate) fn bounds(line: &PdfOcrLine) -> [f64; 4] {
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

// Recursive whitespace partitioning handles multi-column pages. Spanning
// headings prevent a column cut until their horizontal section is separated.
fn order_regions(regions: &mut [PdfOcrRegion]) {
    fn sort(regions: &mut [PdfOcrRegion], depth: usize) {
        if regions.len() < 2 {
            return;
        }
        if depth < 32 {
            for axis in [0, 1] {
                regions.sort_by(|a, b| a.bbox[axis].total_cmp(&b.bbox[axis]));
                let mut end = regions[0].bbox[axis + 2];
                let mut best = None;
                for (i, region) in regions.iter().enumerate().skip(1) {
                    let gap = region.bbox[axis] - end;
                    if gap > 5. && best.is_none_or(|(_, size)| gap > size) {
                        best = Some((i, gap));
                    }
                    end = end.max(region.bbox[axis + 2]);
                }
                if let Some((index, _)) = best {
                    let (a, b) = regions.split_at_mut(index);
                    sort(a, depth + 1);
                    sort(b, depth + 1);
                    return;
                }
            }
        }
        regions.sort_by(|a, b| {
            a.bbox[1]
                .total_cmp(&b.bbox[1])
                .then(a.bbox[0].total_cmp(&b.bbox[0]))
        });
    }
    sort(regions, 0);
    for (i, region) in regions.iter_mut().enumerate() {
        region.order = i as u32;
    }
}

fn table_content(
    tokens: &[String],
    boxes: &[Vec<f32>],
    lines: &[PdfOcrLine],
    raster: Option<(&RgbImage, u32, u32)>,
) -> Option<PdfOcrRegionContent> {
    let html = tokens.join("");
    if html.len() > 65536 || boxes.len() > 4096 {
        return None;
    }
    let mut reader = Reader::from_str(&html);
    let mut rows: Vec<Vec<PdfOcrCell>> = Vec::new();
    let mut positions = Vec::new();
    let mut in_row = false;
    let mut header_rows = 0;
    loop {
        match reader.read_event().ok()? {
            Event::Start(e) | Event::Empty(e) => match e.name().as_ref() {
                b"tr" => {
                    if in_row || rows.len() >= 256 {
                        return None;
                    }
                    rows.push(Vec::new());
                    in_row = true;
                }
                b"td" | b"th" => {
                    if !in_row {
                        return None;
                    }
                    let mut cell = PdfOcrCell {
                        text: String::new(),
                        col_span: 1,
                        row_span: 1,
                    };
                    for attr in e.attributes() {
                        let attr = attr.ok()?;
                        let value = std::str::from_utf8(&attr.value).ok()?.parse::<u32>().ok()?;
                        if value == 0 || value > 256 {
                            return None;
                        }
                        match attr.key.as_ref() {
                            b"colspan" => cell.col_span = value,
                            b"rowspan" => cell.row_span = value,
                            _ => return None,
                        }
                    }
                    let index = rows.len() - 1;
                    let row = rows.last_mut()?;
                    positions.push((index, row.len()));
                    row.push(cell);
                }
                b"html" | b"body" | b"table" | b"thead" | b"tbody" => {}
                _ => return None,
            },
            Event::End(e) if e.name().as_ref() == b"tr" => in_row = false,
            Event::End(e) if e.name().as_ref() == b"thead" => header_rows = rows.len(),
            Event::Eof => break,
            Event::Text(e) if !e.iter().all(u8::is_ascii_whitespace) => return None,
            _ => {}
        }
    }
    if in_row || positions.is_empty() || positions.len() != boxes.len() {
        return None;
    }
    let cell_boxes: Option<Vec<_>> = boxes
        .iter()
        .map(|v| {
            if v.iter().any(|x| !x.is_finite()) {
                return None;
            }
            match v.len() {
                4 => Some([v[0] as f64, v[1] as f64, v[2] as f64, v[3] as f64]),
                8 => Some([
                    v.iter().step_by(2).copied().fold(f32::INFINITY, f32::min) as f64,
                    v.iter()
                        .skip(1)
                        .step_by(2)
                        .copied()
                        .fold(f32::INFINITY, f32::min) as f64,
                    v.iter()
                        .step_by(2)
                        .copied()
                        .fold(f32::NEG_INFINITY, f32::max) as f64,
                    v.iter()
                        .skip(1)
                        .step_by(2)
                        .copied()
                        .fold(f32::NEG_INFINITY, f32::max) as f64,
                ]),
                _ => None,
            }
        })
        .collect();
    let cell_boxes = cell_boxes?;
    let mut assigned_rows = vec![0; lines.len()];
    let mut ordered: Vec<_> = lines.iter().enumerate().collect();
    ordered.sort_by(|a, b| {
        let a = bounds(a.1);
        let b = bounds(b.1);
        a[1].total_cmp(&b[1]).then(a[0].total_cmp(&b[0]))
    });
    for (line_index, line) in ordered {
        let b = bounds(line);
        let area = (b[2] - b[0]) * (b[3] - b[1]);
        let target = cell_boxes
            .iter()
            .enumerate()
            .filter_map(|(i, c)| {
                if c[2] <= c[0] || c[3] <= c[1] {
                    return None;
                }
                let overlap = (b[2].min(c[2]) - b[0].max(c[0])).max(0.)
                    * (b[3].min(c[3]) - b[1].max(c[1])).max(0.)
                    / area.max(1.);
                let distance =
                    ((b[0] + b[2] - c[0] - c[2]) / 2.).hypot((b[1] + b[3] - c[1] - c[3]) / 2.);
                Some((i, overlap, distance))
            })
            .max_by(|a, b| a.1.total_cmp(&b.1).then(b.2.total_cmp(&a.2)))?;
        let (r, c) = positions[target.0];
        assigned_rows[line_index] = r;
        let value = &mut rows[r][c].text;
        if !value.is_empty() {
            value.push('\n');
        }
        value.push_str(&line.text);
    }
    if let Some((image, x, y)) = raster {
        let mut heights: Vec<_> = lines.iter().map(bounds).map(|b| b[3] - b[1]).collect();
        heights.sort_by(f64::total_cmp);
        if let Some(&height) = heights.get(heights.len() / 2) {
            for (index, &(r, c)) in positions.iter().enumerate() {
                let text = &mut rows[r][c].text;
                if (text.is_empty() || text == "=")
                    && crate::marks::isolated_dash(image, cell_boxes[index], [x, y], height)
                {
                    *text = "-".into();
                }
            }
        }
    }
    repair_numeric_rows(&mut rows, lines);
    let repaired_header = crate::headers::repair(&mut rows, header_rows, lines, &assigned_rows);
    Some(PdfOcrRegionContent::Table {
        rows,
        header_rows: if repaired_header { 2 } else { 0 },
    })
}

// Model cell boxes can drift vertically on dense numeric tables. Recover body
// rows from repeated OCR baselines only when a complete row establishes every
// column and a token-conservation check proves no evidence moves out of the body.
fn repair_numeric_rows(rows: &mut Vec<Vec<PdfOcrCell>>, lines: &[PdfOcrLine]) {
    fn numeric(text: &str) -> bool {
        text.chars().any(|c| c.is_ascii_digit())
            && text
                .chars()
                .all(|c| c.is_ascii_digit() || c.is_whitespace() || ".,+-%()*−–eE".contains(c))
    }
    fn words<'a>(values: impl Iterator<Item = &'a str>) -> Vec<&'a str> {
        let mut words: Vec<_> = values.flat_map(str::split_whitespace).collect();
        words.sort_unstable();
        words
    }
    let width: usize = rows
        .first()
        .map_or(0, |row| row.iter().map(|c| c.col_span as usize).sum());
    if !(4..=64).contains(&width) {
        return;
    }
    let mut heights: Vec<_> = lines
        .iter()
        .map(bounds)
        .map(|b| b[3] - b[1])
        .filter(|h| h.is_finite() && *h > 0.)
        .collect();
    heights.sort_by(f64::total_cmp);
    let Some(&height) = heights.get(heights.len() / 2) else {
        return;
    };
    let mut ordered: Vec<_> = lines.iter().collect();
    ordered.sort_by(|a, b| {
        let a = bounds(a);
        let b = bounds(b);
        (a[1] + a[3]).total_cmp(&(b[1] + b[3]))
    });
    let mut groups: Vec<Vec<&PdfOcrLine>> = Vec::new();
    for line in ordered {
        let b = bounds(line);
        let center = (b[1] + b[3]) / 2.;
        if let Some(group) = groups.last_mut() {
            let first = bounds(group[0]);
            if (center - (first[1] + first[3]) / 2.).abs() < height * 0.55 {
                group.push(line);
                continue;
            }
        }
        groups.push(vec![line]);
    }
    let dense: Vec<_> = groups
        .iter()
        .enumerate()
        .filter(|(_, g)| {
            g.len() * 10 >= width * 7
                && g.len() <= width
                && g.iter().filter(|l| numeric(&l.text)).count() * 5 >= g.len() * 4
        })
        .map(|(i, _)| i)
        .collect();
    if dense.len() < 3 {
        return;
    }
    let Some(seed) = dense.iter().map(|&i| &groups[i]).find(|g| g.len() == width) else {
        return;
    };
    let mut anchors: Vec<_> = seed
        .iter()
        .map(|l| {
            let b = bounds(l);
            (b[0] + b[2]) / 2.
        })
        .collect();
    anchors.sort_by(f64::total_cmp);
    if anchors.windows(2).any(|p| p[1] - p[0] < height) {
        return;
    }
    let data_rows: Vec<_> = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| {
            let cells: Vec<_> = r.iter().filter(|c| !c.text.trim().is_empty()).collect();
            cells.len() >= 3 && cells.iter().filter(|c| numeric(&c.text)).count() * 2 >= cells.len()
        })
        .map(|(i, _)| i)
        .collect();
    let (Some(&start), Some(&end)) = (data_rows.first(), data_rows.last()) else {
        return;
    };
    if rows[..=end].iter().flatten().any(|c| c.row_span != 1) {
        return;
    }
    // A control/baseline row can legitimately omit percentage columns. Include
    // adjacent sparse numeric rows once dense rows establish the column grid.
    let mut first = dense[0];
    let mut last = *dense.last().unwrap();
    let sparse_numeric = |group: &Vec<&PdfOcrLine>| {
        group.len() >= 3
            && group.iter().filter(|line| numeric(&line.text)).count() * 5 >= group.len() * 4
    };
    while first > 0 && sparse_numeric(&groups[first - 1]) {
        first -= 1;
    }
    while last + 1 < groups.len() && sparse_numeric(&groups[last + 1]) {
        last += 1;
    }
    let selected = &groups[first..=last];
    let original = words(rows[start..=end].iter().flatten().map(|c| c.text.as_str()));
    let evidence = words(selected.iter().flatten().map(|l| l.text.as_str()));
    if original != evidence {
        return;
    }
    let mut replacement = Vec::new();
    for group in selected {
        let mut row = vec![
            PdfOcrCell {
                text: String::new(),
                col_span: 1,
                row_span: 1
            };
            width
        ];
        for line in group {
            let b = bounds(line);
            let center = (b[0] + b[2]) / 2.;
            let (column, _) = anchors
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| (*a - center).abs().total_cmp(&(*b - center).abs()))
                .unwrap();
            if !row[column].text.is_empty() {
                row[column].text.push(' ');
            }
            row[column].text.push_str(&line.text);
        }
        replacement.push(row);
    }
    rows.splice(start..=end, replacement);
}

#[cfg(test)]
mod tests {
    use super::*;
    fn line(text: &str, bbox: [f64; 4]) -> PdfOcrLine {
        PdfOcrLine {
            text: text.into(),
            confidence: 0.99,
            quad: [
                [bbox[0], bbox[1]],
                [bbox[2], bbox[1]],
                [bbox[2], bbox[3]],
                [bbox[0], bbox[3]],
            ],
        }
    }
    #[test]
    fn model_grid_preserves_spans_and_all_evidence() {
        let tokens = vec![
            "<table><tr><td colspan=\"2\"></td></tr><tr><td></td><td></td></tr></table>".into(),
        ];
        let boxes = vec![
            vec![0., 0., 200., 50.],
            vec![0., 50., 100., 100.],
            vec![100., 50., 200., 100.],
        ];
        let lines = vec![
            line("Header", [20., 10., 180., 40.]),
            line("Left", [10., 60., 70., 90.]),
            line("Right", [120., 60., 190., 90.]),
        ];
        let PdfOcrRegionContent::Table { rows, .. } =
            table_content(&tokens, &boxes, &lines, None).unwrap()
        else {
            panic!("table expected")
        };
        assert_eq!(rows[0][0].col_span, 2);
        assert_eq!(rows[0][0].text, "Header");
        assert_eq!(rows[1][0].text, "Left");
        assert_eq!(rows[1][1].text, "Right");
        assert!(table_content(&tokens, &boxes[..2], &lines, None).is_none());
        assert!(table_content(
            &["<table><tr><td colspan=\"0\"></td></tr></table>".into()],
            &boxes[..1],
            &lines,
            None
        )
        .is_none());
    }
    #[test]
    fn numeric_baselines_repair_drifting_boxes_without_changing_tokens() {
        let mut rows = vec![vec![PdfOcrCell {
            text: "Header".into(),
            col_span: 4,
            row_span: 1,
        }]];
        for values in [
            ["", "2", "3", "4"],
            ["1 5", "6", "7", "8"],
            ["9", "10", "11", "12"],
        ] {
            rows.push(
                values
                    .into_iter()
                    .map(|text| PdfOcrCell {
                        text: text.into(),
                        col_span: 1,
                        row_span: 1,
                    })
                    .collect(),
            );
        }
        let evidence: Vec<_> = (0..3)
            .flat_map(|r| {
                (0..4).map(move |c| {
                    line(
                        &(r * 4 + c + 1).to_string(),
                        [
                            10. + c as f64 * 30.,
                            50. + r as f64 * 20.,
                            25. + c as f64 * 30.,
                            60. + r as f64 * 20.,
                        ],
                    )
                })
            })
            .collect();
        repair_numeric_rows(&mut rows, &evidence);
        assert_eq!(rows[1][0].text, "1");
        assert_eq!(rows[2][0].text, "5");
        let before = rows.clone();
        repair_numeric_rows(&mut rows, &evidence[1..]);
        assert_eq!(
            rows, before,
            "incomplete evidence must not rewrite the table"
        );
    }
    #[test]
    fn merged_header_repair_keeps_numeric_baseline_recovery() {
        let mut html = String::from(
            "<thead><tr><td></td><td colspan=\"3\"></td></tr>\
             <tr><td></td><td colspan=\"3\"></td></tr>\
             <tr><td></td><td></td><td></td><td></td></tr></thead><tbody>",
        );
        let mut boxes = vec![
            vec![0., 0., 100., 20.],
            vec![100., 0., 400., 20.],
            vec![0., 20., 100., 50.],
            vec![100., 20., 400., 50.],
        ];
        let mut lines = vec![line("Measures", [190., 2., 310., 12.])];
        for (c, (label, unit)) in [
            ("Name", "(id)"),
            ("Mass", "(kg)"),
            ("Length", "(m)"),
            ("Time", "(s)"),
        ]
        .into_iter()
        .enumerate()
        {
            let x = c as f64 * 100.;
            boxes.push(vec![x as f32, 50., x as f32 + 100., 80.]);
            lines.push(line(label, [x + 40., 30., x + 60., 40.]));
            lines.push(line(unit, [x + 40., 60., x + 60., 70.]));
        }
        for r in 0..3 {
            html.push_str("<tr><td></td><td></td><td></td><td></td></tr>");
            for c in 0..4 {
                let x = c as f64 * 100.;
                let y = 100. + r as f64 * 20.;
                // A drifting box sends the first value into the next row.
                let (top, bottom) = match (r, c) {
                    (0, 0) => (90., 103.),
                    (1, 0) => (104., 135.),
                    _ => (y, y + 16.),
                };
                boxes.push(vec![x as f32, top as f32, x as f32 + 100., bottom as f32]);
                lines.push(line(
                    &(r * 4 + c).to_string(),
                    [x + 40., y + 5., x + 60., y + 15.],
                ));
            }
        }
        html.push_str("</tbody>");
        let PdfOcrRegionContent::Table { rows, header_rows } =
            table_content(&[html], &boxes, &lines, None).unwrap()
        else {
            panic!("table expected");
        };
        assert_eq!(header_rows, 2);
        assert_eq!(rows[0][0].row_span, 2);
        assert_eq!(rows[1][0].text, "Mass\n(kg)");
        assert_eq!(rows.len(), 5);
        for (r, row) in rows[2..].iter().enumerate() {
            assert_eq!(row.len(), 4);
            for (c, cell) in row.iter().enumerate() {
                assert_eq!(cell.text, (r * 4 + c).to_string());
            }
        }
    }
    #[test]
    fn sparse_control_rows_are_included_in_numeric_repair() {
        let mut rows = vec![vec![PdfOcrCell {
            text: "Header".into(),
            col_span: 8,
            row_span: 1,
        }]];
        let mut evidence = Vec::new();
        for r in 0..4 {
            let mut row = Vec::new();
            for c in 0..8 {
                let text = if r == 0 && c >= 4 {
                    String::new()
                } else {
                    (r * 8 + c + 1).to_string()
                };
                if !text.is_empty() {
                    evidence.push(line(
                        &text,
                        [
                            10. + c as f64 * 30.,
                            50. + r as f64 * 20.,
                            25. + c as f64 * 30.,
                            60. + r as f64 * 20.,
                        ],
                    ));
                }
                row.push(PdfOcrCell {
                    text,
                    col_span: 1,
                    row_span: 1,
                });
            }
            rows.push(row);
        }
        rows[1][0].text.clear();
        rows[2][0].text = "1 9".into();
        repair_numeric_rows(&mut rows, &evidence);
        assert_eq!(rows[1][0].text, "1");
        assert_eq!(rows[2][0].text, "9");
        assert!(rows[1][4].text.is_empty());
    }
    #[test]
    fn columns_follow_spanning_heading() {
        let mut regions: Vec<_> = [
            ("title", [0., 0., 200., 20.]),
            ("L1", [0., 40., 90., 60.]),
            ("L2", [0., 80., 90., 100.]),
            ("R1", [110., 40., 200., 60.]),
            ("R2", [110., 80., 200., 100.]),
        ]
        .into_iter()
        .rev()
        .map(|(text, bbox)| PdfOcrRegion {
            order: 0,
            bbox,
            label: "text".into(),
            confidence: None,
            content: PdfOcrRegionContent::Text { text: text.into() },
        })
        .collect();
        order_regions(&mut regions);
        let texts: Vec<_> = regions
            .iter()
            .map(|r| match &r.content {
                PdfOcrRegionContent::Text { text } => text.as_str(),
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(texts, vec!["title", "L1", "L2", "R1", "R2"]);
    }
}
