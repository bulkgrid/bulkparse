//! Reduced-fidelity cell previews: fixed-width coordinate/value records prevent
//! sparse worksheets from expanding into a huge empty rectangular grid.
use crate::{Block, BlockContent, Cell, Context, Error, Inline, Source};
use std::collections::HashSet;

#[derive(Default)]
pub(crate) struct Cells<'a, 'b> {
    blocks: Vec<Block>,
    part: String,
    table: Option<usize>,
    seen: HashSet<(u32, u32)>,
    last: Source,
    pub count: usize,
    output: Option<&'a mut crate::render::Writer<'b>>,
    table_rows: usize,
}
impl<'a, 'b> Cells<'a, 'b> {
    pub fn new(output: Option<&'a mut crate::render::Writer<'b>>) -> Self {
        Self {
            output,
            ..Default::default()
        }
    }
    pub fn add(
        &mut self,
        part: &str,
        row: u32,
        col: u32,
        value: String,
        ctx: &mut Context<'_>,
    ) -> Result<bool, Error> {
        ctx.check()?;
        if value.is_empty() {
            return Ok(true);
        }
        let new = self.table.is_none() || self.part != part;
        let r = row.to_string();
        let c = col.to_string();
        let text = value
            .len()
            .saturating_add(r.len())
            .saturating_add(c.len())
            .saturating_add(if new { part.len() + 14 } else { 0 });
        // Three header cells, their paragraphs, heading and table on new parts.
        if !ctx.fits(if new { 14 } else { 6 }, if new { 6 } else { 3 }, text) {
            return Ok(false);
        }
        let source = Source {
            part: Some(part.into()),
            row: Some(row),
            cell: Some(col),
            ..Default::default()
        };
        if new {
            self.seen.clear();
            ctx.text(part)?;
            self.blocks.push(ctx.block(
                BlockContent::Heading {
                    level: 2,
                    inlines: vec![Inline::text(part)],
                },
                Source {
                    part: Some(part.into()),
                    ..Default::default()
                },
            )?);
            let mut header = Vec::with_capacity(3);
            for s in ["Row", "Column", "Value"] {
                header.push(cell(s.into(), &Source::default(), ctx)?);
            }
            self.table = Some(self.blocks.len());
            self.blocks.push(ctx.block(
                BlockContent::Table {
                    rows: vec![header],
                    header_rows: 1,
                },
                Source {
                    part: Some(part.into()),
                    ..Default::default()
                },
            )?);
            self.part = part.into();
            self.table_rows = 1;
            if let Some(output) = self.output.as_deref_mut() {
                output.blocks(&self.blocks)?;
                self.blocks.clear();
            }
        }
        if !self.seen.insert((row, col)) {
            return Err(Error::Malformed(
                "duplicate worksheet cell in preview".into(),
            ));
        }
        let mut cells = Vec::with_capacity(3);
        for v in [r, c, value] {
            cells.push(cell(v, &source, ctx)?);
        }
        if let Some(output) = self.output.as_deref_mut() {
            output.record(&cells, self.table_rows, true, 3)?;
        } else if let BlockContent::Table { rows, .. } =
            &mut self.blocks[self.table.unwrap()].content
        {
            rows.push(cells);
        }
        self.table_rows += 1;
        self.count += 1;
        self.last = source;
        Ok(true)
    }
    pub fn finish(self, ctx: &mut Context<'_>) -> Result<Vec<Block>, Error> {
        if self.count == 0 {
            return Err(Error::Limit("no_content_within_recovery_budget"));
        }
        ctx.diagnostic("spreadsheet_cell_preview", &format!("Retained {} populated source cells as Row/Column/Value records; diagnostic source identifies the last retained cell. Original grid layout, merges, formatting, notes, links and charts are not reconstructed; later cells/sheets may be omitted.",self.count), self.last, true);
        Ok(self.blocks)
    }
}
fn cell(text: String, source: &Source, ctx: &mut Context<'_>) -> Result<Cell, Error> {
    ctx.cell()?;
    ctx.text(&text)?;
    Ok(Cell {
        blocks: vec![ctx.block(
            BlockContent::Paragraph {
                inlines: vec![Inline::text(text)],
            },
            source.clone(),
        )?],
        col_span: 1,
        row_span: 1,
        source: source.clone(),
    })
}
