//! Borderless label/value tables. Repeated baselines and shared gutters, rather
//! than paint order or document vocabulary, establish the rows and columns.
use super::{
    layout,
    positioned::Glyph,
    tables::{Cell, Table},
};
use crate::{Context, Error};
use std::ops::Range;

// Recognise a scalar with an explicit unit/currency, optionally qualified by a
// short abbreviation or followed by a parenthetical annotation. Bare numbers
// alone are deliberately insufficient evidence for a two-column table.
fn value(text: &str) -> bool {
    let text = text.split('(').next().unwrap_or(text).trim();
    let mut s = text;
    let mut explicit = false;
    let prefix = s.trim_start_matches(|c: char| c.is_alphabetic());
    let prefix_len = s[..s.len() - prefix.len()].chars().count();
    if prefix_len > 0 {
        if prefix_len <= 4 && prefix.starts_with('.') {
            s = prefix[1..].trim_start();
        } else if prefix_len == 3
            && s[..s.len() - prefix.len()]
                .chars()
                .all(|c| c.is_ascii_uppercase())
            && prefix.starts_with(char::is_whitespace)
        {
            explicit = true;
            s = prefix.trim_start();
        } else {
            return false;
        }
    }
    let currency = |c: char| {
        matches!(
            c,
            '$' | '€' | '£' | '¥' | '₹' | '₩' | '₽' | '₺' | '₫' | '₪' | '₴' | '₦' | '฿'
        )
    };
    let rest = s.trim_start_matches(currency);
    explicit |= rest.len() != s.len();
    s = rest.trim_start();
    let end = s
        .find(|c: char| !(c.is_numeric() || c.is_whitespace() || ".,+-−~±'’".contains(c)))
        .unwrap_or(s.len());
    if !s[..end].chars().any(char::is_numeric) {
        return false;
    }
    let suffix = s[end..].trim();
    if suffix.is_empty() {
        return explicit;
    }
    // One unit (possibly a rate), never an arbitrary sentence beginning in a
    // number. Unit names and document vocabulary are not enumerated.
    suffix.chars().count() <= 16
        && suffix
            .chars()
            .all(|c| c.is_alphabetic() || currency(c) || "/%°²³".contains(c))
}

struct Field {
    parts: Range<usize>,
    left: f64,
    right: f64,
    // Alignment uses the value itself, excluding its trailing annotation.
    anchor_right: f64,
}
struct Row {
    fields: Vec<Field>,
    y: f64,
    em: f64,
}

fn candidate(glyphs: &[Glyph], parts: &[Vec<usize>], scratch: &mut String) -> Option<Row> {
    if parts.len() < 2 {
        return None;
    }
    let mut fields: Vec<Field> = Vec::new();
    let mut field_start = 0;
    for (p, ids) in parts.iter().enumerate().skip(1) {
        scratch.clear();
        for (j, &i) in ids.iter().enumerate() {
            if j > 0 && glyphs[i].x - glyphs[ids[j - 1]].end > glyphs[i].em * 0.1 {
                scratch.push(' ');
            }
            scratch.push_str(&glyphs[i].text);
        }
        if value(scratch) {
            fields.push(field(glyphs, parts, field_start..p));
            field_start = p;
            if fields.len() == 12 {
                return None;
            }
        }
    }
    if fields.is_empty() {
        return None;
    }
    fields.push(field(glyphs, parts, field_start..parts.len()));
    let first = &glyphs[parts[0][0]];
    Some(Row {
        fields,
        y: first.y,
        em: first.em,
    })
}

fn field(glyphs: &[Glyph], parts: &[Vec<usize>], range: Range<usize>) -> Field {
    let first = &parts[range.start];
    let last = &parts[range.end - 1];
    let anchor_right = first
        .iter()
        .take_while(|&&i| !glyphs[i].text.contains('('))
        .map(|&i| glyphs[i].end)
        .fold(glyphs[first[0]].end, f64::max);
    Field {
        left: glyphs[first[0]].x,
        right: glyphs[*last.last().unwrap()].end,
        anchor_right,
        parts: range,
    }
}

fn aligned(a: &Row, b: &Row) -> bool {
    let em = a.em.min(b.em);
    a.fields.len() == b.fields.len()
        && (a.fields[0].left - b.fields[0].left).abs() <= em * 0.6
        && a.fields.iter().zip(&b.fields).skip(1).all(|(a, b)| {
            (a.left - b.left).abs() <= em * 0.6
                || (a.anchor_right - b.anchor_right).abs() <= em * 0.6
        })
}

