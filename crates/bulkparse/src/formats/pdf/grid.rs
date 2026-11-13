//! Closed painted grids: missing internal edges establish rectangular spans.
use super::{
    layout,
    positioned::Glyph,
    rules::Rule,
    tables::{Cell, Table},
};
use crate::{Context, Error};

fn covered(rules: &[Rule], axis: f64, lo: f64, hi: f64) -> bool {
    rules
        .iter()
        .any(|r| (r.y - axis).abs() <= 1. && r.left <= lo + 1. && r.right >= hi - 1.)
}
// Small mitres/padding can leave a short border overhang into a merged cell.
// Accept only almost complete or almost absent edges; reject intermediate gaps.
fn boundary(rules: &[Rule], axis: f64, lo: f64, hi: f64) -> Option<bool> {
    if covered(rules, axis, lo, hi) {
        return Some(true);
    }
    let overlap = rules
        .iter()
        .filter(|e| (e.y - axis).abs() <= 1.)
        .map(|e| (e.right.min(hi) - e.left.max(lo)).max(0.))
        .fold(0., f64::max);
    if overlap <= (hi - lo) * 0.2 {
        Some(false)
    } else if overlap >= (hi - lo) * 0.8 {
        Some(true)
    } else {
        None
    }
}
fn coordinates(values: &mut Vec<f64>) {
    values.sort_by(f64::total_cmp);
    values.dedup_by(|a, b| (*a - *b).abs() <= 1.);
}

// A pair of nearby strokes around an empty sliver is one decorated boundary,
// not an empty logical row/column. Bound the entire cluster (no transitive
// chaining), scale by local text size, and never merge across native ink.
fn collapse_border_pairs(
    values: &mut Vec<f64>,
    glyphs: &[Glyph],
    ids: &[usize],
    vertical: bool,
    em: f64,
) -> Vec<(f64, f64)> {
    let limit = (em * 0.25).min(3.);
    let mut pairs = Vec::new();
    let mut out = Vec::new();
    let mut start = 0;
    while start < values.len() {
        let mut end = start + 1;
        while end < values.len() && values[end] - values[start] <= limit {
            let has_ink = ids.iter().any(|&i| {
                let g = &glyphs[i];
                !g.text.trim().is_empty()
                    && if vertical {
                        g.x < values[end] && g.end > values[start]
                    } else {
                        g.y > values[start] && g.y < values[end]
                    }
            });
            if has_ink {
                break;
            }
            end += 1;
        }
        if end > start + 1 {
            pairs.push((values[start], values[end - 1]));
        }
        out.push((values[start] + values[end - 1]) / 2.);
        start = end;
    }
    *values = out;
    pairs
}

fn snap(value: f64, pairs: &[(f64, f64)]) -> f64 {
    pairs
        .iter()
        .find(|&&(a, b)| value >= a && value <= b)
        .map_or(value, |&(a, b)| (a + b) / 2.)
}

fn caption_label(word: &str) -> bool {
    matches!(word, "table" | "figure" | "exhibit" | "chart")
}

fn boxed_caption(text: &str) -> bool {
    let lower = text.to_lowercase();
    let mut words = lower.split_whitespace();
    caption_label(words.next().unwrap_or(""))
        && words.next().is_some_and(|word| {
            let number = word.trim_matches(['.', ':', '(', ')']);
            number.starts_with(|c: char| c.is_ascii_digit())
                || number.split_once('.').is_some_and(|(prefix, suffix)| {
                    prefix.len() == 1
                        && prefix.as_bytes()[0].is_ascii_alphabetic()
                        && suffix.starts_with(|c: char| c.is_ascii_digit())
                })
                || (!number.is_empty() && number.chars().all(|c| "ivxlcdm".contains(c)))
        })
}

fn boxed_note(text: &str) -> bool {
    let lower = text.trim_start().to_lowercase();
    if ["note:", "notes:", "source:", "sources:"]
        .iter()
        .any(|p| lower.starts_with(p))
    {
        return true;
    }
    let mut words = lower.split_whitespace();
    caption_label(words.next().unwrap_or(""))
        && matches!(words.next(), Some("reads:" | "note:" | "notes:"))
}

