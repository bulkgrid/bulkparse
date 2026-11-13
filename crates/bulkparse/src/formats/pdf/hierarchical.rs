//! Nested, unboxed headers over repeatedly aligned data columns.
use super::{
    layout,
    positioned::Glyph,
    rules::Rule,
    tables::{chunks, lines, numeric, Cell, Table},
};
use crate::{Context, Error};

pub(super) fn extract(
    glyphs: &[Glyph],
    rules: &[Rule],
    used: &mut [bool],
    ctx: &mut Context<'_>,
) -> Result<Vec<Table>, Error> {
    if glyphs
        .iter()
        .filter(|g| !g.text.trim().is_empty())
        .take(12)
        .count()
        < 12
    {
        return Ok(Vec::new());
    }
    let hs: Vec<_> = rules
        .iter()
        .filter(|r| !r.vertical && !r.background)
        .collect();
    let mut out = Vec::new();
    for top in &hs {
        ctx.check()?;
        for bottom in &hs {
            if top.y <= bottom.y
                || (top.left - bottom.left).abs() > 1.
                || (top.right - bottom.right).abs() > 1.
            {
                continue;
            }
            ctx.node()?;
            // This detector is for unboxed headers. A spanning vertical grid
            // is handled by the explicit-grid detector, without repeatedly
            // scanning every text pair in each possible subrectangle.
            if rules.iter().any(|v| {
                v.vertical
                    && !v.background
                    && v.y >= top.left - 1.
                    && v.y <= top.right + 1.
                    && v.left <= bottom.y + 1.
                    && v.right >= top.y - 1.
            }) {
                continue;
            }
            let partial: Vec<_> = hs
                .iter()
                .filter(|r| {
                    r.y < top.y - 1.
                        && r.y > bottom.y + 1.
                        && r.left >= top.left
                        && r.right <= top.right
                        && r.right - r.left < (top.right - top.left) * 0.9
                })
                .collect();
            let mut levels: Vec<_> = partial.iter().map(|r| r.y).collect();
            levels.sort_by(f64::total_cmp);
            levels.dedup_by(|a, b| (*a - *b).abs() < 1.);
            if levels.len() < 2 {
                continue;
            }
            let ids: Vec<_> = glyphs
                .iter()
                .enumerate()
                .filter(|(_, g)| {
                    g.rotation == 0
                        && !g.text.trim().is_empty()
                        && g.x >= top.left - 0.5
                        && g.end <= top.right + 0.5
                        && g.y < top.y
                        && g.y > bottom.y
                })
                .map(|(i, _)| i)
                .collect();
            if ids.is_empty()
                || ids.iter().any(|&i| used[i])
                || super::tables::clips_text(glyphs, top.left, top.right, bottom.y, top.y, ctx)?
            {
                continue;
            }
            let rows = lines(glyphs, &ids);
            let parts: Vec<_> = rows.iter().map(|r| chunks(glyphs, r)).collect();
            let counts: Vec<_> = parts
                .iter()
                .map(|r| {
                    r.iter()
                        .filter(|p| {
                            numeric(
                                &p.iter()
                                    .map(|&i| glyphs[i].text.as_str())
                                    .collect::<String>(),
                            )
                        })
                        .count()
                })
                .collect();
            let Some(first) = counts.iter().position(|&n| n >= 3) else {
                continue;
            };
            if !(2..=4).contains(&first) || rows.len() - first < 3 {
                continue;
            }
            let nc = parts[first].len();
            if nc < 4 || counts[first + 1] < 3 || parts[first - 1].len() + 1 < nc {
                continue;
            }
            let em = rows[first].iter().map(|&i| glyphs[i].em).fold(0., f64::max);
            let centers: Vec<_> = parts[first]
                .iter()
                .map(|p| (glyphs[p[0]].x + glyphs[*p.last().unwrap()].end) / 2.)
                .collect();
            let column = |part: &Vec<usize>| {
                let x = (glyphs[part[0]].x + glyphs[*part.last().unwrap()].end) / 2.;
                centers
                    .iter()
                    .enumerate()
                    .min_by(|a, b| (a.1 - x).abs().total_cmp(&(b.1 - x).abs()))
                    .unwrap()
                    .0
            };
            if parts[first..]
                .iter()
                .any(|r| r.len() != nc || r.iter().enumerate().any(|(c, p)| column(p) != c))
            {
                continue;
            }
            if rows
                .windows(2)
                .any(|w| glyphs[w[0][0]].y - glyphs[w[1][0]].y > em * 2.5)
            {
                continue;
            }
            if rows
                .len()
                .checked_mul(nc)
                .is_none_or(|n| n > ctx.options.limits.max_table_cells)
            {
                return Err(Error::Limit("max_table_cells"));
            }
            let mut result: Vec<Vec<Cell>> = (0..rows.len()).map(|_| Vec::new()).collect();
            let mut occupied = vec![false; nc];
            let leaf: Vec<_> = parts[first - 1].iter().map(column).collect();
            let mut reserved = Vec::new();
            for (r, row) in parts[..first - 1].iter().enumerate() {
                for (i, p) in row.iter().enumerate() {
                    let c = column(p);
                    if !leaf.contains(&c)
                        && !occupied[c]
                        && parts[..first]
                            .iter()
                            .flatten()
                            .filter(|p| column(p) == c)
                            .count()
                            == 1
                    {
                        occupied[c] = true;
                        reserved.push((r, i));
                        result[0].push(Cell {
                            text: layout::text(
                                p.iter().map(|&i| glyphs[i].clone()).collect(),
                                ctx,
                            )?,
                            column: c,
                            col_span: 1,
                            row_span: first as u32,
                        });
                    }
                }
            }
            let available: Vec<_> = (0..nc).filter(|&c| !occupied[c]).collect();
            let mut valid = true;
            for r in 0..first {
                let labels: Vec<_> = parts[r]
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| !reserved.contains(&(r, *i)))
                    .map(|(_, p)| p)
                    .collect();
                if labels.is_empty() {
                    valid = false;
                    break;
                }
                let mut groups = vec![Vec::new(); labels.len()];
                // Header centering and painted subgroup rules constrain the same
                // leaf columns; assignment is monotone and independent of wording.
                for &c in &available {
                    let k = labels
                        .iter()
                        .enumerate()
                        .min_by(|a, b| {
                            let midpoint = |p: &&Vec<usize>| {
                                (glyphs[p[0]].x + glyphs[*p.last().unwrap()].end) / 2.
                            };
                            (midpoint(a.1) - centers[c])
                                .abs()
                                .total_cmp(&(midpoint(b.1) - centers[c]).abs())
                        })
                        .unwrap()
                        .0;
                    groups[k].push(c);
                }
                for (label, columns) in labels.into_iter().zip(groups) {
                    if columns.is_empty()
                        || columns.last().unwrap() - columns[0] + 1 != columns.len()
                    {
                        valid = false;
                        break;
                    }
                    result[r].push(Cell {
                        text: layout::text(
                            label.iter().map(|&i| glyphs[i].clone()).collect(),
                            ctx,
                        )?,
                        column: columns[0],
                        col_span: columns.len() as u32,
                        row_span: 1,
                    });
                }
                result[r].sort_by_key(|c| c.column);
            }
            if !valid {
                continue;
            }
            for r in first..rows.len() {
                for (c, p) in parts[r].iter().enumerate() {
                    result[r].push(Cell::plain(
                        layout::text(p.iter().map(|&i| glyphs[i].clone()).collect(), ctx)?,
                        c,
                    ));
                }
            }
            for _ in 0..result.iter().map(Vec::len).sum::<usize>() {
                ctx.cell()?;
            }
            for (i, g) in glyphs.iter().enumerate() {
                if g.rotation == 0
                    && g.x >= top.left - 0.5
                    && g.end <= top.right + 0.5
                    && g.y < top.y
                    && g.y > bottom.y
                {
                    used[i] = true;
                }
            }
            out.push(Table {
                left: top.left,
                right: top.right,
                top: top.y,
                bottom: bottom.y,
                em,
                rows: result,
            });
        }
    }
    Ok(out)
}
