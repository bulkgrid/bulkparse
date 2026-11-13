//! Numeric tables with separate header and body rules and repeated column alignment.
use super::{
    layout,
    positioned::Glyph,
    rules::Rule,
    tables::{chunks, lines, Cell, Table},
};
use crate::{Context, Error};

pub(super) fn extract(
    glyphs: &[Glyph],
    rules: &[Rule],
    used: &mut [bool],
    ctx: &mut Context<'_>,
) -> Result<Vec<Table>, Error> {
    let mut horizontal: Vec<_> = rules.iter().filter(|r| !r.vertical).copied().collect();
    horizontal.sort_by(|a, b| {
        a.left
            .total_cmp(&b.left)
            .then(a.right.total_cmp(&b.right))
            .then(b.y.total_cmp(&a.y))
    });
    let mut out = Vec::new();
    let mut start = 0;
    while start < horizontal.len() {
        ctx.node()?;
        let first = horizontal[start];
        let mut end = start + 1;
        while end < horizontal.len()
            && (horizontal[end].left - first.left).abs() < 1.
            && (horizontal[end].right - first.right).abs() < 1.
        {
            end += 1;
        }
        let mut bands: Vec<_> = horizontal[start..end].iter().map(|r| r.y).collect();
        start = end;
        bands.sort_by(|a, b| b.total_cmp(a));
        bands.dedup_by(|a, b| (*a - *b).abs() < 1.);
        // Separate tables can share horizontal extents. Validate each bounded
        // header/body window instead of rejecting the entire group of rules.
        let windows: Vec<Vec<f64>> = bands.windows(3).map(|w| w.to_vec()).collect();
        for bands in windows {
            let top = bands[0];
            let bottom = *bands.last().unwrap();
            let ids: Vec<_> = glyphs
                .iter()
                .enumerate()
                .filter(|(_, g)| {
                    g.rotation == 0
                        && g.y > bottom
                        && g.y < top
                        && g.x >= first.left - 0.5
                        && g.end <= first.right + 0.5
                        && !g.text.trim().is_empty()
                })
                .map(|(i, _)| i)
                .collect();
            if ids.is_empty() || ids.iter().any(|&i| used[i]) {
                continue;
            }
            let mut sizes: Vec<_> = ids.iter().map(|&i| glyphs[i].em).collect();
            sizes.sort_by(f64::total_cmp);
            let em = sizes[sizes.len() / 2];
            let text_rows = lines(glyphs, &ids);
            if text_rows.len() < 3
                || text_rows.windows(2).any(|w| {
                    let gap = glyphs[w[0][0]].y - glyphs[w[1][0]].y;
                    gap > em * 3. || gap < em * 0.7
                })
            {
                continue;
            }
            let header = bands.len() == 3
                && text_rows
                    .iter()
                    .filter(|r| glyphs[r[0]].y > bands[1])
                    .count()
                    == 1;
            if !header {
                continue;
            }
            let body = &text_rows[1..];
            if body.len() < 2 || (body.len() == 2 && !header) {
                continue;
            }
            let mut intervals: Vec<_> = body
                .iter()
                .flatten()
                .map(|&i| (glyphs[i].x, glyphs[i].end))
                .collect();
            intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut edge = intervals[0].1;
            let mut cuts = Vec::new();
            for &(left, right) in &intervals[1..] {
                if left - edge > (em * 0.6).max(2.5) {
                    // A long header may extend past the midpoint of a body gutter.
                    // Keep the cut in whitespace shared by header and body.
                    let mut gaps = vec![(edge, left)];
                    if header {
                        for &i in &text_rows[0] {
                            let g = &glyphs[i];
                            gaps = gaps
                                .into_iter()
                                .flat_map(|(a, b)| {
                                    if g.end <= a || g.x >= b {
                                        vec![(a, b)]
                                    } else {
                                        [(a, g.x.max(a)), (g.end.min(b), b)]
                                            .into_iter()
                                            .filter(|(x, y)| y - x >= em * 0.15)
                                            .collect()
                                    }
                                })
                                .collect();
                        }
                    }
                    if let Some(&(a, b)) = gaps
                        .iter()
                        .max_by(|a, b| (a.1 - a.0).total_cmp(&(b.1 - b.0)))
                    {
                        cuts.push((a + b) / 2.);
                    }
                }
                edge = edge.max(right);
            }
            if cuts.is_empty() || cuts.len() > 11 {
                continue;
            }
            let nc = cuts.len() + 1;
            if text_rows
                .len()
                .checked_mul(nc)
                .is_none_or(|n| n > ctx.options.limits.max_table_cells)
            {
                return Err(Error::Limit("max_table_cells"));
            }
            let mut values = Vec::new();
            let mut valid = true;
            let mut repeated = 0;
            for (r, row) in text_rows.iter().enumerate() {
                ctx.node()?;
                let parts = chunks(glyphs, row);
                if r == 0
                    && header
                    && parts.len() == 1
                    && glyphs[*row.last().unwrap()].end > cuts[0]
                {
                    values.push(vec![Cell {
                        text: layout::text(row.iter().map(|&i| glyphs[i].clone()).collect(), ctx)?,
                        column: 0,
                        col_span: nc as u32,
                        row_span: 1,
                    }]);
                    continue;
                }
                let mut cells = vec![Vec::new(); nc];
                for &i in row {
                    let g = &glyphs[i];
                    let c = cuts.partition_point(|&x| x < (g.x + g.end) / 2.);
                    if (c < cuts.len() && g.end > cuts[c] + 0.5)
                        || (c > 0 && g.x < cuts[c - 1] - 0.5)
                    {
                        valid = false;
                        break;
                    }
                    cells[c].push(g.clone());
                }
                // Repeated left, centre or right alignment is valid. Comparing only
                // left edges rejects right-aligned values and centred symbols.
                if r == 1 {
                    for (c, cell) in cells.iter().enumerate() {
                        if cell.is_empty() {
                            continue;
                        }
                        let bounds = |row: &[usize]| {
                            let gs: Vec<_> = row
                                .iter()
                                .map(|&i| &glyphs[i])
                                .filter(|g| cuts.partition_point(|&x| x < (g.x + g.end) / 2.) == c)
                                .collect();
                            (!gs.is_empty()).then(|| {
                                (
                                    gs.iter().map(|g| g.x).fold(f64::INFINITY, f64::min),
                                    gs.iter().map(|g| g.end).fold(f64::NEG_INFINITY, f64::max),
                                )
                            })
                        };
                        let boxes: Vec<_> = body.iter().filter_map(|r| bounds(r)).collect();
                        let aligned = [0, 1, 2].iter().any(|mode| {
                            let coord = |b: &(f64, f64)| match mode {
                                0 => b.0,
                                1 => (b.0 + b.1) / 2.,
                                _ => b.1,
                            };
                            boxes.iter().map(coord).fold(f64::NEG_INFINITY, f64::max)
                                - boxes.iter().map(coord).fold(f64::INFINITY, f64::min)
                                <= em
                        });
                        if !aligned {
                            valid = false;
                        }
                    }
                }
                if cells.iter().filter(|c| !c.is_empty()).count() >= 2 {
                    repeated += 1;
                }
                let mut row_values = Vec::new();
                for (c, cell) in cells.into_iter().enumerate() {
                    row_values.push(Cell::plain(layout::text(cell, ctx)?, c));
                }
                values.push(row_values);
            }
            // Numeric repetition distinguishes rows from wrapped paragraphs or
            // plot labels. Every body row must establish the same columns.
            let numeric_body = values.iter().skip(1).all(|r| {
                let n = r.iter().filter(|c| super::tables::numeric(&c.text)).count();
                n >= 3 && n * 2 >= nc && r.iter().filter(|c| !c.text.is_empty()).count() >= nc - 1
            });
            let numeric_header = values[0]
                .iter()
                .filter(|c| super::tables::numeric(&c.text))
                .count();
            if !valid || repeated < 3 || !numeric_body || numeric_header * 2 >= nc {
                continue;
            }
            if super::tables::clips_text(glyphs, first.left, first.right, bottom, top, ctx)? {
                continue;
            }
            let cells = values
                .len()
                .checked_mul(nc)
                .ok_or(Error::Limit("max_table_cells"))?;
            if cells > ctx.options.limits.max_table_cells {
                return Err(Error::Limit("max_table_cells"));
            }
            for _ in 0..values.iter().map(Vec::len).sum::<usize>() {
                ctx.cell()?;
            }
            // Claim only the region whose text was validated, including whitespace.
            for (i, g) in glyphs.iter().enumerate() {
                if g.rotation == 0
                    && g.y > bottom
                    && g.y < top
                    && g.x >= first.left - 0.5
                    && g.end <= first.right + 0.5
                {
                    used[i] = true;
                }
            }
            out.push(Table {
                left: first.left,
                right: first.right,
                top,
                bottom,
                em,
                rows: values,
            });
        }
    }
    Ok(out)
}
