//! Assemble explicit row boundaries and merge flags into a rectangular table.
use super::*;
#[derive(Clone, Copy, Default)]
pub(super) struct Properties {
    pub right: Option<i64>,
    pub horizontal_start: bool,
    pub horizontal_continue: bool,
    pub vertical_start: bool,
    pub vertical_continue: bool,
}
#[derive(Default)]
pub(super) struct Row {
    pub left: i64,
    pub header: bool,
    pub cells: Vec<(Cell, Properties)>,
}
struct Origin {
    left: usize,
    right: usize,
    cell: Cell,
    vertical_start: bool,
    vertical_continue: bool,
    covered: bool,
}
pub(super) fn build(
    rows: Vec<Row>,
    src: &Source,
    ctx: &mut Context<'_>,
) -> Result<Option<Block>, Error> {
    build_for_format(rows, src, ctx, "rtf")
}
pub(super) fn build_for_format(
    mut rows: Vec<Row>,
    src: &Source,
    ctx: &mut Context<'_>,
    format: &str,
) -> Result<Option<Block>, Error> {
    if rows.is_empty() {
        return Ok(None);
    }
    let headers = rows.iter().take_while(|r| r.header).count();
    let mut edges = std::collections::BTreeSet::new();
    for row in &mut rows {
        let mut previous = row.left;
        edges.insert(previous);
        for (_, p) in &mut row.cells {
            let right = if let Some(r) = p.right.filter(|r| *r > previous) {
                r
            } else {
                warn(ctx,&format!("{format}_table_geometry"),"Missing or non-increasing cell boundary; text retained with a synthetic boundary.",src);
                previous
                    .checked_add(1440)
                    .ok_or_else(|| bad("table boundary overflow"))?
            };
            p.right = Some(right);
            edges.insert(right);
            previous = right;
        }
    }
    let edges: Vec<_> = edges.into_iter().collect();
    let width = edges.len().saturating_sub(1);
    if width.saturating_mul(rows.len()) > ctx.options.limits.max_table_cells {
        return Err(Error::Limit("max_table_cells"));
    }
    let col = |v| edges.binary_search(&v).unwrap();
    let mut origins: Vec<Vec<Origin>> = vec![];
    for (r, row) in rows.into_iter().enumerate() {
        let mut left = col(row.left);
        let mut current: Vec<Origin> = vec![];
        let mut hactive = false;
        for (index, (mut cell, p)) in row.cells.into_iter().enumerate() {
            let right = col(p.right.unwrap());
            cell.source.row = Some(r as u32 + 1);
            cell.source.cell = Some(index as u32 + 1);
            for b in &mut cell.blocks {
                b.source = cell.source.clone();
            }
            if p.horizontal_continue && hactive {
                let origin = current.last_mut().unwrap();
                origin.right = right;
                origin.cell.blocks.extend(cell.blocks);
            } else {
                if p.horizontal_continue {
                    warn(
                        ctx,
                        &format!("{format}_merge_orphan"),
                        "Horizontal continuation has no restart; cell retained.",
                        &cell.source,
                    );
                }
                current.push(Origin {
                    left,
                    right,
                    cell,
                    vertical_start: p.vertical_start,
                    vertical_continue: p.vertical_continue,
                    covered: false,
                });
                hactive = p.horizontal_start;
            }
            left = right;
        }
        origins.push(current);
    }
    let mut active: HashMap<(usize, usize), (usize, usize)> = HashMap::new();
    for r in 0..origins.len() {
        let mut next = HashMap::new();
        for i in 0..origins[r].len() {
            let key = (origins[r][i].left, origins[r][i].right);
            if origins[r][i].vertical_continue {
                if let Some(&(row, index)) = active.get(&key) {
                    let text = std::mem::take(&mut origins[r][i].cell.blocks);
                    origins[row][index].cell.blocks.extend(text);
                    origins[row][index].cell.row_span += 1;
                    origins[r][i].covered = true;
                    next.insert(key, (row, index));
                    continue;
                }
                warn(
                    ctx,
                    &format!("{format}_merge_orphan"),
                    "Vertical continuation has no matching restart above; cell retained.",
                    &origins[r][i].cell.source,
                );
            }
            if origins[r][i].vertical_start {
                next.insert(key, (r, i));
            }
        }
        active = next;
    }
    let height = origins.len();
    let mut covered = HashSet::new();
    let mut anchors = HashMap::new();
    for (r, row) in origins.into_iter().enumerate() {
        for mut origin in row {
            if origin.covered {
                continue;
            }
            origin.cell.col_span = (origin.right - origin.left) as u32;
            for rr in r..r + origin.cell.row_span as usize {
                for c in origin.left..origin.right {
                    if !covered.insert((rr, c)) {
                        return Err(bad("overlapping table geometry"));
                    }
                }
            }
            anchors.insert((r, origin.left), origin.cell);
        }
    }
    let mut result = vec![];
    for r in 0..height {
        let mut row = vec![];
        for c in 0..width {
            ctx.cell()?;
            if let Some(cell) = anchors.remove(&(r, c)) {
                row.push(cell);
            } else if !covered.contains(&(r, c)) {
                row.push(Cell {
                    blocks: vec![],
                    row_span: 1,
                    col_span: 1,
                    source: Source {
                        row: Some(r as u32 + 1),
                        ..src.clone()
                    },
                });
            }
        }
        result.push(row);
    }
    Ok(Some(ctx.block(
        BlockContent::Table {
            rows: result,
            header_rows: headers,
        },
        src.clone(),
    )?))
}