pub(super) fn extract(
    glyphs: &[Glyph],
    rows: &[Vec<usize>],
    parts: &[Vec<Vec<usize>>],
    used: &mut [bool],
    ctx: &mut Context<'_>,
) -> Result<Vec<Table>, Error> {
    // Each row is analysed once. Candidates borrow ranges of existing chunks,
    // without copying glyphs or their indices. Runs advance monotonically, including rejected runs.
    let mut candidates = Vec::with_capacity(parts.len());
    let mut scratch = String::new();
    for p in parts {
        ctx.node()?;
        candidates.push(if p.iter().flatten().any(|&i| used[i]) {
            None
        } else {
            candidate(glyphs, p, &mut scratch)
        });
    }
    let mut out = Vec::new();
    let mut start = 0;
    while start < rows.len() {
        ctx.node()?;
        let Some(first) = &candidates[start] else {
            start += 1;
            continue;
        };
        let nc = first.fields.len();
        let mut left: Vec<_> = first.fields.iter().map(|f| f.left).collect();
        let mut right: Vec<_> = first.fields.iter().map(|f| f.right).collect();
        let mut end = start + 1;
        while end < rows.len() {
            ctx.node()?;
            let Some(next) = &candidates[end] else {
                break;
            };
            let previous = candidates[end - 1].as_ref().unwrap();
            if !aligned(first, next) || previous.y - next.y > first.em.max(next.em) * 2.8 {
                break;
            }
            // Every boundary must remain in actual whitespace across the run.
            if (1..nc).any(|c| {
                right[c - 1].max(next.fields[c - 1].right) + first.em * 0.5
                    > left[c].min(next.fields[c].left)
            }) {
                break;
            }
            for c in 0..nc {
                left[c] = left[c].min(next.fields[c].left);
                right[c] = right[c].max(next.fields[c].right);
            }
            end += 1;
        }
        if end - start < 2 {
            start = end;
            continue;
        }
        let mut selected_start = start;
        let mut header_cells = None;
        if start > 0
            && candidates[start - 1].is_none()
            && glyphs[rows[start - 1][0]].y - first.y <= first.em * 3.
            && !rows[start - 1].iter().any(|&i| used[i])
        {
            // Assign entire header fragments to columns. This permits empty
            // headings and wider labels without cutting through header words.
            let mut cells = vec![Vec::new(); nc];
            let mut valid = true;
            let mut previous_col = None;
            for part in &parts[start - 1] {
                let x = glyphs[part[0]].x;
                let edge = glyphs[*part.last().unwrap()].end;
                let distance = |c: usize| {
                    (left[c] - x)
                        .abs()
                        .min((right[c] - edge).abs())
                        .min(((left[c] + right[c] - x - edge) / 2.).abs())
                };
                let col = (0..nc)
                    .min_by(|&a, &b| distance(a).total_cmp(&distance(b)))
                    .unwrap();
                if distance(col) > first.em || previous_col.is_some_and(|c| col <= c) {
                    valid = false;
                    break;
                }
                previous_col = Some(col);
                cells[col].extend_from_slice(part);
            }
            if valid && cells.iter().filter(|c| !c.is_empty()).count() >= 2 {
                let mut hl = left.clone();
                let mut hr = right.clone();
                for (c, ids) in cells.iter().enumerate().filter(|(_, ids)| !ids.is_empty()) {
                    hl[c] = hl[c].min(glyphs[ids[0]].x);
                    hr[c] = hr[c].max(glyphs[*ids.last().unwrap()].end);
                }
                if (1..nc).all(|c| hr[c - 1] + first.em * 0.15 < hl[c]) {
                    selected_start -= 1;
                    left = hl;
                    right = hr;
                    header_cells = Some(cells);
                }
            }
        }
        let count = (end - selected_start)
            .checked_mul(nc)
            .ok_or(Error::Limit("max_table_cells"))?;
        if count > ctx.options.limits.max_table_cells {
            return Err(Error::Limit("max_table_cells"));
        }
        let mut values = Vec::with_capacity(end - selected_start);
        for r in selected_start..end {
            let mut row_values = Vec::with_capacity(nc);
            for c in 0..nc {
                ctx.cell()?;
                let chunks: &[Vec<usize>] = if r < start {
                    std::slice::from_ref(&header_cells.as_ref().unwrap()[c])
                } else {
                    &parts[r][candidates[r].as_ref().unwrap().fields[c].parts.clone()]
                };
                let text = layout::ordered_text(chunks.iter().flatten().map(|&i| &glyphs[i]));
                if text.len() > ctx.options.limits.max_text_bytes {
                    return Err(Error::Limit("max_text_bytes"));
                }
                row_values.push(Cell::plain(text, c));
                for &i in chunks.iter().flatten() {
                    used[i] = true;
                }
            }
            values.push(row_values);
        }
        out.push(Table {
            left: left[0],
            right: right[nc - 1],
            top: glyphs[rows[selected_start][0]].y + first.em,
            bottom: candidates[end - 1].as_ref().unwrap().y - first.em * 0.3,
            em: first.em,
            rows: values,
        });
        start = end;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::value;
    #[test]
    fn scalar_evidence_accepts_units_and_currency_but_not_numeric_prose() {
        for text in [
            "USD 12.50",
            "8 EUR",
            "12 kg",
            "25,5 m/s",
            "$19.95",
            "£8",
            "20%",
            "~350 bottles",
            "min. 40 cm",
            "42 kr (per person)",
        ] {
            assert!(value(text), "{text}");
        }
        for text in [
            "12",
            "2024 report is here",
            "12 people arrived",
            "Right column 2024",
            "(20 kg)",
            "",
            "USD",
            "abc. word",
        ] {
            assert!(!value(text), "{text}");
        }
    }
}
