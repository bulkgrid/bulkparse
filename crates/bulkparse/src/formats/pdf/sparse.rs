//! Tables bounded by horizontal rules with consistently aligned short rows.
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
        // Additional painted row boundaries belong to the regular grid path.
        if !(2..=3).contains(&bands.len()) {
            continue;
        }
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
        if super::tables::clips_text(glyphs, first.left, first.right, bottom, top, ctx)? {
            continue;
        }
        let mut sizes: Vec<_> = ids.iter().map(|&i| glyphs[i].em).collect();
        sizes.sort_by(f64::total_cmp);
        let em = sizes[sizes.len() / 2];
        let text_rows = lines(glyphs, &ids);
        if text_rows.len() < 4
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
        let body = &text_rows[usize::from(header)..];
        if body.len() < 3 {
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
                cuts.push((left + edge) / 2.);
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
        let anchors: Vec<_> = (0..nc)
            .map(|c| {
                body.iter()
                    .flat_map(|r| chunks(glyphs, r))
                    .filter(|part| cuts.partition_point(|&x| x < glyphs[part[0]].x) == c)
                    .map(|part| glyphs[part[0]].x)
                    .fold(f64::INFINITY, f64::min)
            })
            .collect();
        for (r, row) in text_rows.iter().enumerate() {
            ctx.node()?;
            let parts = chunks(glyphs, row);
            if r == 0 && header && parts.len() == 1 && glyphs[*row.last().unwrap()].end > cuts[0] {
                values.push(vec![Cell {
                    text: layout::text(row.iter().map(|&i| glyphs[i].clone()).collect(), ctx)?,
                    column: 0,
                    col_span: nc as u32,
                    row_span: 1,
                }]);
                continue;
            }
            let mut cells = vec![Vec::new(); nc];
            for part in &parts {
                let g = &glyphs[part[0]];
                let c = cuts.partition_point(|&x| x < g.x);
                if (c < cuts.len() && glyphs[*part.last().unwrap()].end > cuts[c] + 0.5)
                    || (r > usize::from(header) && (g.x - anchors[c]).abs() > em)
                {
                    valid = false;
                    break;
                }
                cells[c].extend(part.iter().map(|&i| glyphs[i].clone()));
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
        // Empty worksheet columns are permitted only when a multi-cell first
        // row establishes them and all later rows are short single labels.
        let worksheet = repeated == 1
            && chunks(glyphs, &text_rows[0]).len() == nc
            && body
                .iter()
                .skip(1)
                .all(|r| chunks(glyphs, r).len() == 1 && r.len() <= 24);
        if !valid || (repeated < 3 && !worksheet) {
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
    Ok(out)
}
