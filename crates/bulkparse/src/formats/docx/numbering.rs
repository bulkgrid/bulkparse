use super::{attr, child, w, xml};
use crate::{Block, BlockContent, Context, Error};
use roxmltree::Node;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone)]
struct Level {
    start: u64,
    format: String,
    pattern: Option<String>,
    style: Option<String>,
    restart: Option<usize>,
}
fn level(node: Node<'_, '_>) -> Level {
    let val = |name| child(node, name).and_then(|n| attr(n, "val"));
    Level {
        start: val("start").and_then(|v| v.parse().ok()).unwrap_or(1),
        format: val("numFmt").unwrap_or("decimal").into(),
        pattern: val("lvlText").map(str::to_owned),
        style: val("pStyle").map(str::to_owned),
        restart: val("lvlRestart").and_then(|v| v.parse().ok()),
    }
}
#[derive(Default)]
pub(super) struct Numbering {
    levels: HashMap<u32, HashMap<usize, Arc<Level>>>,
    counts: HashMap<(u32, usize), u64>,
}
pub(super) struct Marker {
    pub id: u32,
    pub level: usize,
    pub start: Option<u64>,
}
impl Numbering {
    pub fn parse(text: Option<&str>, ctx: &Context<'_>) -> Result<Self, Error> {
        let Some(text) = text else {
            return Ok(Self::default());
        };
        let doc = xml(text, ctx)?;
        let mut abstracts = HashMap::new();
        for n in doc
            .root_element()
            .children()
            .filter(|n| w(*n, "abstractNum"))
        {
            let Some(id) = attr(n, "abstractNumId").and_then(|v| v.parse::<u32>().ok()) else {
                continue;
            };
            // Linked numbering styles require additional resolution. Leave such
            // instances unresolved so use produces an explicit partial result.
            if child(n, "numStyleLink").is_some() {
                continue;
            }
            let levels: HashMap<_, _> = n
                .children()
                .filter(|n| w(*n, "lvl"))
                .filter_map(|n| {
                    let index = attr(n, "ilvl")?.parse::<usize>().ok()?;
                    (index <= 8).then(|| (index, Arc::new(level(n))))
                })
                .collect();
            abstracts.insert(id, levels);
        }
        let mut result = Self::default();
        for n in doc.root_element().children().filter(|n| w(*n, "num")) {
            let Some(id) = attr(n, "numId").and_then(|v| v.parse::<u32>().ok()) else {
                continue;
            };
            let Some(base) = child(n, "abstractNumId")
                .and_then(|n| attr(n, "val"))
                .and_then(|v| v.parse::<u32>().ok())
                .and_then(|id| abstracts.get(&id))
            else {
                continue;
            };
            let mut levels = base.clone();
            for ov in n.children().filter(|n| w(*n, "lvlOverride")) {
                let Some(index) = attr(ov, "ilvl").and_then(|v| v.parse::<usize>().ok()) else {
                    continue;
                };
                if index > 8 {
                    continue;
                }
                if let Some(n) = child(ov, "lvl") {
                    levels.insert(index, Arc::new(level(n)));
                }
                if let Some(start) = child(ov, "startOverride")
                    .and_then(|n| attr(n, "val"))
                    .and_then(|v| v.parse().ok())
                {
                    if let Some(level) = levels.get_mut(&index) {
                        Arc::make_mut(level).start = start;
                    }
                }
            }
            result.levels.insert(id, levels);
        }
        Ok(result)
    }
    pub fn marker(
        &mut self,
        id: u32,
        explicit_level: Option<usize>,
        styles: &[String],
    ) -> Result<Option<Marker>, &'static str> {
        if id == 0 {
            return Ok(None);
        }
        let levels = self
            .levels
            .get(&id)
            .ok_or("Numbering definition is missing or uses an unsupported style link.")?;
        let index = explicit_level
            .or_else(|| {
                styles.iter().find_map(|style| {
                    levels
                        .iter()
                        .filter(|(_, l)| l.style.as_ref() == Some(style))
                        .map(|(i, _)| *i)
                        .min()
                })
            })
            .unwrap_or(0);
        if index > 8 {
            return Err("List level is outside the supported OOXML range.");
        }
        let level = levels
            .get(&index)
            .ok_or("List level definition is missing.")?;
        if level.format == "none" {
            return Ok(None);
        }
        let expected_pattern = format!("%{}.", index + 1);
        if !matches!(level.format.as_str(), "decimal" | "bullet")
            || (level.format == "decimal"
                && level
                    .pattern
                    .as_ref()
                    .is_some_and(|p| p != &expected_pattern))
        {
            return Err("Custom list markers are not represented yet; paragraph text is retained.");
        }
        // Word's counters belong to numId, not to a contiguous block of paragraphs.
        // A table or prose interruption therefore does not restart the sequence.
        for (&other, def) in levels {
            if other > index && def.restart != Some(0) && def.restart.is_none_or(|r| index + 1 == r)
            {
                self.counts.remove(&(id, other));
            }
        }
        let number = self
            .counts
            .entry((id, index))
            .and_modify(|n| *n = n.saturating_add(1))
            .or_insert(level.start);
        Ok(Some(Marker {
            id,
            level: index,
            start: (level.format == "decimal").then_some(*number),
        }))
    }
}

pub(super) struct Item {
    pub block: Block,
    pub marker: Marker,
}

pub(super) fn lists(items: Vec<Item>, ctx: &mut Context<'_>) -> Result<Vec<Block>, Error> {
    fn build(
        input: &mut std::iter::Peekable<std::vec::IntoIter<Item>>,
        ctx: &mut Context<'_>,
    ) -> Result<Block, Error> {
        let first = input.next().expect("nonempty list input");
        let source = first.block.source.clone();
        let key = first.marker;
        let mut items = vec![vec![first.block]];
        while let Some(next) = input.peek() {
            if next.marker.level < key.level {
                break;
            }
            if next.marker.level > key.level {
                if next.marker.level > key.level + 1 {
                    ctx.diagnostic(
                        "docx_list_level_gap",
                        "List skips an ancestor level; nesting is attached to the preceding item.",
                        next.block.source.clone(),
                        true,
                    );
                }
                items.last_mut().unwrap().push(build(input, ctx)?);
            } else if next.marker.id == key.id
                && next.marker.start == key.start.map(|n| n.saturating_add(items.len() as u64))
            {
                items.push(vec![input.next().unwrap().block]);
            } else {
                break;
            }
        }
        ctx.block(
            BlockContent::List {
                start: key.start,
                items,
            },
            source,
        )
    }
    let mut input = items.into_iter().peekable();
    let mut out = Vec::new();
    while let Some(first) = input.peek() {
        if first.marker.level > 0 {
            ctx.diagnostic(
                "docx_list_level_gap",
                "List starts without its ancestor level; content is retained at the root.",
                first.block.source.clone(),
                true,
            );
        }
        out.push(build(&mut input, ctx)?);
    }
    Ok(out)
}
