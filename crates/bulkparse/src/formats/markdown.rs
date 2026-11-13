use crate::{Block, BlockContent, Cell, Context, Error, Inline, Source};
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag};

struct Node<'a> {
    event: Event<'a>,
    children: Vec<Node<'a>>,
    range: [usize; 2],
}

pub fn parse(text: &str, ctx: &mut Context<'_>, source: Source) -> Result<Vec<Block>, Error> {
    ctx.text(text)?;
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut stack: Vec<Node<'_>> = Vec::new();
    let mut roots = Vec::new();
    for (event, range) in Parser::new_ext(text, options).into_offset_iter() {
        ctx.node()?;
        match event {
            Event::Start(_) => {
                ctx.depth(stack.len() + 1)?;
                stack.push(Node {
                    event,
                    children: vec![],
                    range: [range.start, range.end],
                });
            }
            Event::End(_) => {
                let mut node = stack
                    .pop()
                    .ok_or_else(|| Error::Malformed("unbalanced Markdown events".into()))?;
                node.range[1] = range.end;
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(node);
                } else {
                    roots.push(node);
                }
            }
            _ => {
                let node = Node {
                    event,
                    children: vec![],
                    range: [range.start, range.end],
                };
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(node);
                } else {
                    roots.push(node);
                }
            }
        }
    }
    blocks(&roots, ctx, &source)
}

fn located(node: &Node<'_>, source: &Source) -> Source {
    Source {
        byte_range: Some(node.range.into()),
        ..source.clone()
    }
}

fn blocks(nodes: &[Node<'_>], ctx: &mut Context<'_>, source: &Source) -> Result<Vec<Block>, Error> {
    let mut out = Vec::new();
    let mut pending = Vec::new();
    for node in nodes {
        let content = match &node.event {
            Event::Start(Tag::Paragraph) => Some(BlockContent::Paragraph {
                inlines: inlines(&node.children),
            }),
            Event::Start(Tag::Heading { level, .. }) => Some(BlockContent::Heading {
                level: *level as u8,
                inlines: inlines(&node.children),
            }),
            Event::Start(Tag::CodeBlock(kind)) => Some(BlockContent::Code {
                language: match kind {
                    CodeBlockKind::Fenced(s) => s.to_string(),
                    _ => String::new(),
                },
                text: plain(&node.children),
            }),
            Event::Start(Tag::BlockQuote(_)) => Some(BlockContent::Quote {
                blocks: blocks(&node.children, ctx, source)?,
            }),
            Event::Start(Tag::List(start)) => {
                let mut items = Vec::new();
                for item in &node.children {
                    items.push(blocks(&item.children, ctx, source)?);
                }
                Some(BlockContent::List {
                    start: *start,
                    items,
                })
            }
            Event::Start(Tag::Table(_)) => {
                let mut rows = Vec::new();
                for row in &node.children {
                    let mut cells = Vec::new();
                    for cell in &row.children {
                        ctx.cell()?;
                        let loc = located(cell, source);
                        cells.push(Cell {
                            blocks: vec![ctx.block(
                                BlockContent::Paragraph {
                                    inlines: inlines(&cell.children),
                                },
                                loc.clone(),
                            )?],
                            col_span: 1,
                            row_span: 1,
                            source: loc,
                        });
                    }
                    rows.push(cells);
                }
                Some(BlockContent::Table {
                    rows,
                    header_rows: 1,
                })
            }
            Event::Rule => Some(BlockContent::ThematicBreak),
            Event::Start(Tag::HtmlBlock) => {
                ctx.diagnostic(
                    "raw_html",
                    "HTML is preserved as inert source rather than executed or interpreted.",
                    located(node, source),
                    false,
                );
                Some(BlockContent::Raw {
                    media_type: "text/html".into(),
                    text: plain(&node.children),
                })
            }
            Event::Html(s) => Some(BlockContent::Raw {
                media_type: "text/html".into(),
                text: s.to_string(),
            }),
            _ => None,
        };
        if let Some(content) = content {
            if !pending.is_empty() {
                out.push(ctx.block(
                    BlockContent::Paragraph {
                        inlines: std::mem::take(&mut pending),
                    },
                    source.clone(),
                )?);
            }
            out.push(ctx.block(content, located(node, source))?);
        } else {
            pending.extend(inlines(std::slice::from_ref(node)));
        }
    }
    if !pending.is_empty() {
        out.push(ctx.block(BlockContent::Paragraph { inlines: pending }, source.clone())?);
    }
    Ok(out)
}

fn inlines(nodes: &[Node<'_>]) -> Vec<Inline> {
    let mut out = Vec::new();
    for node in nodes {
        match &node.event {
            Event::Text(s) => out.push(Inline::text(s.to_string())),
            Event::Code(s) => out.push(Inline::Code {
                text: s.to_string(),
            }),
            Event::InlineHtml(s) | Event::Html(s) => out.push(Inline::text(s.to_string())),
            Event::SoftBreak => out.push(Inline::text("\n")),
            Event::HardBreak => out.push(Inline::Break),
            Event::TaskListMarker(done) => {
                out.push(Inline::text(if *done { "[x] " } else { "[ ] " }))
            }
            Event::FootnoteReference(s) => out.push(Inline::text(format!("[^{s}]"))),
            Event::InlineMath(s) | Event::DisplayMath(s) => out.push(Inline::text(s.to_string())),
            Event::Start(Tag::Strong) => out.push(Inline::Strong {
                children: inlines(&node.children),
            }),
            Event::Start(Tag::Emphasis) => out.push(Inline::Emphasis {
                children: inlines(&node.children),
            }),
            Event::Start(Tag::Strikethrough) => out.push(Inline::Strike {
                children: inlines(&node.children),
            }),
            Event::Start(Tag::Link { dest_url, .. }) => out.push(Inline::Link {
                url: dest_url.to_string(),
                children: inlines(&node.children),
            }),
            Event::Start(Tag::Image { dest_url, .. }) => out.push(Inline::Image {
                url: dest_url.to_string(),
                alt: plain(&node.children),
            }),
            _ => out.extend(inlines(&node.children)),
        }
    }
    out
}

fn plain(nodes: &[Node<'_>]) -> String {
    let mut out = String::new();
    for node in nodes {
        match &node.event {
            Event::Text(s) | Event::Code(s) | Event::Html(s) | Event::InlineHtml(s) => {
                out.push_str(s)
            }
            Event::SoftBreak | Event::HardBreak => out.push('\n'),
            _ => out.push_str(&plain(&node.children)),
        }
    }
    out
}
