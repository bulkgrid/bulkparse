//! Deterministic geometric line grouping and whitespace cuts. This is a native
//! heuristic, not a learned layout model or an inverse Unicode bidi algorithm.
use super::positioned::Glyph;
use crate::{Context, Error};
use unicode_bidi::{bidi_class, BidiClass};

fn direction(text: &str) -> Option<bool> {
    text.chars().find_map(|c| match bidi_class(c) {
        BidiClass::R | BidiClass::AL => Some(true),
        BidiClass::L | BidiClass::EN | BidiClass::AN => Some(false),
        _ => None,
    })
}
fn rtl<'a>(glyphs: impl Iterator<Item = &'a Glyph>) -> bool {
    let (mut r, mut l) = (0, 0);
    for g in glyphs {
        match direction(&g.text) {
            Some(true) => r += 1,
            Some(false) => l += 1,
            None => {}
        }
    }
    r > l
}
pub(super) fn ordered_text<'a>(glyphs: impl Iterator<Item = &'a Glyph> + Clone) -> String {
    let base = rtl(glyphs.clone());
    let mut units: Vec<(String, Option<bool>)> = Vec::new();
    let mut previous: Option<&Glyph> = None;
    for g in glyphs {
        if let Some(p) = previous {
            let gap = g.x - p.end;
            if gap > 0.10 * g.em.min(p.em).max(1.)
                && !p.text.ends_with(char::is_whitespace)
                && !g.text.starts_with(char::is_whitespace)
            {
                units.push((" ".into(), None));
            }
        }
        // A CMap glyph may map to a ligature or a base+combining-mark sequence.
        // Preserve that atomic sequence when ordering geometric runs.
        if g.text.chars().all(|c| bidi_class(c) == BidiClass::NSM) && !units.is_empty() {
            units.last_mut().unwrap().0.push_str(&g.text);
        } else {
            units.push((g.text.clone(), direction(&g.text)));
        }
        previous = Some(g);
    }
    let mut after = vec![None; units.len()];
    let mut next = None;
    for i in (0..units.len()).rev() {
        after[i] = next;
        next = units[i].1.or(next);
    }
    let mut before = None;
    for (i, (_, dir)) in units.iter_mut().enumerate() {
        if dir.is_none() {
            *dir = Some(if before == after[i] {
                before.unwrap_or(base)
            } else {
                base
            });
        } else {
            before = *dir;
        }
    }
    let mut runs: Vec<(bool, Vec<String>)> = Vec::new();
    for (text, dir) in units {
        let dir = dir.unwrap_or(base);
        if runs.last().is_none_or(|r| r.0 != dir) {
            runs.push((dir, Vec::new()));
        }
        runs.last_mut().unwrap().1.push(text);
    }
    for (dir, items) in &mut runs {
        if *dir {
            items.reverse();
        }
    }
    if base {
        runs.reverse();
    }
    runs.into_iter()
        .flat_map(|(_, items)| items)
        .collect::<String>()
        .trim()
        .to_owned()
}
#[derive(Debug)]
struct Line {
    text: String,
    x: f64,
    end: f64,
    y: f64,
    em: f64,
    rtl: bool,
    bottom: f64,
    table: Option<usize>,
    heading_em: f64,
    bold: bool,
    mixed_bold_lead: bool,
    complete_field: bool,
}
fn line(glyphs: &[Glyph]) -> Line {
    let start = glyphs
        .iter()
        .position(|g| !g.text.trim().is_empty())
        .unwrap_or(0);
    let end = glyphs
        .iter()
        .rposition(|g| !g.text.trim().is_empty())
        .map_or(glyphs.len(), |i| i + 1);
    let glyphs = &glyphs[start..end];
    let mut sizes: Vec<_> = glyphs
        .iter()
        .filter(|g| !g.text.trim().is_empty())
        .map(|g| g.em)
        .collect();
    sizes.sort_by(f64::total_cmp);
    let text = ordered_text(glyphs.iter());
    let em = glyphs.iter().map(|g| g.em).fold(0., f64::max);
    let mut heading_em = sizes.get(sizes.len() / 2).copied().unwrap_or(0.);
    // Small-cap fonts encode initial capitals slightly larger than the other
    // capitals. They provide heading evidence without accepting drop caps.
    if text
        .chars()
        .filter(|c| c.is_alphabetic())
        .all(|c| c.is_uppercase())
        && em <= heading_em * 1.4
    {
        heading_em = em;
    }
    // Outline numbers often use the body face next to an otherwise bold title.
    // Judge the alphabetic title run separately when explicit numbering exists.
    let title_glyphs = if heading_number(&text) == Some(HeadingNumber::Explicit) {
        &glyphs[glyphs
            .iter()
            .position(|g| g.text.chars().any(char::is_alphabetic))
            .unwrap_or(0)..]
    } else {
        glyphs
    };
    let title_ink = title_glyphs
        .iter()
        .filter(|g| !g.text.trim().is_empty())
        .count();
    let bold = title_glyphs
        .iter()
        .filter(|g| g.bold && !g.text.trim().is_empty())
        .count()
        * 5
        >= title_ink.max(1) * 4;
    Line {
        text,
        x: glyphs.iter().map(|g| g.x).fold(f64::INFINITY, f64::min),
        end: glyphs
            .iter()
            .map(|g| g.end)
            .fold(f64::NEG_INFINITY, f64::max),
        y: glyphs.iter().map(|g| g.y).sum::<f64>() / glyphs.len() as f64,
        em: glyphs.iter().map(|g| g.em).fold(0., f64::max),
        rtl: rtl(glyphs.iter()),
        bottom: glyphs.iter().map(|g| g.y).sum::<f64>() / glyphs.len() as f64,
        table: None,
        complete_field: false,
        // A single drop cap or oversized symbol must not promote the whole line.
        heading_em,
        bold,
        mixed_bold_lead: glyphs.first().is_some_and(|g| g.bold)
            && glyphs
                .iter()
                .take_while(|g| g.bold || g.text.trim().is_empty())
                .flat_map(|g| g.text.chars())
                .filter(|c| c.is_alphabetic())
                .count()
                >= 3
            && glyphs
                .iter()
                .filter(|g| !g.bold)
                .flat_map(|g| g.text.chars())
                .filter(|c| c.is_alphabetic())
                .count()
                >= 8,
    }
}

fn numeric(text: &str) -> bool {
    text.chars().any(|c| c.is_numeric())
        && text
            .chars()
            .all(|c| c.is_numeric() || c.is_whitespace() || ",.+-−()%$€£¥".contains(c))
}

// An independently numbered heading begins a new block even when its font and
// spacing look like a wrapped title. Recognize outline syntax, not title words.
// A bare number without a title can instead be a wrapped display year; bare
// Roman/letter words and alphanumeric words such as "3D" are not markers.
#[derive(Clone, Copy, PartialEq)]
enum HeadingNumber {
    Explicit,
    Bare,
}

