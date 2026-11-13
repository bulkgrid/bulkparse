//! Bounded, styled multi-column tables whose paragraphs establish row starts.
use super::{
    layout,
    positioned::Glyph,
    rules::Rule,
    tables::{lines, Cell, Table},
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
        .filter(|g| g.bold && !g.text.trim().is_empty())
        .take(12)
        .count()
        < 12
    {
        return Ok(Vec::new());
    }
    let mut hs: Vec<_> = rules
        .iter()
        .filter(|r| !r.vertical && r.background)
        .collect();
    hs.sort_by(|a, b| b.y.total_cmp(&a.y));
    let mut out = Vec::new();
    for top in &hs {
        ctx.check()?;
        for bottom in hs.iter().rev() {
            if top.y - bottom.y < 40.
                || (top.left - bottom.left).abs() > 1.
                || (top.right - bottom.right).abs() > 1.
            {
                continue;
            }
            ctx.node()?;
            let enclosing = |x: f64| {
                rules.iter().any(|v| {
                    v.vertical
                        && (v.y - x).abs() < 1.
                        && v.left <= bottom.y + 1.
                        && v.right >= top.y - 1.
                })
            };
            if !enclosing(top.left) || !enclosing(top.right) {
                continue;
            }
            let ids: Vec<_> = glyphs
                .iter()
                .enumerate()
                .filter(|(_, g)| {
                    g.rotation == 0
                        && !g.text.trim().is_empty()
                        && g.x >= top.left
                        && g.end <= top.right
                        && g.y < top.y
                        && g.y > bottom.y
                })
                .map(|(i, _)| i)
                .collect();
            if ids.len() < 30 || ids.iter().any(|&i| used[i]) {
                continue;
            }
            let mut sizes: Vec<_> = ids.iter().map(|&i| glyphs[i].em).collect();
            sizes.sort_by(f64::total_cmp);
            let em = sizes[sizes.len() / 2];
            let mut intervals: Vec<_> = ids.iter().map(|&i| (glyphs[i].x, glyphs[i].end)).collect();
            intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut edge = intervals[0].1;
            let mut cuts = Vec::new();
            for (a, b) in intervals.into_iter().skip(1) {
                if a - edge > em * 1.2 {
                    cuts.push((a + edge) / 2.);
                }
                edge = edge.max(b);
            }
            let nc = cuts.len() + 1;
            if !(3..=8).contains(&nc) {
                continue;
            }
            let mut cols = vec![Vec::new(); nc];
            for &i in &ids {
                cols[cuts.partition_point(|&x| x < glyphs[i].x)].push(i);
            }
            let labels: Vec<_> = cols
                .iter()
                .enumerate()
                .filter(|(_, c)| c.iter().filter(|&&i| glyphs[i].bold).count() * 5 >= c.len() * 4)
                .map(|(i, _)| i)
                .collect();
            if labels.len() < 2 || labels.len() == nc {
                continue;
            }
            let mut templates = Vec::new();
            for &c in &labels {
                let rows = lines(glyphs, &cols[c]);
                let mut groups: Vec<Vec<usize>> = Vec::new();
                for row in rows {
                    if groups.last().is_none_or(|g| {
                        glyphs[*g.last().unwrap()].y - glyphs[row[0]].y > glyphs[row[0]].em * 2.2
                    }) {
                        groups.push(Vec::new());
                    }
                    groups.last_mut().unwrap().extend(row);
                }
                templates.push(groups);
            }
            let template = templates.iter().max_by_key(|r| r.len()).unwrap();
            if template.len() < 5 {
                continue;
            }
            // Reject non-tabular background bands before the more expensive
            // perimeter-word check. Dense ordinary grids often have many such
            // bands; none should spend the document's work budget twice.
            if super::tables::clips_text(glyphs, top.left, top.right, bottom.y, top.y, ctx)? {
                continue;
            }
            let nr = template.len();
            if nr
                .checked_mul(nc)
                .is_none_or(|n| n > ctx.options.limits.max_table_cells)
            {
                return Err(Error::Limit("max_table_cells"));
            }
            let mut ys = vec![top.y];
            // Labels mark row starts, not row centres. A long paragraph in the
            // preceding row can extend far below its short label. Allow baseline
            // variation at the next start without cutting that paragraph in half.
            for pair in template.windows(2) {
                let next = glyphs[pair[1][0]].y;
                ys.push(next + em * 1.2);
            }
            ys.push(bottom.y);
            let mut buckets = vec![vec![Vec::new(); nc]; nr];
            for (c, col) in cols.iter().enumerate() {
                for &i in col {
                    let r = ys
                        .partition_point(|&y| y > glyphs[i].y)
                        .saturating_sub(1)
                        .min(nr - 1);
                    buckets[r][c].push(glyphs[i].clone());
                }
            }
            // Require a fully populated header and repeated multi-column body rows.
            if buckets[0].iter().any(Vec::is_empty)
                || buckets
                    .iter()
                    .skip(1)
                    .filter(|r| r.iter().filter(|c| !c.is_empty()).count() >= 3)
                    .count()
                    < nr - 2
            {
                continue;
            }
            let mut result: Vec<Vec<Cell>> = (0..nr).map(|_| Vec::new()).collect();
            for (c, _) in cols.iter().enumerate() {
                let mut r = 0;
                while r < nr {
                    let mut origin = r;
                    if labels.contains(&c) && r > 0 && buckets[r][c].is_empty() {
                        let mut next = r + 1;
                        while next < nr && buckets[next][c].is_empty() {
                            next += 1;
                        }
                        // A label aligned at the bottom of a tall cell occupies its
                        // leading blank rows. Trailing blanks remain explicit:
                        // absence of a border is insufficient evidence to merge.
                        if next < nr && next - r >= 2 {
                            r = next;
                        } else {
                            origin = r;
                        }
                    }
                    let text = layout::text(std::mem::take(&mut buckets[r][c]), ctx)?;
                    ctx.cell()?;
                    result[origin].push(Cell {
                        text,
                        column: c,
                        col_span: 1,
                        row_span: (r - origin + 1) as u32,
                    });
                    r += 1;
                }
            }
            for row in &mut result {
                row.sort_by_key(|c| c.column);
            }
            for (i, g) in glyphs.iter().enumerate() {
                if g.rotation == 0
                    && g.x >= top.left
                    && g.end <= top.right
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
            break;
        }
    }
    Ok(out)
}
