//! Diagnostic retained Vec capacity; never used for benchmark timing.
use bulkparse::{Block, BlockContent, Cell, Inline, Parser, Source};
#[derive(Default, serde::Serialize)]
struct Sizes {
    blocks: [usize; 2],
    inlines: [usize; 2],
    cells: [usize; 2],
    sources: usize,
    byte_ranges: usize,
}
fn source(v: &Source, s: &mut Sizes) {
    s.sources += 1;
    s.byte_ranges += usize::from(v.byte_range.is_some());
}
fn inline(v: &Vec<Inline>, s: &mut Sizes) {
    s.inlines[0] += v.len();
    s.inlines[1] += v.capacity();
    for i in v {
        match i {
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Strike { children }
            | Inline::Link { children, .. } => inline(children, s),
            _ => {}
        }
    }
}
fn blocks(v: &Vec<Block>, s: &mut Sizes) {
    s.blocks[0] += v.len();
    s.blocks[1] += v.capacity();
    for b in v {
        source(&b.source, s);
        match &b.content {
            BlockContent::Paragraph { inlines } | BlockContent::Heading { inlines, .. } => {
                inline(inlines, s)
            }
            BlockContent::Quote { blocks: v } => blocks(v, s),
            BlockContent::List { items, .. } => {
                for v in items {
                    blocks(v, s);
                }
            }
            BlockContent::Table { rows, .. } => {
                for row in rows {
                    s.cells[0] += row.len();
                    s.cells[1] += row.capacity();
                    for c in row {
                        source(&c.source, s);
                        blocks(&c.blocks, s);
                    }
                }
            }
            _ => {}
        }
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().any(|a| a == "--schema") {
        println!(
            "{}",
            serde_json::to_string(&schemars::schema_for!(bulkparse::ParseResult))?
        );
        return Ok(());
    }
    let parser = Parser::default();
    for path in std::env::args().skip(1) {
        let bytes = std::fs::read(&path)?;
        let result = parser.parse_bytes(&bytes, Some(&path))?;
        let mut sizes = Sizes::default();
        blocks(&result.document.blocks, &mut sizes);
        println!(
            "{}",
            serde_json::json!({"path":path,"sizes":sizes,"bytes_per_slot":{"block":std::mem::size_of::<Block>(),"inline":std::mem::size_of::<Inline>(),"cell":std::mem::size_of::<Cell>(),"source":std::mem::size_of::<Source>()}})
        );
    }
    Ok(())
}
