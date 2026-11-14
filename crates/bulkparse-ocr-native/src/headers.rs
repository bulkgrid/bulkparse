//! Recover multiline metric labels only when units and repeated body columns
//! agree. No vocabulary, reference text, or document identity enters the repair.
use crate::layout::bounds;
use bulkparse::{PdfOcrCell, PdfOcrLine};

pub(crate) fn repair(
    rows: &mut Vec<Vec<PdfOcrCell>>,
    header_rows: usize,
    lines: &[PdfOcrLine],
    assigned_rows: &[usize],
) -> bool {
    if header_rows < 3
        || header_rows >= rows.len()
        || rows[..header_rows]
            .iter()
            .flatten()
            .any(|c| c.row_span != 1)
    {
        return false;
    }
    let width: usize = rows[0].iter().map(|c| c.col_span as usize).sum();
    if !(4..=64).contains(&width) || !rows[0].iter().any(|c| c.col_span > 1) {
        return false;
    }
    let header: Vec<_> = lines
        .iter()
        .zip(assigned_rows)
        .filter(|(_, r)| **r > 0 && **r < header_rows)
        .map(|(l, _)| l)
        .collect();
    let units: Vec<_> = header
        .iter()
        .filter(|l| {
            let text = l.text.trim();
            text.starts_with('(') && text.ends_with(')') && text.chars().count() <= 16
        })
        .collect();
    if units.len() * 2 < width {
        return false;
    }
    let mut heights: Vec<_> = units
        .iter()
        .map(|l| {
            let b = bounds(l);
            b[3] - b[1]
        })
        .collect();
    heights.sort_by(f64::total_cmp);
    let height = heights[heights.len() / 2];
    let unit_y = units
        .iter()
        .map(|l| {
            let b = bounds(l);
            (b[1] + b[3]) / 2.
        })
        .sum::<f64>()
        / units.len() as f64;
    if height <= 0.
        || units.iter().any(|l| {
            let b = bounds(l);
            ((b[1] + b[3]) / 2. - unit_y).abs() > height * 0.5
        })
    {
        return false;
    }
    // A full numeric body baseline supplies independent column anchors.
    let mut body: Vec<_> = lines
        .iter()
        .zip(assigned_rows)
        .filter(|(_, r)| **r >= header_rows)
        .map(|(l, _)| l)
        .collect();
    body.sort_by(|a, b| {
        let a = bounds(a);
        let b = bounds(b);
        (a[1] + a[3]).total_cmp(&(b[1] + b[3]))
    });
    let mut groups: Vec<Vec<&PdfOcrLine>> = Vec::new();
    for line in body {
        let b = bounds(line);
        if let Some(group) = groups.last_mut() {
            let a = bounds(group[0]);
            if ((a[1] + a[3] - b[1] - b[3]) / 2.).abs() < height * 0.45 {
                group.push(line);
                continue;
            }
        }
        groups.push(vec![line]);
    }
    let full: Vec<_> = groups
        .iter()
        .filter(|g| {
            g.len() == width
                && g.iter()
                    .filter(|l| l.text.chars().any(|c| c.is_ascii_digit()))
                    .count()
                    * 5
                    >= width * 4
        })
        .collect();
    if full.len() < 3 {
        return false;
    }
    let mut anchors: Vec<f64> = full[0]
        .iter()
        .map(|l| {
            let b = bounds(l);
            (b[0] + b[2]) / 2.
        })
        .collect();
    anchors.sort_by(f64::total_cmp);
    if anchors.windows(2).any(|v| v[1] - v[0] < height) {
        return false;
    }
    for group in &full[1..] {
        let mut centers: Vec<_> = group
            .iter()
            .map(|l| {
                let b = bounds(l);
                (b[0] + b[2]) / 2.
            })
            .collect();
        centers.sort_by(f64::total_cmp);
        if centers
            .iter()
            .zip(&anchors)
            .any(|(a, b)| (a - b).abs() > height * 0.6)
        {
            return false;
        }
    }
    let column = |x: f64| {
        anchors
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| (*a - x).abs().total_cmp(&(*b - x).abs()))
            .unwrap()
            .0
    };
    let mut labels = vec![
        PdfOcrCell {
            text: String::new(),
            row_span: 1,
            col_span: 1
        };
        width
    ];
    let mut header = header;
    header.sort_by(|a, b| {
        let a = bounds(a);
        let b = bounds(b);
        a[1].total_cmp(&b[1]).then(a[0].total_cmp(&b[0]))
    });
    for line in header {
        let b = bounds(line);
        if (b[1] + b[3]) / 2. > unit_y + height * 0.6 {
            return false;
        }
        let left = column(b[0] + height * 0.15);
        let right = column(b[2] - height * 0.15);
        if left == right || line.text.split_whitespace().count() == 1 {
            append(&mut labels[column((b[0] + b[2]) / 2.)].text, &line.text);
        } else {
            // Split only at existing word boundaries, and only when each word
            // has an unambiguous projected center in a different body column.
            let words: Vec<_> = line.text.split_whitespace().collect();
            if right - left + 1 > words.len() {
                return false;
            }
            let total = line.text.chars().count() as f64;
            let mut offset = 0.;
            let mut last = left;
            for word in &words {
                let len = word.chars().count() as f64;
                let x = b[0] + (offset + len / 2.) / total * (b[2] - b[0]);
                let c = column(x);
                if c < last || c > last + 1 || c > right {
                    return false;
                }
                append(&mut labels[c].text, word);
                last = c;
                offset += len + 1.;
            }
            if last != right {
                return false;
            }
        }
    }
    if labels.iter().any(|c| c.text.is_empty()) {
        return false;
    }
    let words = |values: Vec<&str>| {
        let mut v: Vec<String> = values
            .iter()
            .flat_map(|s| s.split_whitespace().map(str::to_owned))
            .collect();
        v.sort();
        v
    };
    if words(
        rows[1..header_rows]
            .iter()
            .flatten()
            .map(|c| c.text.as_str())
            .collect(),
    ) != words(labels.iter().map(|c| c.text.as_str()).collect())
    {
        return false;
    }
    let mut top = rows[0].clone();
    let mut lower = Vec::new();
    let mut col = 0;
    for cell in &mut top {
        if cell.col_span == 1 && cell.text.trim().is_empty() {
            cell.text = labels[col].text.clone();
            cell.row_span = 2;
        } else {
            lower.extend_from_slice(&labels[col..col + cell.col_span as usize]);
        }
        col += cell.col_span as usize;
    }
    rows.splice(..header_rows, [top, lower]);
    true
}
fn append(target: &mut String, text: &str) {
    if !target.is_empty() {
        target.push('\n');
    }
    target.push_str(text);
}