fn heading_number(text: &str) -> Option<HeadingNumber> {
    let mut words = text.split_whitespace();
    let marker = words.next()?;
    let (label, delimited) =
        if let Some(inner) = marker.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
            (inner, true)
        } else if let Some(inner) = marker.strip_suffix(['.', ')', ':']) {
            (inner, true)
        } else {
            (marker, false)
        };
    let hierarchical = label.contains('.');
    if words.next().is_none() && !delimited && !hierarchical {
        return None;
    }
    let valid = label.split('.').enumerate().all(|(i, part)| {
        !part.is_empty()
            && (part.bytes().all(|c| c.is_ascii_digit())
                || (i == 0
                    && (delimited || hierarchical)
                    && ((part.len() == 1 && part.as_bytes()[0].is_ascii_alphabetic())
                        || part.chars().all(|c| "IVXLCDMivxlcdm".contains(c)))))
    });
    valid.then_some(if delimited || hierarchical {
        HeadingNumber::Explicit
    } else {
        HeadingNumber::Bare
    })
}

fn numbered_heading_boundary(previous: Option<HeadingNumber>, next: Option<HeadingNumber>) -> bool {
    // A plain leading number can be part of a wrapped title (a year, count,
    // measurement, etc.). Require outline evidence on the preceding heading
    // before treating that ambiguous syntax as a fresh section.
    next.is_some_and(|kind| kind == HeadingNumber::Explicit || previous.is_some())
}

fn caption_start(text: &str) -> bool {
    let lower = text.to_lowercase();
    ["table ", "figure ", "fig. "].iter().any(|prefix| {
        lower
            .strip_prefix(prefix)
            .is_some_and(|s| s.starts_with(char::is_numeric))
    }) || lower.starts_with("source:")
        || lower.starts_with("note:")
        || lower.starts_with("table:")
        || lower.starts_with("figure:")
}

fn bullet(text: &str) -> bool {
    matches!(
        text.trim(),
        "•" | "◦" | "▪" | "‣" | "⁃" | "●" | "○" | "■" | "–" | "-"
    )
}

fn letter_marker(text: &str) -> bool {
    let s = text.as_bytes();
    (s.len() == 2 && s[0].is_ascii_alphabetic() && matches!(s[1], b'.' | b')'))
        || (s.len() == 3 && s[0] == b'(' && s[1].is_ascii_alphabetic() && s[2] == b')')
}

// Continued contents pages may have no "Contents" title or dotted leaders.
// Require a dominant outline with hierarchical identifiers, aligned separate
// page references and a nondecreasing sequence. Ordinary numeric tables and
// numbered prose do not supply this combination of evidence.
fn join_contents_entries(lines: &mut Vec<Line>) -> bool {
    if lines.iter().any(|l| l.table.is_some()) {
        return false;
    }
    let right = lines
        .iter()
        .map(|l| l.end)
        .fold(f64::NEG_INFINITY, f64::max);
    let mut pairs = Vec::new();
    for (j, p) in lines.iter().enumerate() {
        if p.text.len() > 4 || !p.text.chars().all(|c| c.is_ascii_digit()) || p.end < right - p.em {
            continue;
        }
        let Ok(number) = p.text.parse::<u32>() else {
            continue;
        };
        if let Some((i, _)) = lines
            .iter()
            .enumerate()
            .filter(|(_, l)| {
                l.text.chars().any(char::is_alphabetic)
                    && l.text.chars().count() <= 120
                    && (l.y - p.y).abs() < p.em * 0.35
                    && (l.heading_em - p.heading_em).abs() < 0.5
                    && p.x - l.end > p.em * 2.
            })
            .max_by(|(_, a), (_, b)| a.end.total_cmp(&b.end))
        {
            pairs.push((i, j, number));
        }
    }
    if pairs.len() < 6 {
        return false;
    }
    pairs.sort_by(|a, b| lines[b.0].y.total_cmp(&lines[a.0].y));
    let structured = pairs
        .iter()
        .filter(|(i, _, _)| {
            lines[*i].text.split_whitespace().take(2).any(|t| {
                let parts: Vec<_> = t
                    .trim_matches(|c: char| !c.is_ascii_digit())
                    .split('.')
                    .collect();
                parts.len() >= 2
                    && parts
                        .iter()
                        .all(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))
            })
        })
        .count();
    let entry_chars: usize = pairs
        .iter()
        .map(|(i, _, _)| lines[*i].text.chars().count())
        .sum();
    let total_chars: usize = lines.iter().map(|l| l.text.chars().count()).sum();
    let distinct: std::collections::BTreeSet<_> = pairs.iter().map(|(_, _, n)| *n).collect();
    if structured < 6
        || structured * 5 < pairs.len() * 3
        || entry_chars * 2 < total_chars
        || distinct.len() < 6
        || pairs.windows(2).any(|w| w[0].2 > w[1].2)
    {
        return false;
    }
    let mut removed = vec![false; lines.len()];
    for (i, j, _) in pairs {
        let (text, end) = (lines[j].text.clone(), lines[j].end);
        lines[i].text.push(' ');
        lines[i].text.push_str(&text);
        lines[i].end = end;
        removed[j] = true;
    }
    let mut i = 0;
    lines.retain(|_| {
        let keep = !removed[i];
        i += 1;
        keep
    });
    true
}

// A short running title and a folio/course identifier share a page-edge row,
// not two content columns. Retain both as one plain-text line. Font, isolation
// and row-shape evidence protect large display titles and ordinary columns.
fn join_running_header(lines: &mut Vec<Line>) {
    if lines.len() <= 8 {
        return;
    }
    let (a, b, next) = (&lines[0], &lines[1], &lines[2]);
    let mut weights = std::collections::BTreeMap::<i64, usize>::new();
    for l in lines.iter() {
        *weights.entry((l.em * 4.).round() as i64).or_default() += l.text.chars().count();
    }
    let body = weights
        .iter()
        .max_by_key(|(_, n)| **n)
        .map_or(0., |(em, _)| *em as f64 / 4.);
    if (a.y - b.y).abs() < a.em.min(b.em) * 0.35
        && b.x - a.end > a.em.max(b.em) * 4.
        && a.y - next.y > a.em.max(b.em) * 1.8
        && a.em.max(b.em) <= body * 1.1
        && a.text.chars().count() + b.text.chars().count() <= 90
        && b.text.chars().count() <= 16
        && b.text.chars().any(char::is_numeric)
        && a.text
            .chars()
            .filter(|c| c.is_alphabetic())
            .all(char::is_uppercase)
    {
        let b = lines.remove(1);
        let a = &mut lines[0];
        a.text.push(' ');
        a.text.push_str(&b.text);
        a.end = b.end;
        a.heading_em = 0.;
    }
}