// A paragraph table may omit its perimeter while retaining internal dividers.
// Recover only a repeated grid: at least three full-width horizontal rules,
// two long internal dividers, and populated cells in every body row. The ends
// of the dividers establish the last row's extent, not the amount of prose.
pub(super) fn open_sides(
    glyphs: &[Glyph],
    rules: &[Rule],
    used: &mut [bool],
    released: &mut [bool],
    ctx: &mut Context<'_>,
) -> Result<Vec<Table>, Error> {
    let mut out = Vec::new();
    for top in rules.iter().filter(|r| !r.vertical && !r.background) {
        ctx.node()?;
        let mut hs: Vec<_> = rules
            .iter()
            .copied()
            .filter(|r| {
                !r.vertical
                    && (r.left - top.left).abs() <= 1.5
                    && (r.right - top.right).abs() <= 1.5
                    && r.y <= top.y + 1.
            })
            .collect();
        hs.sort_by(|a, b| b.y.total_cmp(&a.y));
        hs.dedup_by(|a, b| (a.y - b.y).abs() < 1.);
        if hs.len() < 3 {
            continue;
        }
        let last = hs.last().unwrap().y;
        let mut vs: Vec<_> = rules
            .iter()
            .copied()
            .filter(|r| {
                r.vertical
                    && r.y > top.left + 1.
                    && r.y < top.right - 1.
                    && r.right >= top.y - 2.
                    && r.left < last - 8.
            })
            .collect();
        vs.sort_by(|a, b| a.y.total_cmp(&b.y));
        vs.dedup_by(|a, b| (a.y - b.y).abs() < 1.);
        if vs.len() < 2 {
            continue;
        }
        let bottom = vs.iter().map(|v| v.left).fold(f64::INFINITY, f64::min);
        // Existing perimeter grids belong to the closed-grid detector.
        if rules.iter().any(|v| {
            v.vertical
                && ((v.y - top.left).abs() < 2. || (v.y - top.right).abs() < 2.)
                && v.right >= top.y - 2.
                && v.left <= last + 1.
        }) {
            continue;
        }
        let ids: Vec<_> = glyphs
            .iter()
            .enumerate()
            .filter(|(_, g)| {
                g.rotation == 0
                    && g.y < top.y
                    && g.y > bottom
                    && g.x >= top.left
                    && g.end <= top.right
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
        if vs.iter().any(|v| v.left - bottom > em * 3.)
            || last - bottom > em * 24.
            || hs.windows(2).any(|w| w[0].y - w[1].y < em * 2.)
        {
            continue;
        }
        // Internal boundaries must be free of text, even in their unpainted tails.
        if vs.iter().any(|v| {
            ids.iter()
                .any(|&i| glyphs[i].x < v.y - 0.5 && glyphs[i].end > v.y + 0.5)
        }) {
            continue;
        }
        let mut ys: Vec<_> = hs.iter().map(|h| h.y).collect();
        ys.push(bottom);
        let mut xs = vec![top.left];
        xs.extend(vs.iter().map(|v| v.y));
        xs.push(top.right);
        let mut populated = vec![vec![false; xs.len() - 1]; ys.len() - 1];
        for &i in &ids {
            let g = &glyphs[i];
            let r = ys.partition_point(|&y| y > g.y) - 1;
            let c = xs
                .partition_point(|&x| x < g.x)
                .saturating_sub(1)
                .min(xs.len() - 2);
            populated[r][c] = true;
        }
        if populated.iter().any(|r| r.iter().any(|v| !*v)) {
            continue;
        }
        for h in &mut hs {
            h.left = top.left;
            h.right = top.right;
            h.background = false;
        }
        hs.push(Rule {
            left: top.left,
            right: top.right,
            y: bottom,
            vertical: false,
            background: false,
        });
        for v in &mut vs {
            v.left = bottom;
            v.right = top.y;
            v.background = false;
        }
        for x in [top.left, top.right] {
            vs.push(Rule {
                left: bottom,
                right: top.y,
                y: x,
                vertical: true,
                background: false,
            });
        }
        // A short aligned header can sit immediately above an open table.
        let header: Vec<_> = glyphs
            .iter()
            .enumerate()
            .filter(|(i, g)| {
                !used[*i]
                    && g.rotation == 0
                    && g.y > top.y
                    && g.y < top.y + em * 2.5
                    && g.x >= top.left
                    && g.end <= top.right
                    && !g.text.trim().is_empty()
            })
            .map(|(i, _)| i)
            .collect();
        let header_lines = super::tables::lines(glyphs, &header);
        if header_lines.len() == 1 {
            let mut cells = vec![false; xs.len() - 1];
            let mut valid = true;
            for &i in &header {
                let g = &glyphs[i];
                let c = xs
                    .partition_point(|&x| x < g.x)
                    .saturating_sub(1)
                    .min(xs.len() - 2);
                if g.end > xs[c + 1] + 0.5 {
                    valid = false;
                }
                cells[c] = true;
            }
            if valid && cells.iter().filter(|v| **v).count() >= xs.len() - 2 {
                let new_top = header
                    .iter()
                    .map(|&i| glyphs[i].y + glyphs[i].em)
                    .fold(top.y, f64::max);
                hs.push(Rule {
                    left: top.left,
                    right: top.right,
                    y: new_top,
                    vertical: false,
                    background: false,
                });
                for v in &mut vs {
                    v.right = new_top;
                }
            }
        }
        let upper = hs.iter().map(|h| h.y).fold(top.y, f64::max);
        if let Some(table) = candidate(
            glyphs, &hs, &vs, used, released, top.left, top.right, bottom, upper, ctx,
        )? {
            out.push(table);
        }
    }
    Ok(out)
}

pub(super) fn extract(
    glyphs: &[Glyph],
    rules: &[Rule],
    used: &mut [bool],
    released: &mut [bool],
    ctx: &mut Context<'_>,
) -> Result<Vec<Table>, Error> {
    let ink: Vec<_> = glyphs
        .iter()
        .filter(|g| g.rotation == 0 && !g.text.trim().is_empty())
        .collect();
    if !ink.first().is_some_and(|first| {
        ink.iter()
            .any(|g| (g.y - first.y).abs() > g.em.min(first.em) * 0.5)
    }) {
        return Ok(Vec::new());
    }
    let mut horizontal: Vec<_> = rules
        .iter()
        .copied()
        .filter(|r| !r.vertical && !r.background)
        .collect();
    let vertical: Vec<_> = rules
        .iter()
        .copied()
        .filter(|r| r.vertical && !r.background)
        .collect();
    if vertical.is_empty() {
        return Ok(Vec::new());
    }
    horizontal.sort_by(|a, b| b.y.total_cmp(&a.y));
    let mut tables: Vec<Table> = Vec::new();
    for top in &horizontal {
        ctx.node()?;
        if tables.iter().any(|t| {
            top.y <= t.top + 1.
                && top.y >= t.bottom - 1.
                && top.left >= t.left - 1.
                && top.right <= t.right + 1.
        }) {
            continue;
        }
        let bottoms: Vec<_> = horizontal
            .iter()
            .filter(|b| {
                b.y < top.y - 2.
                    && (b.left - top.left).abs() <= 1.
                    && (b.right - top.right).abs() <= 1.
            })
            .collect();
        for bottom in bottoms.into_iter().rev() {
            ctx.node()?;
            let has_left = covered(&vertical, top.left, bottom.y, top.y);
            let has_right = covered(&vertical, top.right, bottom.y, top.y);
            let mut enclosing = vertical.clone();
            if !has_left || !has_right {
                // A clipped/open outer side is recoverable when the opposite
                // side and several internal dividers establish the grid.
                let internal = vertical
                    .iter()
                    .filter(|v| {
                        v.y > top.left + 1.
                            && v.y < top.right - 1.
                            && v.left <= bottom.y + 1.
                            && v.right >= bottom.y + (top.y - bottom.y) * 0.5
                    })
                    .count();
                if has_left == has_right || internal < 2 {
                    continue;
                }
                enclosing.push(Rule {
                    left: bottom.y,
                    right: top.y,
                    y: if has_left { top.right } else { top.left },
                    vertical: true,
                    background: false,
                });
            }
            if let Some(table) = candidate(
                glyphs,
                &horizontal,
                &enclosing,
                used,
                released,
                top.left,
                top.right,
                bottom.y,
                top.y,
                ctx,
            )? {
                tables.push(table);
                break;
            }
        }
    }
    Ok(tables)
}

#[allow(clippy::too_many_arguments)]
fn candidate(
    glyphs: &[Glyph],
    horizontal: &[Rule],
    vertical: &[Rule],
    used: &mut [bool],
    released: &mut [bool],
    left: f64,
    right: f64,
    bottom: f64,
    top: f64,
    ctx: &mut Context<'_>,
) -> Result<Option<Table>, Error> {
    let ids: Vec<_> = glyphs
        .iter()
        .enumerate()
        .filter(|(_, g)| {
            g.rotation == 0
                && g.y > bottom
                && g.y < top
                && g.x >= left - 0.5
                && g.end <= right + 0.5
        })
        .map(|(i, _)| i)
        .collect();
    if ids.iter().any(|&i| used[i]) || !ids.iter().any(|&i| !glyphs[i].text.trim().is_empty()) {
        return Ok(None);
    }
    if super::tables::clips_text(glyphs, left, right, bottom, top, ctx)? {
        return Ok(None);
    }
    let mut sizes: Vec<_> = ids
        .iter()
        .filter(|&&i| !glyphs[i].text.trim().is_empty())
        .map(|&i| glyphs[i].em)
        .collect();
    sizes.sort_by(f64::total_cmp);
    let em = sizes[sizes.len() / 2];
    // Only junction-to-junction separators define rows. Underlined text and
    // inset background edges inside a cell are not grid boundaries.
    let junction = |x: f64, y: f64| {
        vertical
            .iter()
            .any(|v| (v.y - x).abs() <= 1. && v.left <= y + 1. && v.right >= y - 1.)
    };
    let mut ys = vec![bottom, top];
    ys.extend(
        horizontal
            .iter()
            .filter(|r| {
                r.y > bottom + 1.
                    && r.y < top - 1.
                    && r.left >= left - 1.
                    && r.right <= right + 1.
                    && junction(r.left, r.y)
                    && junction(r.right, r.y)
            })
            .map(|r| r.y),
    );
    coordinates(&mut ys);
    let y_pairs = collapse_border_pairs(&mut ys, glyphs, &ids, false, em);
    ys.reverse();
    let mut xs = vec![left, right];
    xs.extend(
        vertical
            .iter()
            .filter(|r| {
                r.y > left + 1.
                    && r.y < right - 1.
                    && ys
                        .windows(2)
                        .any(|band| r.left <= band[1] + 1. && r.right >= band[0] - 1.)
            })
            .map(|r| r.y),
    );
    coordinates(&mut xs);
    let x_pairs = collapse_border_pairs(&mut xs, glyphs, &ids, true, em);
    // Move the corresponding painted edges and junctions to the same axes;
    // collapsing coordinate slots alone leaves phantom columns at thick frames.
    let hs_rules: Vec<_> = horizontal
        .iter()
        .map(|r| Rule {
            y: snap(r.y, &y_pairs),
            left: snap(r.left, &x_pairs),
            right: snap(r.right, &x_pairs),
            ..*r
        })
        .collect();
    let vs_rules: Vec<_> = vertical
        .iter()
        .map(|r| Rule {
            y: snap(r.y, &x_pairs),
            left: snap(r.left, &y_pairs),
            right: snap(r.right, &y_pairs),
            ..*r
        })
        .collect();
    let horizontal = hs_rules.as_slice();
    let vertical = vs_rules.as_slice();
    let nr = ys.len() - 1;
    let nc = xs.len() - 1;
    if nr < 2 {
        return Ok(None);
    }
    let count = nr.checked_mul(nc).ok_or(Error::Limit("max_table_cells"))?;
    if count > ctx.options.limits.max_table_cells {
        return Err(Error::Limit("max_table_cells"));
    }
    let mut hs = vec![vec![false; nc]; nr + 1];
    let mut vs = vec![vec![false; nc + 1]; nr];
    for r in 0..=nr {
        for c in 0..nc {
            ctx.node()?;
            let Some(present) = boundary(horizontal, ys[r], xs[c], xs[c + 1]) else {
                return Ok(None);
            };
            hs[r][c] = present;
        }
    }
    for r in 0..nr {
        for c in 0..=nc {
            ctx.node()?;
            let Some(present) = boundary(vertical, xs[c], ys[r + 1], ys[r]) else {
                return Ok(None);
            };
            vs[r][c] = present;
        }
    }
    let painted_vs = vs.clone();
    // Headers can share one painted background while containing distinct,
    // independently aligned labels. Repeated same-baseline chunks establish
    // the omitted divider; a single spanning title does not.
    let text_lines = super::tables::lines(
        glyphs,
        &ids.iter()
            .copied()
            .filter(|&i| !glyphs[i].text.trim().is_empty())
            .collect::<Vec<_>>(),
    );
    for r in 0..nr {
        for c in 1..nc {
            // Only subdivide a horizontally enclosed band. Filling a missing
            // vertical edge in an L-shaped region would manufacture a rowspan.
            if vs[r][c] || !hs[r][c - 1] || !hs[r][c] || !hs[r + 1][c - 1] || !hs[r + 1][c] {
                continue;
            }
            for line in &text_lines {
                let y = glyphs[line[0]].y;
                if y >= ys[r] || y <= ys[r + 1] {
                    continue;
                }
                let chunks = super::tables::chunks(glyphs, line);
                let left = chunks.iter().any(|part| {
                    let a = &glyphs[part[0]];
                    let b = &glyphs[*part.last().unwrap()];
                    a.x >= xs[c - 1] - 0.5
                        && b.end < xs[c] + a.em * 0.5
                        && (a.x + b.end) / 2. < xs[c]
                });
                let right = chunks.iter().any(|part| {
                    let a = &glyphs[part[0]];
                    let b = &glyphs[*part.last().unwrap()];
                    a.x > xs[c] - a.em * 0.5
                        && b.end <= xs[c + 1] + 0.5
                        && (a.x + b.end) / 2. > xs[c]
                });
                if left && right {
                    vs[r][c] = true;
                    break;
                }
            }
        }
    }
    if hs[0].iter().chain(&hs[nr]).any(|v| !v) || vs.iter().any(|r| !r[0] || !r[nc]) {
        return Ok(None);
    }
    let mut owner = vec![usize::MAX; count];
    let mut regions = Vec::new();
    for id in 0..count {
        if owner[id] != usize::MAX {
            continue;
        }
        let index = regions.len();
        owner[id] = index;
        let mut stack = vec![id];
        let mut members = Vec::new();
        while let Some(i) = stack.pop() {
            ctx.node()?;
            members.push(i);
            let r = i / nc;
            let c = i % nc;
            let neighbors = [
                (r > 0 && !hs[r][c], i.wrapping_sub(nc)),
                (r + 1 < nr && !hs[r + 1][c], i + nc),
                (c > 0 && !vs[r][c], i.wrapping_sub(1)),
                (c + 1 < nc && !vs[r][c + 1], i + 1),
            ];
            for (join, j) in neighbors {
                if join && owner[j] == usize::MAX {
                    owner[j] = index;
                    stack.push(j);
                }
            }
        }
        let r0 = members.iter().map(|i| i / nc).min().unwrap();
        let r1 = members.iter().map(|i| i / nc).max().unwrap() + 1;
        let c0 = members.iter().map(|i| i % nc).min().unwrap();
        let c1 = members.iter().map(|i| i % nc).max().unwrap() + 1;
        if members.len() != (r1 - r0) * (c1 - c0) {
            return Ok(None);
        }
        // A rectangle with an internal painted edge is an ambiguous/damaged
        // grid, not a valid merged cell.
        if (r0 + 1..r1).any(|r| (c0..c1).any(|c| hs[r][c]))
            || (r0..r1).any(|r| (c0 + 1..c1).any(|c| vs[r][c]))
        {
            return Ok(None);
        }
        regions.push((r0, r1, c0, c1));
    }
    if regions.len() < 3 {
        return Ok(None);
    }
    // A boxed multi-level heading above unruled numeric rows is not a
    // complete table. Leave it available to the borderless/table fallback
    // instead of claiming an isolated header as an extra table region.
    if nr <= 2
        && regions
            .iter()
            .any(|&(r0, r1, c0, c1)| r1 - r0 > 1 || c1 - c0 > 1)
    {
        let below: Vec<_> = glyphs
            .iter()
            .enumerate()
            .filter(|(i, g)| {
                !used[*i]
                    && g.rotation == 0
                    && !g.text.trim().is_empty()
                    && g.y < bottom
                    && g.y > bottom - em * 2.
                    && g.x >= left
                    && g.end <= right
            })
            .map(|(i, _)| i)
            .collect();
        let continued = super::tables::lines(glyphs, &below).iter().any(|line| {
            let mut columns = Vec::new();
            for part in super::tables::chunks(glyphs, line) {
                let text: String = part.iter().map(|&i| glyphs[i].text.as_str()).collect();
                if super::tables::numeric(&text) {
                    let a = &glyphs[part[0]];
                    let b = &glyphs[*part.last().unwrap()];
                    let c = xs
                        .partition_point(|x| *x <= (a.x + b.end) / 2.)
                        .saturating_sub(1)
                        .min(nc - 1);
                    if a.x >= xs[c] - em * 0.5 && b.end <= xs[c + 1] + em * 0.5 {
                        columns.push(c);
                    }
                }
            }
            columns.sort_unstable();
            columns.dedup();
            columns.len() >= 2
        });
        if continued {
            return Ok(None);
        }
    }
    let mut buckets = vec![Vec::new(); regions.len()];
    for line in &text_lines {
        for part in super::tables::chunks(glyphs, line) {
            let g = &glyphs[part[0]];
            let r = ys
                .partition_point(|y| *y > g.y)
                .saturating_sub(1)
                .min(nr - 1);
            let cell_for = |i: usize| {
                let g = &glyphs[i];
                let c = xs
                    .partition_point(|x| *x <= (g.x + g.end) / 2.)
                    .saturating_sub(1)
                    .min(nc - 1);
                (owner[r * nc + c], c)
            };
            // Preserve a whole word/chunk when it fits a cell within the
            // existing font-metric tolerance. Individual glyph centers near
            // an inferred divider must not cut the final letters off a word.
            let a = &glyphs[part[0]];
            let b = &glyphs[*part.last().unwrap()];
            let middle = (a.x + b.end) / 2.;
            let column = xs
                .partition_point(|x| *x <= middle)
                .saturating_sub(1)
                .min(nc - 1);
            let cell = owner[r * nc + column];
            let (_, _, c0, c1) = regions[cell];
            if a.x >= xs[c0] - a.em * 0.5 && b.end <= xs[c1] + b.em * 0.5 {
                buckets[cell].extend(part.iter().map(|&i| glyphs[i].clone()));
                continue;
            }
            let mut begin = 0;
            for stop in 1..=part.len() {
                let (cell, _) = cell_for(part[begin]);
                if stop < part.len() {
                    let (next, c) = cell_for(part[stop]);
                    if next == cell {
                        continue;
                    }
                    let a = &glyphs[part[stop - 1]];
                    let b = &glyphs[part[stop]];
                    // A painted divider can separate tightly padded cells.
                    // Never cut an unspaced word merely to fit a guessed grid.
                    if !painted_vs[r][c] || b.x - a.end < a.em.min(b.em) * 0.1 {
                        return Ok(None);
                    }
                }
                let (_, _, c0, c1) = regions[cell];
                let a = &glyphs[part[begin]];
                let b = &glyphs[part[stop - 1]];
                if a.x < xs[c0] - a.em * 0.5 || b.end > xs[c1] + b.em * 0.5 {
                    return Ok(None);
                }
                buckets[cell].extend(part[begin..stop].iter().map(|&i| glyphs[i].clone()));
                begin = stop;
            }
        }
    }
    // Native spaces carry the gaps of justified prose and list indentation.
    // Keep them inside their cell: removing them would make the reading-order
    // pass mistake separated words or bullet markers for independent columns.
    for &i in &ids {
        let g = &glyphs[i];
        if !g.text.trim().is_empty() {
            continue;
        }
        let r = ys
            .partition_point(|y| *y > g.y)
            .saturating_sub(1)
            .min(nr - 1);
        let c = xs
            .partition_point(|x| *x <= g.x)
            .saturating_sub(1)
            .min(nc - 1);
        buckets[owner[r * nc + c]].push(g.clone());
    }
    if buckets
        .iter()
        .filter(|b| b.iter().any(|g| !g.text.trim().is_empty()))
        .count()
        < 3
    {
        return Ok(None);
    }
    let mut rows: Vec<Vec<Cell>> = (0..nr).map(|_| Vec::new()).collect();
    for ((r0, r1, c0, c1), bucket) in regions.into_iter().zip(buckets) {
        if bucket.iter().all(|g| g.text.trim().is_empty()) && r1 - r0 > 1 {
            for row in &mut rows[r0..r1] {
                ctx.cell()?;
                row.push(Cell {
                    text: String::new(),
                    column: c0,
                    col_span: (c1 - c0) as u32,
                    row_span: 1,
                });
            }
            continue;
        }
        ctx.cell()?;
        rows[r0].push(Cell {
            text: layout::text(bucket, ctx)?,
            column: c0,
            col_span: (c1 - c0) as u32,
            row_span: (r1 - r0) as u32,
        });
    }
    for row in &mut rows {
        row.sort_by_key(|c| c.column);
    }
    // A full-width caption/note inside an enclosing frame belongs outside
    // the data grid. Keep its glyphs unclaimed for ordinary text emission.
    // Partial-width group headers and unlabeled spanning titles stay cells.
    let full_width = |row: &[Cell]| {
        row.len() == 1
            && row[0].column == 0
            && row[0].col_span as usize == nc
            && row[0].row_span == 1
    };
    // Released explanatory text may contain multiple lines, but must not be
    // detected again as a one-column table in a later extraction pass.
    if nc == 1
        && rows
            .first()
            .is_some_and(|row| row.first().is_some_and(|cell| boxed_note(&cell.text)))
    {
        return Ok(None);
    }
    let mut first = 0;
    let mut last = rows.len();
    if nc >= 2 && last >= 3 && full_width(&rows[0]) && boxed_caption(&rows[0][0].text) {
        first = 1;
    }
    if nc >= 2
        && last - first >= 3
        && full_width(&rows[last - 1])
        && boxed_note(&rows[last - 1][0].text)
    {
        last -= 1;
    }
    let top = ys[first];
    let bottom = ys[last];
    let rows = rows.drain(first..last).collect();
    for i in ids {
        used[i] = true;
        released[i] = glyphs[i].y <= bottom || glyphs[i].y >= top;
    }
    Ok(Some(Table {
        left,
        right,
        top,
        bottom,
        em,
        rows,
    }))
}

// A filled header band and stroked body can be found in different passes.
// Rejoin only touching grids with the same outer edges and column count;
// a painted full-width shared edge must support the join.
pub(super) fn join_adjacent(tables: &mut Vec<Table>, rules: &[Rule]) {
    tables.sort_by(|a, b| b.top.total_cmp(&a.top));
    let columns = |t: &Table| {
        t.rows
            .iter()
            .flatten()
            .map(|c| c.column + c.col_span as usize)
            .max()
            .unwrap_or(0)
    };
    let mut i = 0;
    while i < tables.len() {
        let join = (i + 1..tables.len()).find(|&j| {
            let a = &tables[i];
            let b = &tables[j];
            (a.bottom - b.top).abs() <= 1.
                && (a.left - b.left).abs() <= 1.
                && (a.right - b.right).abs() <= 1.
                && a.em.min(b.em) >= a.em.max(b.em) * 0.8
                && columns(a) >= 2
                && columns(a) == columns(b)
                && rules.iter().any(|r| {
                    !r.vertical
                        && (r.y - a.bottom).abs() <= 1.
                        && r.left <= a.left + 1.
                        && r.right >= a.right - 1.
                })
        });
        if let Some(j) = join {
            let b = tables.remove(j);
            tables[i].bottom = b.bottom;
            tables[i].rows.extend(b.rows);
        } else {
            i += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(top: f64, bottom: f64, nc: usize) -> Table {
        Table {
            left: 40.,
            right: 400.,
            top,
            bottom,
            em: 10.,
            rows: vec![(0..nc)
                .map(|c| Cell {
                    text: format!("Cell {c}"),
                    column: c,
                    col_span: 1,
                    row_span: 1,
                })
                .collect()],
        }
    }

    #[test]
    fn touching_header_and_body_join_only_with_matching_grid_evidence() {
        let rules = vec![Rule {
            left: 40.,
            right: 400.,
            y: 680.,
            vertical: false,
            background: true,
        }];
        let mut tables = vec![table(680., 600., 3), table(720., 680., 3)];
        join_adjacent(&mut tables, &rules);
        assert_eq!(tables.len(), 1);
        assert_eq!(
            (tables[0].top, tables[0].bottom, tables[0].rows.len()),
            (720., 600., 2)
        );
        for mut body in [
            table(675., 600., 3),
            table(680., 600., 2),
            table(680., 600., 3),
        ] {
            if body.top == 680. && body.rows[0].len() == 3 {
                body.left = 50.;
            }
            let mut tables = vec![table(720., 680., 3), body];
            join_adjacent(&mut tables, &rules);
            assert_eq!(tables.len(), 2);
        }
        let mut tables = vec![table(720., 680., 3), table(680., 600., 3)];
        join_adjacent(&mut tables, &[]);
        assert_eq!(tables.len(), 2);
    }
}
