use crate::{Block, BlockContent, Context, Error, Inline, Source};

pub fn parse(text: &str, ctx: &mut Context<'_>, source: Source) -> Result<Vec<Block>, Error> {
    ctx.text(text)?;
    let mut blocks = Vec::new();
    let mut paragraph = String::new();
    for line in text.lines() {
        ctx.check()?;
        if line.trim().is_empty() {
            if !paragraph.is_empty() {
                blocks.push(ctx.block(
                    BlockContent::Paragraph {
                        inlines: vec![Inline::text(std::mem::take(&mut paragraph))],
                    },
                    source.clone(),
                )?);
            }
        } else {
            if !paragraph.is_empty() {
                paragraph.push('\n');
            }
            paragraph.push_str(line);
        }
    }
    if !paragraph.is_empty() {
        blocks.push(ctx.block(
            BlockContent::Paragraph {
                inlines: vec![Inline::text(paragraph)],
            },
            source,
        )?);
    }
    Ok(blocks)
}