// Follow the nearest overlapping line, so interleaved columns cannot break a
// wrapped caption or make it leak into the neighbouring column. Style changes
// and paragraph-sized gaps end the run.
fn heading_exclusions(lines: &[Line], body: f64) -> Vec<bool> {
    let mut excluded: Vec<_> = lines.iter().map(|l| caption_start(&l.text)).collect();
    let next: Vec<_> = lines
        .iter()
        .map(|a| {
            lines
                .iter()
                .enumerate()
                .filter(|(_, b)| {
                    b.y < a.y - a.em * 0.5 && b.x < a.end && b.end > a.x && b.table.is_none()
                })
                .max_by(|(_, a), (_, b)| a.y.total_cmp(&b.y))
                .and_then(|(j, b)| {
                    (a.y - b.y < a.em.max(b.em) * 1.8
                        && (a.heading_em - b.heading_em).abs() < 0.5
                        && a.bold == b.bold)
                        .then_some(j)
                })
        })
        .collect();
    for i in 0..lines.len() {
        let marked = lines[i].heading_em <= body * 1.1
            && (lines[i].text.starts_with(['*', '†', '‡'])
                || lines[i].text.split_whitespace().next().is_some_and(bullet));
        if caption_start(&lines[i].text) || marked {
            excluded[i] = true;
            let mut current = i;
            while let Some(j) = next[current] {
                excluded[j] = true;
                current = j;
            }
        }
        // Long body-size styled paragraphs (including pull quotes) are not
        // headings merely because every line is bold. Short wrapped titles stay.
        if lines[i].bold && lines[i].heading_em <= body * 1.1 {
            let mut run = vec![i];
            let mut current = i;
            while let Some(j) = next[current] {
                run.push(j);
                current = j;
            }
            if run.len() >= 4
                && run
                    .iter()
                    .map(|&j| lines[j].text.chars().count())
                    .sum::<usize>()
                    > 180
            {
                for j in run {
                    excluded[j] = true;
                }
            }
        }
    }
    excluded
}

// Parallel short labels need nearby data evidence before being treated as
// chart/table labels. Three genuine section titles can also share a baseline.
fn data_labels(lines: &[Line]) -> Vec<bool> {
    let mut result = vec![false; lines.len()];
    let mut repeated = std::collections::BTreeSet::new();
    for (i, l) in lines.iter().enumerate() {
        if result[i] || l.table.is_some() || l.text.chars().count() > 24 {
            continue;
        }
        let mut row: Vec<_> = lines
            .iter()
            .enumerate()
            .filter(|(_, p)| {
                (p.y - l.y).abs() < l.em * 0.35
                    && (p.heading_em - l.heading_em).abs() < 0.5
                    && p.text.chars().count() <= 24
                    && p.text.chars().any(char::is_alphabetic)
            })
            .collect();
        if row.len() < 3 {
            continue;
        }
        row.sort_by(|(_, a), (_, b)| a.x.total_cmp(&b.x));
        let (left, right) = (row[0].1.x, row.last().unwrap().1.end);
        let evidence = lines.iter().any(|p| {
            p.x < right
                && p.end > left
                && ((p.table.is_some() && l.y > p.y && l.y - p.y < l.em * 2.)
                    || (numeric(&p.text)
                        && p.text.ends_with('%')
                        && p.y > l.y
                        && p.y - l.y < l.em * 5.))
        });
        if evidence {
            let label = row
                .iter()
                .map(|(_, p)| p.text.split_whitespace().collect::<Vec<_>>().join(" "))
                .collect::<Vec<_>>()
                .join(" ");
            repeated.insert(label);
            for (j, _) in row {
                result[j] = true;
            }
        }
    }
    // A continued table can paint the same header labels as one text run.
    for (i, l) in lines.iter().enumerate() {
        if repeated.contains(&l.text.split_whitespace().collect::<Vec<_>>().join(" ")) {
            result[i] = true;
        }
        // A vertical series of aligned numeric intervals under a figure caption
        // is an axis/category list, including its open-ended textual category.
        if l.text.chars().count() <= 24
            && l.text.chars().any(|c| c.is_ascii_digit())
            && l.text.chars().any(char::is_alphabetic)
            && lines
                .iter()
                .any(|p| caption_start(&p.text) && p.y > l.y && p.y - l.y < l.em * 6.)
        {
            let ranges: Vec<_> = lines
                .iter()
                .filter(|p| {
                    let parts: Vec<_> = p.text.split(['-', '–', '—']).collect();
                    parts.len() == 2
                        && parts.iter().all(|s| {
                            !s.trim().is_empty() && s.trim().chars().all(|c| c.is_ascii_digit())
                        })
                        && (p.end - l.end).abs() < l.em * 0.5
                        && (p.heading_em - l.heading_em).abs() < 0.5
                        && p.y < l.y
                        && l.y - p.y < l.em * 16.
                })
                .collect();
            if ranges.len() >= 3 && ranges.iter().any(|p| l.y - p.y < l.em * 6.) {
                result[i] = true;
            }
        }
    }
    result
}

// Keep a wrapped caption together even after removing its heading markup.
// Otherwise a small superscript or a neighbouring column can interrupt it.
fn join_caption_lines(lines: &mut Vec<Line>) {
    let mut removed = vec![false; lines.len()];
    for i in 0..lines.len() {
        if removed[i] || !caption_start(&lines[i].text) {
            continue;
        }
        loop {
            let a = &lines[i];
            let next = lines
                .iter()
                .enumerate()
                .filter(|(j, b)| {
                    *j != i
                        && !removed[*j]
                        && b.table.is_none()
                        && b.y < a.bottom - a.em * 0.5
                        && b.x < a.end
                        && b.end > a.x
                        && b.em >= a.em * 0.8
                })
                .max_by(|(_, a), (_, b)| a.y.total_cmp(&b.y));
            let Some((j, b)) = next else {
                break;
            };
            // Only intervene when a small numeric annotation would interrupt
            // the caption. Ordinary caption lines retain their established
            // geometry and column order.
            let annotation_between = lines.iter().any(|p| {
                p.y < a.bottom
                    && p.y > b.y
                    && p.em < a.em * 0.8
                    && numeric(&p.text)
                    && p.x >= a.x
                    && p.x <= a.end.max(b.end) + a.em
            });
            if a.bottom - b.y >= a.em.max(b.em) * 1.8
                || (a.heading_em - b.heading_em).abs() >= 0.5
                || a.bold != b.bold
                || !annotation_between
                || a.text
                    .trim_end_matches(['”', '"', '’'])
                    .ends_with(['.', '!', '?'])
            {
                break;
            }
            let (text, x, end, bottom) = (b.text.clone(), b.x, b.end, b.bottom);
            lines[i].text.push(' ');
            lines[i].text.push_str(&text);
            lines[i].x = lines[i].x.min(x);
            lines[i].end = lines[i].end.max(end);
            lines[i].bottom = bottom;
            removed[j] = true;
        }
    }
    let mut i = 0;
    lines.retain(|_| {
        let keep = !removed[i];
        i += 1;
        keep
    });
}