#[cfg(test)]
mod tests {
    use super::*;
    fn cell(text: &str, span: u32) -> PdfOcrCell {
        PdfOcrCell {
            text: text.into(),
            col_span: span,
            row_span: 1,
        }
    }
    fn line(text: &str, x: f64, y: f64, w: f64) -> PdfOcrLine {
        PdfOcrLine {
            text: text.into(),
            confidence: 0.99,
            quad: [[x, y], [x + w, y], [x + w, y + 10.], [x, y + 10.]],
        }
    }
    fn fixture() -> (Vec<Vec<PdfOcrCell>>, Vec<PdfOcrLine>, Vec<usize>) {
        let mut rows = vec![
            vec![cell("", 1), cell("Measures", 3)],
            vec![cell("Name", 1), cell("Mass Length Time", 3)],
            vec![
                cell("(id)", 1),
                cell("(kg)", 1),
                cell("(m)", 1),
                cell("(s)", 1),
            ],
        ];
        let mut lines = vec![line("Measures", 190., 0., 220.)];
        let mut assigned = vec![0];
        for (i, name) in ["Name", "Mass", "Length", "Time"].iter().enumerate() {
            lines.push(line(name, 90. + i as f64 * 100., 30., 20.));
            assigned.push(1);
        }
        for (i, unit) in ["(id)", "(kg)", "(m)", "(s)"].iter().enumerate() {
            lines.push(line(unit, 90. + i as f64 * 100., 60., 20.));
            assigned.push(2);
        }
        for r in 0..3 {
            let mut row = Vec::new();
            for c in 0..4 {
                let value = (r * 4 + c).to_string();
                row.push(cell(&value, 1));
                lines.push(line(
                    &value,
                    90. + c as f64 * 100.,
                    100. + r as f64 * 20.,
                    20.,
                ));
                assigned.push(3 + r);
            }
            rows.push(row);
        }
        (rows, lines, assigned)
    }
    #[test]
    fn metric_headers_rejoin_labels_preserving_body_and_tokens() {
        let (mut rows, lines, assigned) = fixture();
        let body = rows[3..].to_vec();
        assert!(repair(&mut rows, 3, &lines, &assigned));
        assert_eq!(
            rows[0][0],
            PdfOcrCell {
                text: "Name\n(id)".into(),
                row_span: 2,
                col_span: 1
            }
        );
        assert_eq!(rows[0][1].text, "Measures");
        assert_eq!(rows[1][0].text, "Mass\n(kg)");
        assert_eq!(rows[2..], body);
    }
    #[test]
    fn uncertain_headers_keep_original_cells() {
        let (rows, lines, assigned) = fixture();
        for change in 0..4 {
            let mut candidate = rows.clone();
            let mut evidence = lines.clone();
            let mut assignments = assigned.clone();
            match change {
                0 => {
                    evidence[2].text = "other".into();
                }
                1 => {
                    candidate[1][0].row_span = 2;
                }
                2 => {
                    evidence[5].quad.iter_mut().for_each(|p| p[1] += 30.);
                }
                _ => {
                    evidence.truncate(evidence.len() - 1);
                    assignments.pop();
                }
            }
            let original = candidate.clone();
            assert!(!repair(&mut candidate, 3, &evidence, &assignments));
            assert_eq!(candidate, original);
        }
    }
}
