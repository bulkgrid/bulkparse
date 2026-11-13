//! Conservative recursive whitespace cuts. No text, filenames or model labels
//! influence order. Only horizontal LTR layouts with persistent gutters qualify.
use super::PdfOcrLine;
use crate::{CancellationToken, Error};

fn bounds(line: &PdfOcrLine) -> [f64; 4] {
    let mut b = [f64::INFINITY, f64::INFINITY, 0.0_f64, 0.0_f64];
    for p in line.quad {
        b[0] = b[0].min(p[0]);
        b[1] = b[1].min(p[1]);
        b[2] = b[2].max(p[0]);
        b[3] = b[3].max(p[1]);
    }
    b
}

fn gaps(lines: &[PdfOcrLine], axis: usize) -> Vec<(f64, f64)> {
    let mut spans: Vec<_> = lines
        .iter()
        .map(|l| {
            let b = bounds(l);
            (b[axis], b[axis + 2])
        })
        .collect();
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut end = spans[0].1;
    let mut gaps = vec![];
    for (start, stop) in spans.into_iter().skip(1) {
        if start > end {
            gaps.push((end, start));
        }
        end = end.max(stop);
    }
    gaps.sort_by(|a, b| {
        (b.1 - b.0)
            .total_cmp(&(a.1 - a.0))
            .then(a.0.total_cmp(&b.0))
    });
    gaps
}

fn cut(lines: &[PdfOcrLine], height: f64) -> Option<(usize, f64)> {
    // Bound candidate scans even for adversarial pages with thousands of gaps.
    for (left, right) in gaps(lines, 0).into_iter().take(32) {
        if right - left < 1.5 * height {
            continue;
        }
        let at = (left + right) / 2.0;
        let mut counts = [0; 2];
        let mut extents = [[f64::INFINITY, 0.0_f64]; 2];
        for line in lines {
            let b = bounds(line);
            let side = usize::from(b[0] >= at);
            counts[side] += 1;
            extents[side][0] = extents[side][0].min(b[1]);
            extents[side][1] = extents[side][1].max(b[3]);
        }
        // Repeated lines in both overlapping columns; a single label/value row
        // is insufficient evidence to infer columns.
        let overlap = extents[0][1].min(extents[1][1]) - extents[0][0].max(extents[1][0]);
        if counts.iter().all(|n| *n >= 2) && overlap >= height {
            return Some((0, at));
        }
    }
    // Separate spacious horizontal bands (e.g. spanning headings/footers) only
    // when no persistent vertical gutter qualifies.
    gaps(lines, 1)
        .into_iter()
        .find(|(a, b)| b - a >= height)
        .map(|(a, b)| (1, (a + b) / 2.0))
}

fn recurse(
    mut lines: Vec<PdfOcrLine>,
    depth: usize,
    token: &CancellationToken,
    out: &mut Vec<PdfOcrLine>,
) -> Result<(), Error> {
    if token.is_cancelled() {
        return Err(Error::Cancelled);
    }
    if lines.len() > 1 && depth < 32 {
        let mut heights: Vec<_> = lines
            .iter()
            .map(|l| {
                let b = bounds(l);
                b[3] - b[1]
            })
            .collect();
        heights.sort_by(f64::total_cmp);
        let height = heights[heights.len() / 2];
        if let Some((axis, at)) = cut(&lines, height) {
            let (first, second): (Vec<_>, Vec<_>) =
                lines.into_iter().partition(|l| bounds(l)[axis] < at);
            recurse(first, depth + 1, token, out)?;
            recurse(second, depth + 1, token, out)?;
            return Ok(());
        }
    }
    // Ambiguous/overlapping regions retain the engine's supplied line order.
    out.append(&mut lines);
    Ok(())
}

pub(super) fn order(
    lines: Vec<PdfOcrLine>,
    token: &CancellationToken,
) -> Result<Vec<PdfOcrLine>, Error> {
    if token.is_cancelled() {
        return Err(Error::Cancelled);
    }
    // A horizontal whitespace model cannot reliably order rotated/skewed lines.
    // Preserve evidence and engine order instead of treating tall boxes as columns.
    if lines.iter().any(|l| {
        let b = bounds(l);
        let edge = [l.quad[1][0] - l.quad[0][0], l.quad[1][1] - l.quad[0][1]];
        b[3] - b[1] > b[2] - b[0] || edge[1].abs() > edge[0].abs() * 0.2
    }) {
        return Ok(lines);
    }
    let mut out = Vec::with_capacity(lines.len());
    recurse(lines, 0, token, &mut out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn line(text: &str, x: f64, y: f64, width: f64) -> PdfOcrLine {
        PdfOcrLine {
            text: text.into(),
            quad: [[x, y], [x + width, y], [x + width, y + 10.], [x, y + 10.]],
            confidence: 0.9,
        }
    }
    fn ordered(lines: Vec<PdfOcrLine>) -> Vec<String> {
        order(lines, &CancellationToken::default())
            .unwrap()
            .into_iter()
            .map(|l| l.text)
            .collect()
    }
    #[test]
    fn columns_use_geometry_and_are_scale_translation_invariant() {
        for (scale, dx, dy) in [(1., 0., 0.), (3.7, 113., 29.)] {
            let mut lines = vec![
                line("c", 200., 0., 70.),
                line("a", 0., 0., 70.),
                line("d", 200., 30., 70.),
                line("b", 0., 30., 70.),
            ];
            for l in &mut lines {
                for p in &mut l.quad {
                    p[0] = p[0] * scale + dx;
                    p[1] = p[1] * scale + dy;
                }
            }
            assert_eq!(ordered(lines), ["a", "b", "c", "d"]);
        }
    }
    #[test]
    fn spanning_heading_and_footer_surround_three_columns() {
        let lines = vec![
            line("footer", 0., 120., 500.),
            line("title", 0., 0., 500.),
            line("a", 0., 40., 70.),
            line("c", 200., 40., 70.),
            line("e", 400., 40., 70.),
            line("b", 0., 70., 70.),
            line("d", 200., 70., 70.),
            line("f", 400., 70., 70.),
        ];
        assert_eq!(
            ordered(lines),
            ["title", "a", "b", "c", "d", "e", "f", "footer"]
        );
    }
    #[test]
    fn ambiguous_overlapping_lines_and_single_row_keep_engine_order() {
        assert_eq!(
            ordered(vec![
                line("second", 0., 4., 80.),
                line("first", 0., 0., 80.)
            ]),
            ["second", "first"]
        );
        assert_eq!(
            ordered(vec![
                line("right", 200., 0., 80.),
                line("left", 0., 0., 80.)
            ]),
            ["right", "left"]
        );
    }
    #[test]
    fn cancellation_is_observed() {
        let token = CancellationToken::default();
        token.cancel();
        assert!(matches!(order(vec![], &token), Err(Error::Cancelled)));
    }

    #[test]
    fn skewed_lines_keep_engine_order_and_all_coordinates() {
        let mut lines = vec![
            line("right", 200., 0., 70.),
            line("left", 0., 0., 70.),
            line("right lower", 200., 30., 70.),
            line("left lower", 0., 30., 70.),
        ];
        for l in &mut lines {
            l.quad[1][1] += 20.;
            l.quad[2][1] += 20.;
        }
        let expected = lines[0].quad;
        let result = order(lines, &CancellationToken::default()).unwrap();
        assert_eq!(result[0].text, "right");
        assert_eq!(result[0].quad, expected);
    }
}