// Repeated dense numeric rows are tabular even when merged headers or sparse
// rules prevent safe cell reconstruction. Keep their row order without inventing
// a table schema. Ordinary prose columns still use whitespace cuts below.
fn join_numeric_rows(lines: Vec<Line>) -> Vec<Line> {
    let mut rows: Vec<Vec<Line>> = Vec::new();
    for l in lines {
        if rows
            .last()
            .is_none_or(|row| (row[0].y - l.y).abs() > row[0].em.min(l.em) * 0.35)
        {
            rows.push(Vec::new());
        }
        rows.last_mut().unwrap().push(l);
    }
    let candidates: Vec<bool> = rows
        .iter()
        .map(|row| {
            let n = row.iter().filter(|l| numeric(&l.text)).count();
            n >= 3 && n * 5 >= row.len() * 3
        })
        .collect();
    let mut links = vec![false; rows.len()];
    for i in 1..rows.len() {
        let (a, b) = (&rows[i - 1], &rows[i]);
        let em = a[0].em.max(b[0].em);
        if !candidates[i - 1] || !candidates[i] || a[0].y - b[0].y > em * 2.2 {
            continue;
        }
        // A two-pointer scan keeps this bounded even for very wide pages.
        let (mut j, mut k, mut matches) = (0, 0, 0);
        while j < a.len() && k < b.len() {
            if (a[j].end - b[k].end).abs() <= em * 0.6 || (a[j].x - b[k].x).abs() <= em * 0.6 {
                if numeric(&a[j].text) && numeric(&b[k].text) {
                    matches += 1;
                }
                j += 1;
                k += 1;
            } else if a[j].x < b[k].x {
                j += 1;
            } else {
                k += 1;
            }
        }
        links[i] = matches >= 3;
    }
    let mut joined = vec![false; rows.len()];
    for i in 2..rows.len() {
        if links[i - 1] && links[i] {
            joined[i - 2..=i].fill(true);
        }
    }
    let mut out = Vec::new();
    for (i, row) in rows.into_iter().enumerate() {
        if joined[i] {
            let mut parts = row.into_iter();
            let mut l = parts.next().unwrap();
            l.heading_em = 0.;
            for next in parts {
                l.text.push(' ');
                l.text.push_str(&next.text);
                l.end = l.end.max(next.end);
                l.em = l.em.max(next.em);
            }
            out.push(l);
        } else {
            out.extend(row);
        }
    }
    out
}

