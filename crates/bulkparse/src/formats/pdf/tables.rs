//! Geometric tables from painted rules and repeated cell alignment.
use super::{layout, positioned::Glyph, rules::Rule};
use crate::{Context, Error};

pub(super) struct Table {
    pub left: f64,
    pub right: f64,
    pub top: f64,
    pub bottom: f64,
    pub em: f64,
    pub rows: Vec<Vec<Cell>>,
}

pub(super) struct Cell {
    pub text: String,
    pub column: usize,
    pub col_span: u32,
    pub row_span: u32,
}
impl Cell {
    pub(super) fn plain(text: String, column: usize) -> Self {
        Self {
            text,
            column,
            col_span: 1,
            row_span: 1,
        }
    }
}

// Reject boundaries through glyphs or unspaced words, even when no individual
// glyph straddles the edge. Preserve the whole line as ordinary text instead.
pub(super) fn clips_text(
    glyphs: &[Glyph],
    left: f64,
    right: f64,
    bottom: f64,
    top: f64,
    ctx: &mut Context<'_>,
) -> Result<bool, Error> {
    for x in [left, right] {
        let nearby: Vec<_> = glyphs
            .iter()
            .filter(|g| {
                g.rotation == 0
                    && g.y > bottom
                    && g.y < top
                    && !g.text.trim().is_empty()
                    && g.x < x + g.em
                    && g.end > x - g.em
            })
            .collect();
        for a in &nearby {
            ctx.node()?;
            if a.x < x - 0.5 && a.end > x + 0.5 {
                return Ok(true);
            }
            if a.end > x + 0.5 {
                continue;
            }
            for b in &nearby {
                ctx.node()?;
                if b.x >= x - 0.5
                    && (a.y - b.y).abs() < a.em.min(b.em) * 0.3
                    && b.x >= a.end - 0.5
                    && b.x - a.end < a.em.min(b.em) * 0.15
                {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}

// PDF writers often paint one edge per cell, or duplicate the edges of adjacent
// rectangles. Join collinear fragments before looking for complete table rows.
fn merge_rules(mut rules: Vec<Rule>, ctx: &Context<'_>) -> Result<Vec<Rule>, Error> {
    rules.sort_by(|a, b| {
        a.background
            .cmp(&b.background)
            .then(a.vertical.cmp(&b.vertical))
            .then(a.y.total_cmp(&b.y))
            .then(a.left.total_cmp(&b.left))
    });
    let mut out: Vec<Rule> = Vec::new();
    let mut start = 0;
    while start < rules.len() {
        ctx.check()?;
        let first = rules[start];
        let mut end = start + 1;
        while end < rules.len()
            && rules[end].background == first.background
            && rules[end].vertical == first.vertical
            && (rules[end].y - first.y).abs() <= 1.
        {
            end += 1;
        }
        rules[start..end].sort_by(|a, b| a.left.total_cmp(&b.left));
        let mut edge = rules[start];
        for r in &rules[start + 1..end] {
            if r.left <= edge.right + 1.5 {
                edge.right = edge.right.max(r.right);
            } else {
                out.push(edge);
                edge = *r;
            }
        }
        out.push(edge);
        start = end;
    }
    Ok(out)
}

pub(super) fn lines(glyphs: &[Glyph], ids: &[usize]) -> Vec<Vec<usize>> {
    let mut ids = ids.to_vec();
    ids.sort_by(|&a, &b| {
        glyphs[b]
            .y
            .total_cmp(&glyphs[a].y)
            .then(glyphs[a].x.total_cmp(&glyphs[b].x))
    });
    let mut out: Vec<Vec<usize>> = Vec::new();
    for id in ids {
        if out.last().is_none_or(|row| {
            (glyphs[row[0]].y - glyphs[id].y).abs() > glyphs[row[0]].em.min(glyphs[id].em) * 0.3
        }) {
            out.push(Vec::new());
        }
        out.last_mut().unwrap().push(id);
    }
    for row in &mut out {
        row.sort_by(|&a, &b| glyphs[a].x.total_cmp(&glyphs[b].x));
    }
    out
}

pub(super) fn numeric(text: &str) -> bool {
    let text = text.trim();
    // Measurements and scientific notation are numeric alignment evidence too.
    // Require a recognised short unit, never arbitrary words after a number.
    let text = [
        "µL", "μL", "mL", "ml", "kg", "mg", "cm", "mm", "km", "ms", "Hz", "kPa", "MPa", "°C", "°F",
        "L", "l", "g", "m", "s", "K", "M", "B",
    ]
    .iter()
    .find_map(|unit| text.strip_suffix(unit))
    .unwrap_or(text)
    .trim();
    text.chars().any(|c| c.is_numeric())
        && text
            .chars()
            .all(|c| c.is_numeric() || c.is_whitespace() || ",.+-−()%$€£¥~±*eE".contains(c))
}

pub(super) fn chunks(glyphs: &[Glyph], ids: &[usize]) -> Vec<Vec<usize>> {
    let mut out: Vec<Vec<usize>> = Vec::new();
    for &id in ids {
        if out.last().is_none_or(|part| {
            let previous = &glyphs[*part.last().unwrap()];
            glyphs[id].x - previous.end > glyphs[id].em.min(previous.em) * 0.7
        }) {
            out.push(Vec::new());
        }
        out.last_mut().unwrap().push(id);
    }
    out
}

fn numeric_lines(glyphs: &[Glyph], ids: &[usize], minimum: usize) -> Vec<f64> {
    lines(glyphs, ids)
        .into_iter()
        .filter_map(|line| {
            let parts = chunks(glyphs, &line);
            let count = parts
                .iter()
                .filter(|part| {
                    numeric(
                        &part
                            .iter()
                            .map(|&i| glyphs[i].text.as_str())
                            .collect::<String>(),
                    )
                })
                .count();
            (count >= minimum && count * 2 >= parts.len()).then_some(glyphs[line[0]].y)
        })
        .collect()
}

// Without painted borders require at least three aligned numeric columns over
// several rows. Include an aligned header if present; without one require four
// data rows. Never label the first data row as a semantic header. Two prose
// columns, isolated number lists and sparse alignments are insufficient.
fn borderless(
    glyphs: &[Glyph],
    used: &mut [bool],
    ctx: &mut Context<'_>,
) -> Result<Vec<Table>, Error> {
    let ids: Vec<_> = glyphs
        .iter()
        .enumerate()
        .filter(|(i, g)| !used[*i] && g.rotation == 0 && !g.text.trim().is_empty())
        .map(|(i, _)| i)
        .collect();
    let rows = lines(glyphs, &ids);
    let parts: Vec<_> = rows.iter().map(|row| chunks(glyphs, row)).collect();
    let counts: Vec<_> = parts
        .iter()
        .map(|row| {
            row.iter()
                .filter(|cell| {
                    numeric(
                        &cell
                            .iter()
                            .map(|&i| glyphs[i].text.as_str())
                            .collect::<String>(),
                    )
                })
                .count()
        })
        .collect();
    let mut tables = Vec::new();
    let mut start = 0;
    while start < rows.len() {
        ctx.node()?;
        if used[rows[start][0]] || counts[start] < 3 || counts[start] * 2 < parts[start].len() {
            start += 1;
            continue;
        }
        let template = &parts[start];
        let em = rows[start].iter().map(|&i| glyphs[i].em).fold(0., f64::max);
        let ncols = template.len();
        let mut cuts: Vec<_> = template
            .windows(2)
            .map(|pair| (glyphs[*pair[0].last().unwrap()].end + glyphs[pair[1][0]].x) / 2.)
            .collect();
        let mut aligned_header = false;
        if start > 0
            && !used[rows[start - 1][0]]
            && counts[start - 1] == 0
            && glyphs[rows[start - 1][0]].y - glyphs[rows[start][0]].y <= em * 2.
        {
            let header = &rows[start - 1];
            let mut candidate = Vec::new();
            for pair in template.windows(2) {
                let lo = glyphs[*pair[0].last().unwrap()].end;
                let hi = glyphs[pair[1][0]].x;
                let gap = header
                    .windows(2)
                    .filter_map(|w| {
                        let a = &glyphs[w[0]];
                        let b = &glyphs[w[1]];
                        let x = (a.end + b.x) / 2.;
                        (b.x - a.end > em * 0.15 && x >= lo && x <= hi).then_some(x)
                    })
                    .min_by(|a, b| (a - hi).abs().total_cmp(&(b - hi).abs()));
                if let Some(x) = gap {
                    candidate.push(x);
                }
            }
            if candidate.len() == cuts.len() {
                let mut cells = vec![Vec::new(); ncols];
                for &i in header {
                    cells[candidate.partition_point(|&x| x < glyphs[i].x)].push(i);
                }
                aligned_header = cells.iter().enumerate().all(|(c, cell)| {
                    !cell.is_empty()
                        && (glyphs[cell[0]].x - glyphs[template[c][0]].x).abs() <= em * 2.
                        && cell
                            .iter()
                            .any(|&i| glyphs[i].text.chars().any(char::is_alphabetic))
                });
                if aligned_header {
                    cuts = candidate;
                }
            }
        }
        let fits = |row: usize| -> bool {
            if parts[row].len() > ncols || parts[row].len() < 2 {
                return false;
            }
            let mut occupied = vec![false; ncols];
            for part in &parts[row] {
                let left = glyphs[part[0]].x;
                let right = glyphs[*part.last().unwrap()].end;
                let col = cuts.partition_point(|&x| x < left);
                if (col < cuts.len() && right > cuts[col] + 0.5) || occupied[col] {
                    return false;
                }
                let anchor = &template[col];
                if (left - glyphs[anchor[0]].x).abs() > em
                    && (right - glyphs[*anchor.last().unwrap()].end).abs() > em
                {
                    return false;
                }
                occupied[col] = true;
            }
            true
        };
        let has_header = aligned_header
            || start > 0
                && !used[rows[start - 1][0]]
                && counts[start - 1] * 2 < parts[start - 1].len()
                && parts[start - 1].len() == ncols
                && glyphs[rows[start - 1][0]].y - glyphs[rows[start][0]].y <= em * 3.
                && fits(start - 1);
        let mut end = start + 1;
        while end < rows.len()
            && !used[rows[end][0]]
            && counts[end] >= 2
            && fits(end)
            && glyphs[rows[end - 1][0]].y - glyphs[rows[end][0]].y <= em * 2.8
        {
            ctx.node()?;
            end += 1;
        }
        if end - start < if has_header { 3 } else { 4 }
            || counts[start..end].iter().filter(|&&n| n >= 3).count() < 3
        {
            start += 1;
            continue;
        }
        let selected = &rows[start - usize::from(has_header)..end];
        if selected
            .len()
            .checked_mul(ncols)
            .is_none_or(|cells| cells > ctx.options.limits.max_table_cells)
        {
            return Err(Error::Limit("max_table_cells"));
        }
        let mut values = Vec::new();
        for row in selected {
            let mut cells = vec![Vec::new(); ncols];
            for &i in row {
                cells[cuts.partition_point(|&x| x < glyphs[i].x)].push(glyphs[i].clone());
            }
            let mut text = Vec::new();
            for (c, cell) in cells.into_iter().enumerate() {
                ctx.cell()?;
                text.push(Cell::plain(layout::text(cell, ctx)?, c));
            }
            values.push(text);
        }
        let all: Vec<_> = selected.iter().flatten().copied().collect();
        tables.push(Table {
            left: all
                .iter()
                .map(|&i| glyphs[i].x)
                .fold(f64::INFINITY, f64::min),
            right: all
                .iter()
                .map(|&i| glyphs[i].end)
                .fold(f64::NEG_INFINITY, f64::max),
            top: glyphs[selected[0][0]].y + em,
            bottom: glyphs[selected.last().unwrap()[0]].y - em * 0.3,
            em,
            rows: values,
        });
        for i in all {
            used[i] = true;
        }
        start = end + 1;
    }
    tables.extend(super::field_table::extract(
        glyphs, &rows, &parts, used, ctx,
    )?);
    Ok(tables)
}

pub(super) fn extract(
    glyphs: &[Glyph],
    rules: Vec<Rule>,
    ctx: &mut Context<'_>,
) -> Result<(Vec<Table>, Vec<bool>), Error> {
    let rules = merge_rules(rules, ctx)?;
    let mut used = vec![false; glyphs.len()];
    let mut released = vec![false; glyphs.len()];
    let mut tables = super::hierarchical::extract(glyphs, &rules, &mut used, ctx)?;
    tables.extend(super::prose_table::extract(glyphs, &rules, &mut used, ctx)?);
    tables.extend(super::grid::open_sides(
        glyphs,
        &rules,
        &mut used,
        &mut released,
        ctx,
    )?);
    tables.extend(super::grid::extract(
        glyphs,
        &rules,
        &mut used,
        &mut released,
        ctx,
    )?);
    let joined = merge_rules(
        rules
            .iter()
            .map(|r| Rule {
                background: false,
                ..*r
            })
            .collect(),
        ctx,
    )?;
    tables.extend(super::grid::extract(
        glyphs,
        &joined,
        &mut used,
        &mut released,
        ctx,
    )?);
    super::grid::join_adjacent(&mut tables, &joined);
    tables.extend(super::aligned_table::extract(
        glyphs, &joined, &mut used, ctx,
    )?);
    tables.extend(super::sparse::extract(glyphs, &joined, &mut used, ctx)?);
    let vertical: Vec<_> = rules.iter().copied().filter(|r| r.vertical).collect();
    let mut rules: Vec<_> = rules.into_iter().filter(|r| !r.vertical).collect();
    rules.sort_by(|a, b| {
        a.left
            .total_cmp(&b.left)
            .then(a.right.total_cmp(&b.right))
            .then(b.y.total_cmp(&a.y))
    });
    let mut by_y: Vec<_> = (0..glyphs.len()).collect();
    by_y.sort_by(|&a, &b| glyphs[b].y.total_cmp(&glyphs[a].y));
    let mut start = 0;
    while start < rules.len() {
        ctx.node()?;
        let first = rules[start];
        let mut end = start + 1;
        while end < rules.len()
            && (rules[end].left - first.left).abs() < 1.
            && (rules[end].right - first.right).abs() < 1.
        {
            end += 1;
        }
        let mut ys: Vec<_> = rules[start..end].iter().map(|r| r.y).collect();
        start = end;
        ys.sort_by(|a, b| b.total_cmp(a));
        ys.dedup_by(|a, b| (*a - *b).abs() < 1.);
        if ys.len() < 3 {
            continue;
        }
        // Separate vertically distant tables sharing the same rule extents.
        let mut run = 0;
        for stop in 1..=ys.len() {
            let numeric_bridge = if stop < ys.len() && ys[stop - 1] - ys[stop] > 72. {
                let from = by_y.partition_point(|&i| glyphs[i].y >= ys[stop - 1]);
                let to = by_y.partition_point(|&i| glyphs[i].y > ys[stop]);
                let mut ids = Vec::new();
                for &i in &by_y[from..to] {
                    ctx.node()?;
                    let g = &glyphs[i];
                    if g.rotation == 0
                        && !g.text.trim().is_empty()
                        && g.x >= first.left
                        && g.end <= first.right
                        && g.y < ys[stop - 1]
                        && g.y > ys[stop]
                    {
                        ids.push(i);
                    }
                }
                numeric_lines(glyphs, &ids, 3).len() >= 3
            } else {
                false
            };
            let mut connected = false;
            if stop < ys.len() && ys[stop - 1] - ys[stop] > 72. && !numeric_bridge {
                for v in &vertical {
                    ctx.node()?;
                    if v.y >= first.left - 1.
                        && v.y <= first.right + 1.
                        && v.right >= ys[stop - 1] - 1.
                        && v.left <= ys[stop] + 1.
                    {
                        connected = true;
                        break;
                    }
                }
            }
            if stop < ys.len() && (ys[stop - 1] - ys[stop] <= 72. || numeric_bridge || connected) {
                continue;
            }
            let mut bands = ys[run..stop].to_vec();
            run = stop;
            if bands.len() < 3 {
                continue;
            }
            let mut ids = Vec::new();
            let mut claimed = Vec::new();
            let mut clipped = false;
            let from = by_y.partition_point(|&i| glyphs[i].y >= bands[0]);
            let to = by_y.partition_point(|&i| glyphs[i].y > *bands.last().unwrap());
            for &i in &by_y[from..to] {
                let g = &glyphs[i];
                ctx.node()?;
                if g.rotation == 0
                    && !g.text.trim().is_empty()
                    && ((g.x < first.left - 0.5 && g.end > first.left + 0.5)
                        || (g.x < first.right - 0.5 && g.end > first.right + 0.5))
                {
                    clipped = true;
                }
                if g.rotation == 0
                    && g.x >= first.left - 0.5
                    && g.end <= first.right + 0.5
                    && g.y < bands[0]
                    && g.y > *bands.last().unwrap()
                {
                    claimed.push(i);
                    if !g.text.trim().is_empty() {
                        ids.push(i);
                    }
                }
            }
            if clipped || ids.is_empty() || ids.iter().any(|&i| used[i]) {
                continue;
            }
            // A rule endpoint in a word gap must not clip a word whose glyphs
            // happen to fall wholly to either side of that endpoint.
            for boundary in [first.left, first.right] {
                let nearby: Vec<_> = by_y[from..to]
                    .iter()
                    .map(|&i| &glyphs[i])
                    .filter(|g| {
                        g.rotation == 0
                            && !g.text.trim().is_empty()
                            && g.x < boundary + g.em
                            && g.end > boundary - g.em
                    })
                    .collect();
                for a in &nearby {
                    if a.end > boundary + 0.5 {
                        continue;
                    }
                    for b in &nearby {
                        ctx.node()?;
                        if b.x >= boundary - 0.5
                            && (a.y - b.y).abs() < a.em.min(b.em) * 0.3
                            && b.x >= a.end - 0.5
                            && b.x - a.end < a.em.min(b.em) * 0.15
                        {
                            clipped = true;
                        }
                    }
                }
            }
            if clipped {
                continue;
            }
            let mut sizes: Vec<_> = ids.iter().map(|&i| glyphs[i].em).collect();
            sizes.sort_by(f64::total_cmp);
            let em = sizes[sizes.len() / 2];
            let mut vertical_cuts = Vec::new();
            for v in &vertical {
                ctx.node()?;
                if v.y > first.left + 1.
                    && v.y < first.right - 1.
                    && v.right >= bands[0] - 1.
                    && v.left <= *bands.last().unwrap() + 1.
                {
                    vertical_cuts.push(v.y);
                }
            }
            if vertical_cuts.is_empty()
                && numeric_lines(glyphs, &ids, 3).len() < 3
                && bands
                    .windows(2)
                    .any(|w| w[0] - w[1] > em * 6. || w[0] - w[1] < em * 0.8)
            {
                continue;
            }
            // Global whitespace gutters must survive every row (including wrapped text).
            let mut intervals: Vec<_> = ids.iter().map(|&i| (glyphs[i].x, glyphs[i].end)).collect();
            intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut right = intervals[0].1;
            let mut cuts = Vec::new();
            for (left, last) in intervals.into_iter().skip(1) {
                if left - right > (em * 0.6).max(2.5) {
                    // A painted edge inside the same gutter already defines it.
                    cuts.push(
                        vertical_cuts
                            .iter()
                            .copied()
                            .find(|&x| x >= right && x <= left)
                            .unwrap_or((left + right) / 2.),
                    );
                }
                right = right.max(last);
            }
            let explicit_columns = !vertical_cuts.is_empty();
            if explicit_columns {
                // Painted dividers can separate groups while whitespace divides
                // numeric subcolumns. Require both sides of an inferred gutter
                // to contain numbers on at least three shared baselines within
                // the same painted group, not labels in adjacent panels.
                let text_rows = lines(glyphs, &ids);
                cuts.retain(|&cut| {
                    if vertical_cuts.iter().any(|&x| (x - cut).abs() < 1.) {
                        return true;
                    }
                    let lo = vertical_cuts
                        .iter()
                        .copied()
                        .filter(|&x| x < cut)
                        .fold(first.left, f64::max);
                    let hi = vertical_cuts
                        .iter()
                        .copied()
                        .filter(|&x| x > cut)
                        .fold(first.right, f64::min);
                    text_rows
                        .iter()
                        .filter(|row| {
                            let mut a = String::new();
                            let mut b = String::new();
                            for &i in row.iter() {
                                let g = &glyphs[i];
                                if g.x >= lo && g.end <= cut {
                                    a.push_str(&g.text);
                                }
                                if g.x >= cut && g.end <= hi {
                                    b.push_str(&g.text);
                                }
                            }
                            numeric(&a) && numeric(&b)
                        })
                        .count()
                        >= 3
                });
                cuts.extend(vertical_cuts);
                cuts.sort_by(f64::total_cmp);
                cuts.dedup_by(|a, b| (*a - *b).abs() < 1.);
            }
            if cuts.is_empty() {
                continue;
            }
            // A table can have just a header separator and a bottom rule.
            // Split a large band only when several independently aligned,
            // mostly numeric rows establish the missing horizontal boundaries.
            let mut extra = Vec::new();
            let numeric_rows = numeric_lines(glyphs, &ids, 2);
            for band in bands.windows(2) {
                ctx.node()?;
                let from = numeric_rows.partition_point(|&y| y >= band[0]);
                let to = numeric_rows.partition_point(|&y| y > band[1]);
                let anchors = &numeric_rows[from..to];
                if anchors.len() >= 3 {
                    for pair in anchors.windows(2) {
                        extra.push((pair[0] + pair[1] + em * 0.7) / 2.);
                    }
                }
            }
            bands.extend(extra);
            bands.sort_by(|a, b| b.total_cmp(a));
            let nrows = bands.len() - 1;
            let ncols = cuts.len() + 1;
            let cells = nrows
                .checked_mul(ncols)
                .ok_or(Error::Limit("max_table_cells"))?;
            if cells > ctx.options.limits.max_table_cells {
                return Err(Error::Limit("max_table_cells"));
            }
            let mut buckets = vec![vec![Vec::<Glyph>::new(); ncols]; nrows];
            for &i in &ids {
                let g = &glyphs[i];
                let row = bands.partition_point(|y| *y > g.y) - 1;
                let col = cuts.partition_point(|x| *x < g.x);
                buckets[row][col].push(g.clone());
            }
            // Require repeated evidence for every column and at least two populated rows.
            if (!explicit_columns
                && (0..ncols).any(|c| buckets.iter().filter(|r| !r[c].is_empty()).count() < 2))
                || buckets
                    .iter()
                    .filter(|r| r.iter().filter(|c| !c.is_empty()).count() >= 2)
                    .count()
                    < 2
            {
                continue;
            }
            let mut rows = Vec::new();
            for row in buckets {
                let mut cells = Vec::new();
                for (c, glyphs) in row.into_iter().enumerate() {
                    ctx.cell()?;
                    cells.push(Cell::plain(layout::text(glyphs, ctx)?, c));
                }
                rows.push(cells);
            }
            // Claim whitespace too, preventing duplicate stray paragraphs.
            for i in claimed {
                used[i] = true;
            }
            tables.push(Table {
                left: first.left,
                right: first.right,
                top: bands[0],
                bottom: *bands.last().unwrap(),
                em,
                rows,
            });
        }
    }
    tables.extend(borderless(glyphs, &mut used, ctx)?);
    // Captions and notes are excluded from all table detectors, then released
    // for ordinary text emission so that their contents remain available.
    for (claimed, prose) in used.iter_mut().zip(released) {
        if prose {
            *claimed = false;
        }
    }
    Ok((tables, used))
}
