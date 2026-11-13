//! Resolve explicit Word grid coordinates and merge chains without a dense grid.
use super::*;

struct Entry {
    cell: Cell,
    column: usize,
    vertical: Option<bool>, // true = restart, false = continuation
    covered: bool,
}

fn merge(props: Option<Node<'_, '_>>, name: &str) -> Option<bool> {
    props
        .and_then(|p| child(p, name))
        .map(|n| attr(n, "val") == Some("restart"))
}

impl Walker<'_, '_> {
    pub(super) fn table(&mut self, node: Node<'_, '_>, depth: usize) -> Result<Block, Error> {
        let mut matrix: Vec<Vec<Entry>> = Vec::new();
        let mut header_rows = 0;
        for row in self.members(node, depth + 1)? {
            if !w(row, "tr") {
                if !w(row, "tblPr") && !w(row, "tblGrid") && !metadata(row) {
                    self.warn(
                        "docx_table_content",
                        "Unsupported table-level element was omitted.",
                    );
                }
                continue;
            }
            let props = child(row, "trPr");
            let header = props
                .and_then(|p| child(p, "tblHeader"))
                .is_some_and(|n| !matches!(attr(n, "val"), Some("0" | "false" | "off")));
            if header && header_rows == matrix.len() {
                header_rows += 1;
            }
            let gap = |name| -> Result<usize, Error> {
                props
                    .and_then(|p| child(p, name))
                    .and_then(|n| attr(n, "val"))
                    .map(|v| {
                        v.parse()
                            .map_err(|_| malformed("invalid table grid offset"))
                    })
                    .unwrap_or(Ok(0))
            };
            let before = gap("gridBefore")?;
            let after = gap("gridAfter")?;
            let mut cells: Vec<Entry> = Vec::new();
            let mut column = 0usize;
            let mut horizontal = false;
            if before > 0 {
                self.grid_budget(before)?;
                cells.push(Entry {
                    cell: Cell {
                        blocks: vec![],
                        col_span: before as u32,
                        row_span: 1,
                        source: self.source(row),
                    },
                    column: 0,
                    vertical: None,
                    covered: false,
                });
                column = before;
            }
            for n in self.members(row, depth + 2)? {
                if !w(n, "tc") {
                    if w(n, "tblPrEx") {
                        self.warn("docx_table_style", "Table property overrides affect visual styling, which is not fully reconstructed.");
                    } else if !w(n, "trPr") && !metadata(n) {
                        self.warn(
                            "docx_table_content",
                            "Unsupported row-level element was omitted.",
                        );
                    }
                    continue;
                }
                let props = child(n, "tcPr");
                let span: u32 = props
                    .and_then(|p| child(p, "gridSpan"))
                    .and_then(|n| attr(n, "val"))
                    .map(|v| v.parse().map_err(|_| malformed("invalid table span")))
                    .transpose()?
                    .unwrap_or(1)
                    .max(1);
                self.grid_budget(span as usize)?;
                let blocks = self.blocks(n, depth + 3)?;
                let h = merge(props, "hMerge");
                if h == Some(false) && horizontal {
                    let prior = cells.last_mut().unwrap();
                    prior.cell.col_span = prior
                        .cell
                        .col_span
                        .checked_add(span)
                        .ok_or(Error::Limit("max_table_cells"))?;
                    prior.cell.blocks.extend(blocks);
                } else {
                    if h == Some(false) {
                        self.warn("docx_merge_orphan", "Horizontal continuation without a restart retained as a separate cell.");
                    }
                    cells.push(Entry {
                        cell: Cell {
                            blocks,
                            col_span: span,
                            row_span: 1,
                            source: self.source(n),
                        },
                        column,
                        vertical: merge(props, "vMerge"),
                        covered: false,
                    });
                    horizontal = h == Some(true);
                }
                column = column
                    .checked_add(span as usize)
                    .ok_or(Error::Limit("max_table_cells"))?;
            }
            if after > 0 {
                self.grid_budget(after)?;
                cells.push(Entry {
                    cell: Cell {
                        blocks: vec![],
                        col_span: after as u32,
                        row_span: 1,
                        source: self.source(row),
                    },
                    column,
                    vertical: None,
                    covered: false,
                });
            }
            matrix.push(cells);
        }
        // Only a matching restart in the immediately preceding row can extend
        // a merge. Matching the entire span avoids absorbing adjacent columns.
        let mut active: HashMap<(usize, u32), (usize, usize)> = HashMap::new();
        for r in 0..matrix.len() {
            let mut next = HashMap::new();
            for c in 0..matrix[r].len() {
                let e = &matrix[r][c];
                let key = (e.column, e.cell.col_span);
                if e.vertical == Some(false) {
                    if let Some(&(ar, ac)) = active.get(&key) {
                        let blocks = std::mem::take(&mut matrix[r][c].cell.blocks);
                        let origin: &mut Entry = &mut matrix[ar][ac];
                        origin.cell.row_span += 1;
                        // Nonempty covered content is unusual, but must not be lost.
                        origin
                            .cell
                            .blocks
                            .extend(blocks.into_iter().filter(|b| !empty_block(b)));
                        matrix[r][c].covered = true;
                        next.insert(key, (ar, ac));
                        continue;
                    }
                    self.warn("docx_merge_orphan", "Vertical continuation without a matching restart retained as a separate cell.");
                }
                if matrix[r][c].vertical == Some(true) {
                    next.insert(key, (r, c));
                }
            }
            active = next;
        }
        let rows = matrix
            .into_iter()
            .map(|row| {
                row.into_iter()
                    .filter(|e| !e.covered)
                    .map(|e| e.cell)
                    .collect()
            })
            .collect();
        self.ctx
            .block(BlockContent::Table { rows, header_rows }, self.source(node))
    }

    fn grid_budget(&mut self, count: usize) -> Result<(), Error> {
        if count > self.ctx.options.limits.max_table_cells || count > u32::MAX as usize {
            return Err(Error::Limit("max_table_cells"));
        }
        for _ in 0..count {
            self.ctx.cell()?;
        }
        Ok(())
    }
}