// Repeated, displaced small bold styles can be sidebar headings even below
// the main body size.
fn sidebar_headings(lines: &[Line], body: f64) -> Vec<bool> {
    let mut result = vec![false; lines.len()];
    let mut body_positions = std::collections::BTreeMap::<i64, usize>::new();
    for l in lines.iter().filter(|l| !l.bold && l.table.is_none()) {
        *body_positions
            .entry((l.x / body.max(1.)).round() as i64)
            .or_default() += l.text.chars().count();
    }
    let Some((&body_x, _)) = body_positions.iter().max_by_key(|(_, n)| **n) else {
        return result;
    };
    let mut styles = std::collections::BTreeMap::<(i64, i64), Vec<usize>>::new();
    for (i, l) in lines.iter().enumerate() {
        if !l.bold
            || l.heading_em < body * 0.72
            || l.heading_em >= body * 0.95
            || l.text.chars().count() > 100
            || l.text.ends_with(['.', ',', ';'])
            || l.text.chars().filter(|c| c.is_alphabetic()).count() < 3
        {
            continue;
        }
        let x = (l.x / body.max(1.)).round() as i64;
        if (x - body_x).abs() < 8 {
            continue;
        }
        // A parallel row of labels is evidence of a table or chart.
        if lines.iter().enumerate().any(|(j, p)| {
            j != i
                && p.table.is_none()
                && (p.y - l.y).abs() < l.em * 0.2
                && (p.x - l.x).abs() > body * 2.
        }) {
            continue;
        }
        styles
            .entry((x, (l.heading_em * 2.).round() as i64))
            .or_default()
            .push(i);
    }
    for ids in styles.values() {
        let mut groups: Vec<Vec<usize>> = Vec::new();
        for &i in ids {
            if groups
                .last()
                .is_none_or(|g| lines[*g.last().unwrap()].y - lines[i].y > lines[i].em * 1.8)
            {
                groups.push(Vec::new());
            }
            groups.last_mut().unwrap().push(i);
        }
        if groups.len() < 2 || groups.len() * 5 > lines.len() || groups.iter().any(|g| g.len() > 3)
        {
            continue;
        }
        let titles: Vec<String> = groups
            .iter()
            .map(|g| {
                g.iter()
                    .map(|&i| lines[i].text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect();
        if titles.iter().any(|t| t.split_whitespace().count() > 12)
            || titles.windows(2).any(|p| p[0] == p[1])
        {
            continue;
        }
        for &i in ids {
            result[i] = true;
        }
    }
    result
}

fn heading_levels(
    lines: &[Line],
    tables: &[super::tables::Table],
    page: Option<&super::inspection::ImageCoverage>,
    contents: bool,
) -> Vec<u8> {
    let mut weights = std::collections::BTreeMap::<i64, usize>::new();
    let mut counts = std::collections::BTreeMap::<i64, usize>::new();
    for l in lines {
        if l.table.is_none() {
            *weights.entry((l.em * 4.).round() as i64).or_default() += l.text.chars().count();
            *counts.entry((l.em * 4.).round() as i64).or_default() += 1;
        }
    }
    let outside_sizes: Vec<_> = weights.keys().copied().collect();
    // Extracting a table must not erase the page's body-font evidence. Otherwise
    // a title above a mostly tabular page becomes the apparent body size and
    // loses its heading. Cell text itself still never becomes a page heading.
    for table in tables {
        let size = (table.em * 4.).round() as i64;
        *weights.entry(size).or_default() += table
            .rows
            .iter()
            .flatten()
            .map(|cell| cell.text.chars().count())
            .sum::<usize>();
        *counts.entry(size).or_default() += table.rows.len();
    }
    let (mode, weight) = weights
        .iter()
        .map(|(&em, &n)| (em, n))
        .max_by_key(|&(_, n)| n)
        .unwrap_or_default();
    // Footnotes may contain more characters than the main text. A larger size
    // used on several substantial lines is another body size, not a heading.
    let mut body = weights
        .iter()
        .filter(|(em, n)| {
            **n >= weight / 4
                && counts[em] >= 3
                && lines
                    .iter()
                    .filter(|l| {
                        !l.bold
                            && (l.em * 4.).round() as i64 == **em
                            && l.text.chars().count() >= 40
                    })
                    .count()
                    >= 2
        })
        .map(|(&em, _)| em)
        .max()
        .unwrap_or(mode)
        .max(mode);
    let mut run = 1;
    for pair in lines.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        if (a.em - b.em).abs() < 0.25
            && (a.x - b.x).abs() < a.em
            && a.text.chars().count() >= 40
            && b.text.chars().count() >= 40
            && !a.bold
            && !b.bold
            && a.y - b.y >= a.em * 0.5
            && a.y - b.y <= a.em * 1.8
        {
            run += 1;
            if run >= 3 {
                body = body.max((b.em * 4.).round() as i64);
            }
        } else {
            run = 1;
        }
    }
    // A smaller font used only inside a table is weaker evidence: do not turn
    // ordinary surrounding prose into headings merely because a table is small.
    let contrast = if outside_sizes.contains(&body) {
        1.18
    } else {
        1.5
    };
    let bold_weight: usize = lines
        .iter()
        .filter(|l| l.bold)
        .map(|l| l.text.chars().count())
        .sum();
    let total_weight: usize = weights.values().sum();
    let sidebar = sidebar_headings(lines, body as f64 / 4.);
    let excluded = heading_exclusions(lines, body as f64 / 4.);
    let labels = data_labels(lines);
    let toc = lines.iter().find(|l| {
        matches!(
            l.text.trim().to_lowercase().as_str(),
            "contents" | "table of contents"
        )
    });
    let candidates: Vec<bool> = lines
        .iter()
        .enumerate()
        .map(|(index, l)| {
            let text = l.text.trim();
            let caption = caption_start(text)
                || lines.iter().any(|p| {
                    caption_start(&p.text)
                        && p.y > l.y
                        && p.y - l.y < l.em * 1.8
                        && (p.heading_em - l.heading_em).abs() < 0.5
                        && p.x < l.end
                        && p.end > l.x
                });
            let below = lines
                .iter()
                .filter(|other| {
                    other.y < l.y - l.em * 0.5
                        && other.x < l.end
                        && other.end > l.x
                        && other.table.is_none()
                })
                .max_by(|a, b| a.y.total_cmp(&b.y));
            let separated =
                below.is_some_and(|b| l.y - b.y > l.em * 2. && b.text.chars().count() > 40);
            let caps = text
                .chars()
                .filter(|c| c.is_alphabetic())
                .all(|c| c.is_uppercase())
                && text.chars().filter(|c| c.is_alphabetic()).count() > 8;
            let styled = (l.bold && (bold_weight * 2 < total_weight || separated) || caps)
                && l.heading_em >= body as f64 / 4. * 0.95;
            let opening = index == 0
                && separated
                && text.chars().count() < 70
                && l.em - l.heading_em < 0.5
                && l.heading_em >= body as f64 / 4. * 0.98;
            let large = l.heading_em >= body as f64 / 4. * contrast;
            let title_case = text
                .split_whitespace()
                .filter(|w| {
                    ![
                        "a", "an", "the", "of", "and", "or", "for", "to", "in", "on", "at", "by",
                        "with", "from", "as",
                    ]
                    .contains(&w.to_lowercase().as_str())
                })
                .all(|w| {
                    w.chars()
                        .find(|c| c.is_alphabetic())
                        .is_none_or(char::is_uppercase)
                });
            let author_run = text.matches(',').count() >= 2
                && lines.iter().any(|p| {
                    p.y > l.y
                        && p.y - l.y < l.em * 2.
                        && (p.heading_em - l.heading_em).abs() < 0.5
                        && p.text.matches(',').count() >= 3
                });
            let toc_entry = toc.is_some_and(|t| l.y < t.y && l.heading_em <= t.heading_em)
                || (contents && l.heading_em <= body as f64 / 4. * 1.3);
            let running_head = index == 0
                && lines.len() > 8
                && l.heading_em <= body as f64 / 4. * 1.1
                && below.is_some_and(|b| l.y - b.y > l.em * 3.);
            let paragraph_run = l.heading_em < body as f64 / 4. * 1.3
                && !l.bold
                && text.chars().count() > 45
                && below.is_some_and(|b| {
                    (b.heading_em - l.heading_em).abs() < 0.5
                        && l.y - b.y < l.em * 1.8
                        && b.text.chars().count() > 40
                });
            let annotation = l.heading_em <= body as f64 / 4. * 1.1
                && (text.starts_with(['*', '†', '‡'])
                    || text.split_whitespace().next().is_some_and(bullet));
            let inline_list_lead = l.bold
                && l.heading_em <= body as f64 / 4. * 1.1
                && lines.iter().any(|p| {
                    letter_marker(&p.text)
                        && (p.y - l.y).abs() < l.em * 0.35
                        && p.end < l.x
                        && l.x - p.end < l.em * 3.
                })
                && below.is_some_and(|b| {
                    b.mixed_bold_lead
                        && (b.heading_em - l.heading_em).abs() < 0.5
                        && l.y - b.y < l.em * 1.8
                        && (l.x - b.x).abs() < l.em
                });
            let photo_running_head = index == 0
                && lines.len() >= 3
                && text.chars().count() < 70
                && l.heading_em <= body as f64 / 4. * 1.1
                && page.is_some_and(|page| {
                    page.fraction().is_some_and(|f| f > 0.45)
                        && page.page_box.is_some_and(|[_, bottom, _, top]| {
                            let height = top - bottom;
                            l.y > top - height * 0.1
                                && below.is_some_and(|b| {
                                    l.y - b.y > height * 0.4 && b.y < bottom + height * 0.25
                                })
                        })
                });
            // Repetitive glyph noise (for example an OCR barcode layer) is
            // retained as text but provides no semantic heading evidence.
            let mut symbols = std::collections::BTreeMap::<char, usize>::new();
            for c in text.chars().filter(|c| c.is_alphanumeric()) {
                *symbols.entry(c).or_default() += 1;
            }
            let repetitive = symbols.values().sum::<usize>() >= 30 && symbols.len() <= 4;
            (large || styled || opening || sidebar[index])
                && !repetitive
                && !inline_list_lead
                && !photo_running_head
                && !excluded[index]
                && !labels[index]
                && !annotation
                && !toc_entry
                && !running_head
                && !paragraph_run
                && l.heading_em > 0.
                && !caption
                && !text.contains("...")
                && text.matches(',').count() < 3
                && !author_run
                && !text.contains("://")
                && text.chars().count() <= 120
                && (text.chars().count() * 2 < weight || l.heading_em >= body as f64 / 4. * 1.6)
                && text.chars().filter(|c| c.is_alphabetic()).take(3).count() == 3
                && (!text.ends_with(['.', '!', '?', ';', '-', '‐', '。', '！', '？'])
                    || ((large || (styled && text.ends_with(['?', '!', '？', '！'])))
                        && text.chars().count() < 90
                        && !text.ends_with([';', '-', '‐'])
                        && (!text.ends_with(['.', '。']) || title_case || (l.bold && separated))))
        })
        .collect();
    let mut sizes: Vec<_> = lines
        .iter()
        .zip(&candidates)
        .filter(|(_, yes)| **yes)
        .map(|(l, _)| (l.heading_em * 2.).round() as i64)
        .collect();
    sizes.sort_unstable_by(|a, b| b.cmp(a));
    sizes.dedup();
    lines
        .iter()
        .zip(candidates)
        .map(|(l, yes)| {
            if yes {
                (sizes
                    .binary_search_by(|em| ((l.heading_em * 2.).round() as i64).cmp(em))
                    .unwrap()
                    + 1)
                .min(6) as u8
            } else {
                0
            }
        })
        .collect()
}

// Repeated heading fields can use a larger gap between their label and value
// than between ordinary words. Require two aligned, nearby rows, a colon and
// heading evidence on both sides; never cross an established column gutter or
// consume table cells/body text. This uses layout rather than field vocabulary.
fn join_heading_fields(lines: &mut Vec<Line>, levels: &mut Vec<u8>, gutters: &[f64]) {
    let mut pairs = Vec::new();
    for (i, label) in lines.iter().enumerate() {
        if levels[i] == 0 || label.table.is_some() || !label.text.ends_with(':') {
            continue;
        }
        let next = lines
            .iter()
            .enumerate()
            .filter(|(_, value)| {
                value.x >= label.end && (value.y - label.y).abs() < label.em.min(value.em) * 0.35
            })
            .min_by(|(_, a), (_, b)| a.x.total_cmp(&b.x));
        let Some((j, value)) = next else {
            continue;
        };
        let em = label.em.min(value.em);
        if levels[j] > 0
            && value.table.is_none()
            && !value.text.ends_with(':')
            && heading_number(&value.text) != Some(HeadingNumber::Explicit)
            && label.em.max(value.em) <= em * 1.5
            && value.x - label.end <= em * 5.
            && !gutters.iter().any(|&x| label.end < x && x < value.x)
        {
            pairs.push((i, j));
        }
    }
    let supported: Vec<_> = pairs
        .iter()
        .copied()
        .filter(|&(i, j)| {
            pairs.iter().filter(|&&(_, b)| b == j).count() == 1
                && pairs.iter().any(|&(a, b)| {
                    let em = lines[i].em.min(lines[a].em);
                    i != a
                        && j != b
                        && (lines[i].x - lines[a].x).abs() < em
                        && (lines[i].heading_em - lines[a].heading_em).abs() < 0.5
                        && (lines[j].end - lines[b].end).abs() < em
                        && (lines[i].y - lines[a].y).abs() > em * 0.5
                        && (lines[i].y - lines[a].y).abs() < em * 3.
                })
        })
        .collect();
    let mut removed = vec![false; lines.len()];
    for (i, j) in supported {
        let text = lines[j].text.clone();
        lines[i].text.push(' ');
        lines[i].text.push_str(&text);
        lines[i].end = lines[j].end;
        lines[i].em = lines[i].em.max(lines[j].em);
        lines[i].complete_field = true;
        removed[j] = true;
    }
    let mut i = 0;
    lines.retain(|_| {
        let keep = !removed[i];
        i += 1;
        keep
    });
    let mut i = 0;
    levels.retain(|_| {
        let keep = !removed[i];
        i += 1;
        keep
    });
}

fn gap(lines: &[Line], ids: &[usize], vertical: bool, threshold: f64) -> Option<f64> {
    let mut intervals: Vec<_> = ids
        .iter()
        .map(|&i| {
            let l = &lines[i];
            if vertical {
                (l.x, l.end)
            } else {
                (-l.y - l.em * 0.8, -l.bottom + l.em * 0.2)
            }
        })
        .collect();
    intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut end = intervals[0].1;
    let mut best = None;
    let mut size = threshold;
    for (start, last) in intervals.into_iter().skip(1) {
        if start - end > size {
            size = start - end;
            best = Some((start + end) / 2.);
        }
        end = end.max(last);
    }
    best
}
fn reading_order(lines: &[Line], ctx: &mut Context<'_>) -> Result<Vec<usize>, Error> {
    let mut tasks = vec![((0..lines.len()).collect::<Vec<_>>(), 0usize)];
    let mut out = Vec::new();
    while let Some((mut ids, depth)) = tasks.pop() {
        ctx.node()?;
        if ids.is_empty() {
            continue;
        }
        if ids.len() == 1 {
            out.extend(ids);
            continue;
        }
        let mut sizes: Vec<_> = ids.iter().map(|&i| lines[i].em).collect();
        sizes.sort_by(f64::total_cmp);
        let em = sizes[sizes.len() / 2];
        if depth < ctx.options.limits.max_depth {
            if let Some(cut) = gap(lines, &ids, true, (em * 0.9).max(4.)) {
                let (left, right): (Vec<_>, Vec<_>) =
                    ids.iter().copied().partition(|&i| lines[i].end < cut);
                if left.len() >= 2 && right.len() >= 2 {
                    let base_rtl = ids.iter().filter(|&&i| lines[i].rtl).count() * 2 > ids.len();
                    let (first, second) = if base_rtl {
                        (right, left)
                    } else {
                        (left, right)
                    };
                    tasks.push((second, depth + 1));
                    tasks.push((first, depth + 1));
                    continue;
                }
            }
            if let Some(cut) = gap(lines, &ids, false, em * 0.65) {
                let (top, bottom): (Vec<_>, Vec<_>) =
                    ids.iter().copied().partition(|&i| -lines[i].y < cut);
                if !top.is_empty() && !bottom.is_empty() {
                    tasks.push((bottom, depth + 1));
                    tasks.push((top, depth + 1));
                    continue;
                }
            }
        }
        ids.sort_by(|&a, &b| {
            lines[b]
                .y
                .total_cmp(&lines[a].y)
                .then(lines[a].x.total_cmp(&lines[b].x))
        });
        let left = ids
            .iter()
            .map(|&i| lines[i].x)
            .fold(f64::INFINITY, f64::min);
        let right = ids
            .iter()
            .map(|&i| lines[i].end)
            .fold(f64::NEG_INFINITY, f64::max);
        // Full-width prose has no column cut. Emit in one pass rather than
        // repeatedly sorting every suffix of a long page.
        let broad = ids
            .iter()
            .filter(|&&i| lines[i].end - lines[i].x > (right - left) * 0.65)
            .count();
        if depth >= ctx.options.limits.max_depth || broad * 5 >= ids.len() * 4 {
            out.extend(ids);
            continue;
        }
        let y = lines[ids[0]].y;
        let split = ids.partition_point(|&i| y - lines[i].y < em * 0.4);
        let rest = ids.split_off(split.max(1));
        out.extend(ids);
        tasks.push((rest, depth));
    }
    Ok(out)
}

pub(super) enum Piece {
    Text(String),
    Heading(u8, String),
    Table(usize),
}

pub(super) fn text(glyphs: Vec<Glyph>, ctx: &mut Context<'_>) -> Result<String, Error> {
    let mut out = String::new();
    for piece in arrange_impl(glyphs, &[], ctx, false, None)? {
        if let Piece::Text(text) = piece {
            out.push_str(&text);
        }
    }
    Ok(out)
}

pub(super) fn arrange(
    glyphs: Vec<Glyph>,
    tables: &[super::tables::Table],
    page: &super::inspection::ImageCoverage,
    ctx: &mut Context<'_>,
) -> Result<Vec<Piece>, Error> {
    arrange_impl(glyphs, tables, ctx, true, Some(page))
}

// Projection valleys reveal repeated gutters even when a writer paints spaces
// across them. Only strong alphabetic evidence on both sides supports a cut.
// Cell text never uses this.
fn column_gutters(glyphs: &[Glyph]) -> Vec<f64> {
    let ink: Vec<_> = glyphs
        .iter()
        .filter(|g| !g.text.trim().is_empty())
        .collect();
    if ink.len() < 200 {
        return Vec::new();
    }
    let mut xs: Vec<_> = ink.iter().map(|g| g.x).collect();
    xs.sort_by(f64::total_cmp);
    let (left, right) = (xs[xs.len() / 50], xs[xs.len() * 49 / 50]);
    if right - left < 150. {
        return Vec::new();
    }
    let step = ((right - left) / 256.).max(1.);
    let mut counts = vec![0usize; 257];
    for g in &ink {
        let a = ((g.x - left) / step).floor().max(0.) as usize;
        let b = ((g.end - left) / step).ceil().max(0.) as usize;
        for v in counts.iter_mut().take(b.min(257)).skip(a.min(257)) {
            *v += 1;
        }
    }
    let threshold = counts.iter().copied().max().unwrap_or(0) / 10;
    let mut cuts = Vec::new();
    let mut i = 26;
    while i < 231 {
        if counts[i] > threshold {
            i += 1;
            continue;
        }
        let start = i;
        while i < 231 && counts[i] <= threshold {
            i += 1;
        }
        if (i - start) as f64 * step < 3. {
            continue;
        }
        let x = left + (start + i) as f64 * step / 2.;
        let mut sides = [
            std::collections::BTreeSet::new(),
            std::collections::BTreeSet::new(),
        ];
        for g in &ink {
            if g.text.chars().any(char::is_alphabetic) && (g.x - x).abs() < 150. {
                sides[usize::from(g.x > x)].insert((g.y / 3.).round() as i64);
            }
        }
        if sides.iter().all(|s| s.len() >= 8) {
            cuts.push(x);
        }
    }
    cuts
}

fn arrange_impl(
    mut glyphs: Vec<Glyph>,
    tables: &[super::tables::Table],
    ctx: &mut Context<'_>,
    infer_headings: bool,
    page: Option<&super::inspection::ImageCoverage>,
) -> Result<Vec<Piece>, Error> {
    let mut rotated = Vec::new();
    glyphs.retain(|g| {
        if g.rotation != 0 {
            rotated.push(g.clone());
            false
        } else {
            true
        }
    });
    glyphs.sort_by(|a, b| {
        b.y.total_cmp(&a.y)
            .then(a.x.total_cmp(&b.x))
            .then(a.seq.cmp(&b.seq))
    });
    let gutters = if infer_headings {
        column_gutters(&glyphs)
    } else {
        Vec::new()
    };
    let mut lines = Vec::new();
    let mut start = 0;
    while start < glyphs.len() {
        ctx.check()?;
        let baseline = glyphs[start].y;
        let mut end = start + 1;
        while end < glyphs.len()
            && (baseline - glyphs[end].y).abs() < glyphs[start].em.min(glyphs[end].em) * 0.35
        {
            end += 1;
        }
        let row = &mut glyphs[start..end];
        let first_line = lines.len();
        row.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.seq.cmp(&b.seq)));
        let mut fragment = 0;
        let mut ink = 0;
        for i in 1..row.len() {
            if row[i].x - row[i - 1].end > (row[i].em.min(row[i - 1].em) * 1.0).max(4.)
                || (!row[i].text.trim().is_empty()
                    && row[i].x - row[ink].end > row[i].em.min(row[ink].em) * 0.6
                    && gutters.iter().any(|&x| row[ink].end < x && row[i].x > x))
            {
                let l = line(&row[fragment..i]);
                if !l.text.is_empty() {
                    lines.push(l);
                }
                fragment = i;
            }
            if !row[i].text.trim().is_empty() {
                ink = i;
            }
        }
        let l = line(&row[fragment..]);
        if !l.text.is_empty() {
            lines.push(l);
        }
        if lines.len() > first_line + 1 {
            let fragments = lines.split_off(first_line);
            let mut merged: Vec<Line> = Vec::new();
            for next in fragments {
                if let Some(prev) = merged.last_mut() {
                    if bullet(&prev.text)
                        && next.x - prev.end <= next.em * 3.
                        && next.text.chars().any(char::is_alphabetic)
                    {
                        prev.text.push(' ');
                        prev.text.push_str(&next.text);
                        prev.end = next.end;
                        prev.em = prev.em.max(next.em);
                        prev.heading_em = next.heading_em;
                        prev.bold = next.bold;
                        continue;
                    }
                    let numbered = prev
                        .text
                        .chars()
                        .all(|c| c.is_numeric() || ".()".contains(c));
                    if numbered
                        && (next.bold
                            || prev.bold
                            || next
                                .text
                                .chars()
                                .filter(|c| c.is_alphabetic())
                                .all(|c| c.is_uppercase()))
                        && (prev.em - next.em).abs() < 0.5
                        && next.x - prev.end <= next.em * 3.
                    {
                        prev.text.push(' ');
                        prev.text.push_str(&next.text);
                        prev.end = next.end;
                        prev.bold = next.bold;
                        prev.heading_em = next.heading_em;
                        continue;
                    }
                }
                merged.push(next);
            }
            let numeric_count = merged.iter().filter(|l| numeric(&l.text)).count();
            // A balanced row of explicit outline markers and styled titles is
            // different from numeric data. Require each marker-title pair;
            // simply having several alphabetic fragments is not enough.
            let numbered_titles = merged.len() >= 4
                && merged.len().is_multiple_of(2)
                && merged.as_chunks::<2>().0.iter().all(|pair| {
                    let (marker, title) = (&pair[0], &pair[1]);
                    let label = marker.text.trim();
                    numeric(label)
                        && label.starts_with('(')
                        && label.ends_with(')')
                        && label.len() <= 8
                        && title.bold
                        && title.text.chars().filter(|c| c.is_alphabetic()).count() >= 3
                        && title.x > marker.end
                        && title.x - marker.end < title.em * 8.
                });
            if merged.len() >= 3 && numeric_count >= 2 && !numbered_titles {
                for l in &mut merged {
                    l.heading_em = 0.;
                }
            }
            lines.extend(merged);
        }
        start = end;
    }
    let mut lines = join_numeric_rows(lines);
    if infer_headings {
        join_running_header(&mut lines);
        join_caption_lines(&mut lines);
    }
    for (i, t) in tables.iter().enumerate() {
        lines.push(Line {
            text: String::new(),
            x: t.left,
            end: t.right,
            y: t.top,
            bottom: t.bottom,
            em: t.em,
            rtl: false,
            table: Some(i),
            heading_em: 0.,
            bold: false,
            mixed_bold_lead: false,
            complete_field: false,
        });
    }
    let contents = infer_headings && join_contents_entries(&mut lines);
    let mut levels = if infer_headings {
        heading_levels(&lines, tables, page, contents)
    } else {
        vec![0; lines.len()]
    };
    join_heading_fields(&mut lines, &mut levels, &gutters);
    // Merge physical continuations before reading-order cuts can separate a
    // centred or wrapped title into different sections of the page.
    let numbering: Vec<_> = lines.iter().map(|l| heading_number(&l.text)).collect();
    let mut removed = vec![false; lines.len()];
    for i in 0..lines.len() {
        if levels[i] == 0 || removed[i] || lines[i].complete_field {
            continue;
        }
        loop {
            let continuation = (0..lines.len())
                .filter(|&j| {
                    let (a, b) = (&lines[i], &lines[j]);
                    let marker = a.text.len() <= 8
                        && a.text
                            .chars()
                            .all(|c| c.is_ascii_digit() || "IVXLC.():".contains(c));
                    let lower_start = b
                        .text
                        .chars()
                        .find(|c| c.is_alphabetic())
                        .is_some_and(char::is_lowercase);
                    let numbered = a.text.starts_with(char::is_numeric);
                    let same_style = (a.heading_em - b.heading_em).abs() < 0.5
                        || (marker
                            && a.bold == b.bold
                            && (a.heading_em - b.heading_em).abs() < b.heading_em * 0.15);
                    let aligned = (a.x - b.x).abs() < b.em
                        || ((a.x + a.end - b.x - b.end) / 2.).abs() < b.em
                        || (numbered && lower_start && b.x >= a.x && b.x - a.x < b.em * 3.);
                    j != i
                        && !removed[j]
                        && levels[j] > 0
                        && !b.complete_field
                        && !numbered_heading_boundary(numbering[i], numbering[j])
                        && same_style
                        && aligned
                        && a.bottom - b.y > b.em * 0.5
                        && a.bottom - b.y < b.em * if lower_start { 2.1 } else { 1.65 }
                })
                .max_by(|&a, &b| lines[a].y.total_cmp(&lines[b].y));
            let Some(j) = continuation else {
                break;
            };
            if lines[i].text.len() + lines[j].text.len() > 180 {
                break;
            }
            let text = lines[j].text.clone();
            lines[i].text.push(' ');
            lines[i].text.push_str(&text);
            lines[i].bottom = lines[j].bottom;
            lines[i].heading_em = lines[j].heading_em;
            lines[i].x = lines[i].x.min(lines[j].x);
            lines[i].end = lines[i].end.max(lines[j].end);
            removed[j] = true;
        }
    }
    let mut k = 0;
    lines.retain(|_| {
        let keep = !removed[k];
        k += 1;
        keep
    });
    let mut k = 0;
    levels.retain(|_| {
        let keep = !removed[k];
        k += 1;
        keep
    });
    let order = reading_order(&lines, ctx)?;
    let mut out = String::new();
    let mut pieces = Vec::new();
    let mut previous: Option<&Line> = None;
    let mut previous_heading: Option<&Line> = None;
    for i in order {
        let l = &lines[i];
        if levels[i] > 0 {
            if !out.is_empty() {
                pieces.push(Piece::Text(std::mem::take(&mut out)));
            }
            let joins = previous_heading.is_some_and(|p| {
                !p.complete_field
                    && !l.complete_field
                    && !numbered_heading_boundary(heading_number(&p.text), heading_number(&l.text))
                    && (p.heading_em - l.heading_em).abs() < 0.5
                    && p.x < l.end
                    && p.end > l.x
                    && p.y - l.y > l.em * 0.5
                    && p.y - l.y < l.em * 1.65
            });
            if joins {
                if let Some(Piece::Heading(_, text)) = pieces.last_mut() {
                    text.push(' ');
                    text.push_str(&l.text);
                }
            } else {
                pieces.push(Piece::Heading(levels[i], l.text.clone()));
            }
            previous_heading = Some(l);
            previous = None;
            continue;
        }
        previous_heading = None;
        if let Some(index) = l.table {
            if !out.is_empty() {
                pieces.push(Piece::Text(std::mem::take(&mut out)));
            }
            pieces.push(Piece::Table(index));
            previous = None;
            continue;
        }
        if let Some(p) = previous {
            out.push('\n');
            if p.y - l.y > 1.7 * p.em.max(l.em) || l.y > p.y + p.em * 0.5 {
                out.push('\n');
            }
        }
        out.push_str(&l.text);
        previous = Some(l);
    }
    for orientation in 1..=4 {
        let mut group: Vec<_> = rotated
            .iter()
            .filter(|g| g.rotation == orientation)
            .cloned()
            .collect();
        if group.is_empty() {
            continue;
        }
        out.push_str("\n\n");
        if orientation == 4 {
            group.sort_by_key(|g| g.seq);
            for g in group {
                out.push_str(&g.text);
            }
        } else {
            for g in &mut group {
                g.rotation = 0;
            }
            out.push_str(&text(group, ctx)?);
        }
    }
    if out.len() > ctx.options.limits.max_text_bytes {
        return Err(Error::Limit("max_text_bytes"));
    }
    if !out.is_empty() {
        pieces.push(Piece::Text(out));
    }
    Ok(pieces)
}
