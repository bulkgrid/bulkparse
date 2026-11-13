//! Assemble Word cell/row terminators without throwing away rich cell paragraphs.
use super::*;
#[derive(Default)]
pub(super) struct Builder {
    blocks: Vec<Block>,
    cells: Vec<Cell>,
    rows: Vec<super::edge_table::Row>,
}
impl Builder {
    fn cell(&mut self, ctx: &mut Context<'_>, src: &Source) -> Result<(), Error> {
        ctx.cell()?;
        self.cells.push(Cell {
            blocks: std::mem::take(&mut self.blocks),
            row_span: 1,
            col_span: 1,
            source: src.clone(),
        });
        Ok(())
    }
    fn row(
        &mut self,
        info: super::word_styles::TableInfo,
        src: &Source,
        ctx: &mut Context<'_>,
    ) -> Result<(), Error> {
        if self.cells.is_empty() {
            return Ok(());
        }
        if !info.cells.is_empty() && info.cells.len() != self.cells.len() {
            warn(ctx,"doc_table_cells","Stored cell boundaries and extracted cell count differ; all cell content retained.",src);
        }
        let mut properties = info.cells.into_iter();
        let cells = std::mem::take(&mut self.cells)
            .into_iter()
            .map(|c| (c, properties.next().unwrap_or_default()))
            .collect();
        self.rows.push(super::edge_table::Row {
            left: info.left,
            header: info.header,
            cells,
        });
        Ok(())
    }
    pub fn paragraph(
        &mut self,
        inlines: Vec<Inline>,
        properties: (
            super::style::ParagraphStyle,
            super::word_styles::TableInfo,
            bool,
        ),
        src: &Source,
        ctx: &mut Context<'_>,
        out: &mut Vec<Block>,
    ) -> Result<(), Error> {
        let (style, info, cell_end) = properties;
        if info.nested {
            warn(
                ctx,
                "doc_nested_table",
                "Nested Word table hierarchy is not reconstructed; available cell text retained.",
                src,
            );
        }
        if cell_end || info.inside || info.end_row || info.nested {
            if !inlines.is_empty() {
                super::style::paragraph(inlines, style, &mut self.blocks, src, ctx)?;
            }
            if info.end_row {
                if !self.blocks.is_empty() {
                    self.cell(ctx, src)?;
                }
                self.row(info, src, ctx)?;
            } else if cell_end {
                self.cell(ctx, src)?;
            }
        } else {
            self.finish(src, ctx, out)?;
            if !inlines.is_empty() {
                super::style::paragraph(inlines, style, out, src, ctx)?;
            }
        }
        Ok(())
    }
    pub fn finish(
        &mut self,
        src: &Source,
        ctx: &mut Context<'_>,
        out: &mut Vec<Block>,
    ) -> Result<(), Error> {
        if !self.blocks.is_empty() {
            self.cell(ctx, src)?;
        }
        self.row(Default::default(), src, ctx)?;
        if let Some(t) =
            super::edge_table::build_for_format(std::mem::take(&mut self.rows), src, ctx, "doc")?
        {
            out.push(t);
        }
        Ok(())
    }
}
