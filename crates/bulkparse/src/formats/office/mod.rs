//! BulkParse-owned document readers. Container/encoding crates do not parse documents.
use crate::{Block, BlockContent, Cell, Context, Error, Format, Inline, Source};
use roxmltree::{Document, Node, ParsingOptions};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{Cursor, Read};
mod chart;
mod css;
mod edge_table;
mod epub;
mod layout;
mod legacy;
mod math;
mod odf;
mod ppt_styles;
mod pptx;
pub(super) mod preview;
mod rtf;
mod rtf_tree;
mod spreadsheet;
mod style;
mod word_aux;
mod word_styles;
mod word_table;
mod xls_text;
mod xlsx;

fn emit(
    out: &mut Vec<Block>,
    block: Block,
    output: &mut Option<&mut crate::render::Writer<'_>>,
) -> Result<(), Error> {
    if let Some(writer) = output.as_deref_mut() {
        writer.block(&block)?;
    } else {
        out.push(block);
    }
    Ok(())
}

pub(super) fn write(
    format: Format,
    bytes: &[u8],
    ctx: &mut Context<'_>,
    output: &mut crate::render::Writer<'_>,
) -> Result<(), Error> {
    match format {
        Format::Xls => {
            legacy::write_xls(bytes, ctx, output)?;
        }
        Format::Xlsb => {
            spreadsheet::xlsb_output(bytes, ctx, Some(output))?;
        }
        Format::Xlsx => {
            xlsx::parse_output(bytes, ctx, Some(output))?;
        }
        Format::Ods => {
            odf::parse_output(format, bytes, ctx, Some(output))?;
        }
        _ => {
            output.blocks(&parse(format, bytes, ctx)?)?;
        }
    }
    Ok(())
}
#[cfg(not(target_arch = "wasm32"))]
mod xml_parts;
mod xml_stream;

pub(super) fn bad(e: impl std::fmt::Display) -> Error {
    Error::Malformed(e.to_string())
}
pub(super) fn named(n: Node<'_, '_>, name: &str) -> bool {
    n.is_element() && n.tag_name().name() == name
}
pub(super) fn child<'a, 'b>(n: Node<'a, 'b>, name: &str) -> Option<Node<'a, 'b>> {
    n.children().find(|n| named(*n, name))
}
pub(super) fn attr<'a>(n: Node<'a, '_>, name: &str) -> Option<&'a str> {
    n.attributes()
        .find(|a| a.name() == name && a.namespace().is_some())
        .or_else(|| n.attributes().find(|a| a.name() == name))
        .map(|a| a.value())
}
pub(super) fn content(n: Node<'_, '_>) -> String {
    n.descendants()
        .filter(Node::is_text)
        .filter_map(|n| n.text())
        .collect()
}
pub(super) fn xml<'a>(s: &'a str, ctx: &mut Context<'_>) -> Result<Document<'a>, Error> {
    let doc = Document::parse_with_options(
        s,
        ParsingOptions {
            allow_dtd: false,
            nodes_limit: ctx.options.limits.max_nodes.min(u32::MAX as usize) as u32,
            ..Default::default()
        },
    )
    .map_err(|e| {
        if matches!(e, roxmltree::Error::NodesLimitReached) {
            Error::Limit("max_nodes")
        } else {
            bad(e)
        }
    })?;
    for node in doc.descendants() {
        ctx.node()?;
        ctx.depth(
            node.ancestors()
                .take(ctx.options.limits.max_depth.saturating_add(2))
                .count(),
        )?;
    }
    Ok(doc)
}
pub(super) struct Package<'a> {
    zip: zip::ZipArchive<Cursor<&'a [u8]>>,
    remaining: usize,
}
impl<'a> Package<'a> {
    pub fn new(bytes: &'a [u8], ctx: &Context<'_>) -> Result<Self, Error> {
        let zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(bad)?;
        if zip.len() > ctx.options.limits.max_archive_entries {
            return Err(Error::Limit("max_archive_entries"));
        }
        let mut names = HashSet::new();
        for name in zip.file_names() {
            if name.contains(['\\', '\0'])
                || name.starts_with('/')
                || name.split('/').any(|s| s == "..")
                || !names.insert(name)
            {
                return Err(bad("unsafe or duplicate archive member"));
            }
        }
        Ok(Self {
            zip,
            remaining: ctx.options.limits.max_expanded_bytes,
        })
    }
    pub fn has(&self, name: &str) -> bool {
        self.zip.file_names().any(|n| n == name)
    }
    pub fn read_bytes(&mut self, name: &str) -> Result<Option<Vec<u8>>, Error> {
        let mut f = match self.zip.by_name(name) {
            Ok(f) => f,
            Err(zip::result::ZipError::FileNotFound) => return Ok(None),
            Err(e) => return Err(bad(e)),
        };
        if f.size() > self.remaining as u64 {
            return Err(Error::Limit("max_expanded_bytes"));
        }
        let mut out = Vec::new();
        (&mut f)
            .take((self.remaining as u64).saturating_add(1))
            .read_to_end(&mut out)?;
        if out.len() > self.remaining {
            return Err(Error::Limit("max_expanded_bytes"));
        }
        self.remaining -= out.len();
        Ok(Some(out))
    }
    pub fn read(&mut self, name: &str) -> Result<Option<String>, Error> {
        self.read_bytes(name)?
            .map(xml_stream::owned_utf8)
            .transpose()
    }
    /// Read XML with an early, streaming resource check before allocating a DOM.
    pub fn read_xml(&mut self, name: &str, ctx: &Context<'_>) -> Result<Option<String>, Error> {
        let mut f = match self.zip.by_name(name) {
            Ok(f) => f,
            Err(zip::result::ZipError::FileNotFound) => return Ok(None),
            Err(e) => return Err(bad(e)),
        };
        if f.size() > self.remaining as u64 {
            return Err(Error::Limit("max_expanded_bytes"));
        }
        // Small parts have a modest allocation ceiling. For larger parts, scan
        // without retaining bytes: rejected documents never allocate their XML
        // string or the DOM's source-sized capacity estimates. Accepted large
        // parts are decompressed again, trading some CPU for a lower peak.
        if f.size() > 256 * 1024 {
            xml_stream::check(&mut f, self.remaining, ctx)?;
        }
        drop(f);
        self.read(name)
    }
    pub fn required_xml(&mut self, name: &str, ctx: &Context<'_>) -> Result<String, Error> {
        self.read_xml(name, ctx)?
            .ok_or_else(|| bad(format!("missing package part {name}")))
    }
    pub fn required(&mut self, name: &str) -> Result<String, Error> {
        self.read(name)?
            .ok_or_else(|| bad(format!("missing package part {name}")))
    }
    pub fn rels(
        &mut self,
        part: &str,
        ctx: &mut Context<'_>,
    ) -> Result<HashMap<String, Rel>, Error> {
        let path = match part.rsplit_once('/') {
            Some((p, n)) => format!("{p}/_rels/{n}.rels"),
            None if part.is_empty() => "_rels/.rels".into(),
            None => format!("_rels/{part}.rels"),
        };
        let Some(s) = self.read_xml(&path, ctx)? else {
            return Ok(HashMap::new());
        };
        let d = xml(&s, ctx)?;
        let mut result = HashMap::new();
        for n in d
            .root_element()
            .children()
            .filter(|n| named(*n, "Relationship"))
        {
            let (Some(id), Some(target), Some(kind)) = (
                n.attribute("Id"),
                n.attribute("Target"),
                n.attribute("Type"),
            ) else {
                return Err(bad("incomplete relationship"));
            };
            let rel = Rel {
                target: target.into(),
                kind: kind.into(),
                external: n.attribute("TargetMode") == Some("External"),
            };
            if result.insert(id.into(), rel).is_some() {
                return Err(bad("duplicate relationship id"));
            }
        }
        Ok(result)
    }
    pub fn main(&mut self, fallback: &str, ctx: &mut Context<'_>) -> Result<String, Error> {
        let rels = self.rels("", ctx)?;
        let mut main = rels
            .values()
            .filter(|r| r.kind.ends_with("/officeDocument"));
        let Some(rel) = main.next() else {
            return Ok(fallback.into());
        };
        if main.next().is_some() || rel.external {
            return Err(bad("ambiguous or external main document"));
        }
        resolve("", &rel.target)
    }
}
#[derive(Clone)]
pub(super) struct Rel {
    pub target: String,
    pub kind: String,
    pub external: bool,
}
pub(super) fn resolve(base: &str, target: &str) -> Result<String, Error> {
    if target.contains(['\\', '\0', ':']) {
        return Err(bad("invalid internal package target"));
    }
    let mut parts: Vec<&str> = if target.starts_with('/') {
        vec![]
    } else {
        base.rsplit_once('/')
            .map(|(p, _)| p.split('/').collect())
            .unwrap_or_default()
    };
    for p in target.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err(bad("package target escapes root"));
                }
            }
            p => parts.push(p),
        }
    }
    if parts.is_empty() {
        return Err(bad("empty package target"));
    }
    Ok(parts.join("/"))
}
pub(super) fn linked(rels: &HashMap<String, Rel>, id: &str, part: &str) -> Result<String, Error> {
    let r = rels.get(id).ok_or_else(|| bad("missing relationship"))?;
    if r.external {
        return Err(bad("external content part"));
    }
    resolve(part, &r.target)
}
pub(super) fn warn(ctx: &mut Context<'_>, code: &str, msg: &str, src: &Source) {
    ctx.diagnostic(code, msg, src.clone(), true);
}
pub(super) fn para(text: String, src: &Source, ctx: &mut Context<'_>) -> Result<Block, Error> {
    ctx.text(&text)?;
    ctx.block(
        BlockContent::Paragraph {
            inlines: vec![Inline::text(text)],
        },
        src.clone(),
    )
}
pub(super) fn heading(
    text: String,
    level: u8,
    src: &Source,
    ctx: &mut Context<'_>,
) -> Result<Block, Error> {
    ctx.text(&text)?;
    ctx.block(
        BlockContent::Heading {
            level,
            inlines: vec![Inline::text(text)],
        },
        src.clone(),
    )
}
pub(super) fn count(n: Option<&str>, default: usize) -> Result<usize, Error> {
    n.map(|s| s.parse().map_err(|_| bad("invalid repeat/span/count")))
        .unwrap_or(Ok(default))
}
pub(super) fn table(
    grid: BTreeMap<(u32, u32), String>,
    merges: Vec<(u32, u32, u32, u32)>,
    src: &Source,
    ctx: &mut Context<'_>,
) -> Result<Option<Block>, Error> {
    table_output(grid, merges, src, ctx, None, None)
}
pub(super) fn table_output(
    grid: BTreeMap<(u32, u32), String>,
    merges: Vec<(u32, u32, u32, u32)>,
    src: &Source,
    ctx: &mut Context<'_>,
    output: Option<&mut crate::render::Writer<'_>>,
    prefix: Option<&mut Vec<Block>>,
) -> Result<Option<Block>, Error> {
    table_output_with(grid, merges, src, ctx, output, prefix, |_, _| Ok(()))
}

/// The decoration hook changes inline content only, without changing layout or
/// consuming node/cell budgets. This lets XLSX decorate one completed row before
/// it is written, while the structured path retains its original budget order.
pub(super) fn table_output_with(
    grid: BTreeMap<(u32, u32), String>,
    merges: Vec<(u32, u32, u32, u32)>,
    src: &Source,
    ctx: &mut Context<'_>,
    output: Option<&mut crate::render::Writer<'_>>,
    prefix: Option<&mut Vec<Block>>,
    decorate: impl FnMut(&mut [Cell], &mut Context<'_>) -> Result<(), Error>,
) -> Result<Option<Block>, Error> {
    // Plan paragraph and grid-size budgets before allocating their model.
    // Recovery must not pay for a nearly complete table that will be discarded.
    // Merged ranges retain the existing validation/error order below; their
    // bounding-box cap is checked before allocating cell paragraphs as well.
    let mut budget = ctx.budget_probe();
    for text in grid.values().filter(|text| !text.is_empty()) {
        budget.text(text)?;
        budget.node()?;
    }
    if !grid.is_empty() {
        let minr = grid
            .keys()
            .map(|p| p.0)
            .chain(merges.iter().map(|p| p.0))
            .min()
            .unwrap();
        let maxr = grid
            .keys()
            .map(|p| p.0)
            .chain(merges.iter().map(|p| p.2))
            .max()
            .unwrap();
        let minc = grid
            .keys()
            .map(|p| p.1)
            .chain(merges.iter().map(|p| p.1))
            .min()
            .unwrap();
        let maxc = grid
            .keys()
            .map(|p| p.1)
            .chain(merges.iter().map(|p| p.3))
            .max()
            .unwrap();
        let slots = u64::from(maxr - minr + 1) * u64::from(maxc - minc + 1);
        if slots > ctx.options.limits.max_table_cells as u64 {
            return Err(Error::Limit("max_table_cells"));
        }
        if merges.is_empty() {
            budget.account_cells(slots as usize)?;
            budget.node()?;
        }
    }
    if output.is_none() {
        let mut cells = BTreeMap::new();
        for (position, text) in grid {
            let source = Source {
                row: Some(position.0),
                cell: Some(position.1),
                ..src.clone()
            };
            let blocks = if text.is_empty() {
                vec![]
            } else {
                vec![para(text, &source, ctx)?]
            };
            cells.insert(position, blocks);
        }
        return structured_table_output_with(
            cells,
            merges,
            src,
            ctx,
            0,
            TableOutput { output, prefix },
            decorate,
        );
    }
    if merges.is_empty() {
        return write_unmerged_values(grid, src, ctx, output.unwrap(), prefix, decorate);
    }
    // The writer keeps compact values until each row is materialized. The
    // structured path preserves its allocation pattern: compact staging reduced
    // isolated allocations but increased persistent-worker memory retention.
    let mut cells = BTreeMap::new();
    for (position, text) in grid {
        let id = if text.is_empty() {
            0
        } else {
            ctx.text(&text)?;
            ctx.reserve_block()?
        };
        cells.insert(
            position,
            PendingText {
                text,
                id,
                covered: Vec::new(),
            },
        );
    }
    structured_table_output_with(
        cells,
        merges,
        src,
        ctx,
        0,
        TableOutput { output, prefix },
        decorate,
    )
}

/// Consume ordered unmerged values directly. No second tree of pending cells is
/// needed, and traversing the input once avoids a tree lookup for each grid slot.
fn write_unmerged_values(
    grid: BTreeMap<(u32, u32), String>,
    src: &Source,
    ctx: &mut Context<'_>,
    writer: &mut crate::render::Writer<'_>,
    prefix: Option<&mut Vec<Block>>,
    mut decorate: impl FnMut(&mut [Cell], &mut Context<'_>) -> Result<(), Error>,
) -> Result<Option<Block>, Error> {
    let Some((&(minr, _), _)) = grid.first_key_value() else {
        return Ok(None);
    };
    let maxr = grid.last_key_value().unwrap().0 .0;
    let (mut minc, mut maxc) = (u32::MAX, 0);
    let mut next_id = None;
    // Reserve all paragraph IDs before the table and decorations, retaining the
    // same budget/error order and block numbering as the structured reader.
    for (&(_, c), text) in &grid {
        minc = minc.min(c);
        maxc = maxc.max(c);
        if !text.is_empty() {
            ctx.text(text)?;
            let id = ctx.reserve_block()?;
            next_id.get_or_insert(id);
        }
    }
    let width = (maxc - minc + 1) as usize;
    let slots = u64::from(maxr - minr + 1) * u64::from(maxc - minc + 1);
    ctx.account_cells(slots as usize)?;
    drop(ctx.block(
        BlockContent::Table {
            rows: Vec::new(),
            header_rows: 0,
        },
        src.clone(),
    )?);
    if let Some(prefix) = prefix {
        for block in prefix.drain(..) {
            writer.block(&block)?;
        }
    }
    let native = src.page.is_some();
    writer.table_start(native)?;
    let mut values = grid.into_iter().peekable();
    let mut next_id = next_id.unwrap_or(0);
    let mut row = Vec::with_capacity(width);
    for r in minr..=maxr {
        ctx.check()?;
        row.clear();
        for c in minc..=maxc {
            let source = Source {
                row: Some(r),
                cell: Some(c),
                ..src.clone()
            };
            let text = if values
                .peek()
                .is_some_and(|(position, _)| *position == (r, c))
            {
                values.next().unwrap().1
            } else {
                String::new()
            };
            let blocks = if text.is_empty() {
                Vec::new()
            } else {
                let block = Block {
                    id: crate::block_id(next_id),
                    source: source.clone(),
                    content: BlockContent::Paragraph {
                        inlines: vec![Inline::text(text)],
                    },
                };
                next_id += 1;
                vec![block]
            };
            row.push(Cell {
                blocks,
                row_span: 1,
                col_span: 1,
                source,
            });
        }
        decorate(&mut row, ctx)?;
        writer.table_row(
            &row,
            (r - minr) as usize,
            0,
            width,
            native,
            src.part.is_none(),
        )?;
    }
    writer.table_end(native)?;
    Ok(None)
}

pub(super) fn structured_table(
    grid: BTreeMap<(u32, u32), Vec<Block>>,
    merges: Vec<(u32, u32, u32, u32)>,
    src: &Source,
    ctx: &mut Context<'_>,
) -> Result<Option<Block>, Error> {
    structured_table_output(grid, merges, src, ctx, 0, None, None)
}
pub(super) fn structured_table_output(
    grid: BTreeMap<(u32, u32), Vec<Block>>,
    merges: Vec<(u32, u32, u32, u32)>,
    src: &Source,
    ctx: &mut Context<'_>,
    header_rows: usize,
    output: Option<&mut crate::render::Writer<'_>>,
    prefix: Option<&mut Vec<Block>>,
) -> Result<Option<Block>, Error> {
    structured_table_output_with(
        grid,
        merges,
        src,
        ctx,
        header_rows,
        TableOutput { output, prefix },
        |_, _| Ok(()),
    )
}

trait TableContent: Default {
    fn is_empty(&self) -> bool;
    fn needs_native(&self) -> bool;
    fn append(&mut self, anchor: (u32, u32), position: (u32, u32), other: Self, src: &Source);
    fn into_blocks(self, position: (u32, u32), src: &Source) -> Vec<Block>;
}
impl TableContent for Vec<Block> {
    fn is_empty(&self) -> bool {
        self.is_empty()
    }
    fn needs_native(&self) -> bool {
        self.iter().any(|b| {
            !matches!(
                b.content,
                BlockContent::Paragraph { .. } | BlockContent::Heading { .. }
            )
        })
    }
    fn append(&mut self, _: (u32, u32), _: (u32, u32), other: Self, _: &Source) {
        self.extend(other);
    }
    fn into_blocks(self, _: (u32, u32), _: &Source) -> Vec<Block> {
        self
    }
}

#[derive(Default)]
struct PendingText {
    text: String,
    id: usize,
    // Allocated only for merged anchors containing values in covered cells.
    // Keep original coordinates for decorations and diagnostic provenance.
    covered: Vec<((u32, u32), PendingText)>,
}
impl TableContent for PendingText {
    fn is_empty(&self) -> bool {
        self.text.is_empty() && self.covered.is_empty()
    }
    fn needs_native(&self) -> bool {
        false
    }
    fn append(&mut self, _: (u32, u32), position: (u32, u32), other: Self, _: &Source) {
        self.covered.push((position, other));
    }
    fn into_blocks(self, position: (u32, u32), src: &Source) -> Vec<Block> {
        // Vec::push would reserve four large Blocks for an ordinary one-block
        // cell. That slack becomes hundreds of MiB when structured output keeps
        // millions of cells alive. Ordinary cells need exactly one slot.
        let mut blocks =
            Vec::with_capacity(usize::from(!self.text.is_empty()) + self.covered.len());
        if !self.text.is_empty() {
            blocks.push(Block {
                id: crate::block_id(self.id),
                source: Source {
                    row: Some(position.0),
                    cell: Some(position.1),
                    ..src.clone()
                },
                content: BlockContent::Paragraph {
                    inlines: vec![Inline::text(self.text)],
                },
            });
        }
        for (position, covered) in self.covered {
            blocks.extend(covered.into_blocks(position, src));
        }
        blocks
    }
}

/// ODS has already interpreted styles and nested content at this point. Keep
/// the common single-paragraph cells compact; reconstruct their duplicated
/// source metadata and model wrappers only for the row being written.
enum PendingBlocks {
    Text(usize, String),
    Paragraph(usize, Vec<Inline>),
    Full(Vec<Block>),
}
impl Default for PendingBlocks {
    fn default() -> Self {
        Self::Full(Vec::new())
    }
}
impl PendingBlocks {
    fn new(mut blocks: Vec<Block>, position: (u32, u32), src: &Source) -> Self {
        if blocks.len() == 1 {
            let expected = Source {
                row: Some(position.0),
                cell: Some(position.1),
                ..src.clone()
            };
            let id = blocks[0].id.strip_prefix('b').and_then(|s| s.parse().ok());
            if let Some(id) = id.filter(|_| {
                blocks[0].source == expected
                    && matches!(blocks[0].content, BlockContent::Paragraph { .. })
            }) {
                let BlockContent::Paragraph { mut inlines } = blocks.pop().unwrap().content else {
                    unreachable!()
                };
                return if inlines.len() == 1 && matches!(inlines[0], Inline::Text { .. }) {
                    let Inline::Text { text } = inlines.pop().unwrap() else {
                        unreachable!()
                    };
                    Self::Text(id, text)
                } else {
                    Self::Paragraph(id, inlines)
                };
            }
        }
        Self::Full(blocks)
    }
}
impl TableContent for PendingBlocks {
    fn is_empty(&self) -> bool {
        matches!(self, Self::Full(b) if b.is_empty())
    }
    fn needs_native(&self) -> bool {
        matches!(self, Self::Full(b) if b.needs_native())
    }
    fn append(&mut self, anchor: (u32, u32), position: (u32, u32), other: Self, src: &Source) {
        let mut blocks = std::mem::take(self).into_blocks(anchor, src);
        blocks.extend(other.into_blocks(position, src));
        *self = Self::Full(blocks);
    }
    fn into_blocks(self, position: (u32, u32), src: &Source) -> Vec<Block> {
        let (id, inlines) = match self {
            Self::Full(blocks) => return blocks,
            Self::Text(id, text) => (id, vec![Inline::text(text)]),
            Self::Paragraph(id, inlines) => (id, inlines),
        };
        vec![Block {
            id: crate::block_id(id),
            source: Source {
                row: Some(position.0),
                cell: Some(position.1),
                ..src.clone()
            },
            content: BlockContent::Paragraph { inlines },
        }]
    }
}

struct TableOutput<'writer, 'sink, 'prefix> {
    output: Option<&'writer mut crate::render::Writer<'sink>>,
    prefix: Option<&'prefix mut Vec<Block>>,
}

fn structured_table_output_with<T: TableContent>(
    mut grid: BTreeMap<(u32, u32), T>,
    merges: Vec<(u32, u32, u32, u32)>,
    src: &Source,
    ctx: &mut Context<'_>,
    header_rows: usize,
    destination: TableOutput<'_, '_, '_>,
    mut decorate: impl FnMut(&mut [Cell], &mut Context<'_>) -> Result<(), Error>,
) -> Result<Option<Block>, Error> {
    let TableOutput { mut output, prefix } = destination;
    if grid.is_empty() {
        return Ok(None);
    }
    let minr = grid
        .keys()
        .map(|p| p.0)
        .chain(merges.iter().map(|p| p.0))
        .min()
        .unwrap();
    let minc = grid
        .keys()
        .map(|p| p.1)
        .chain(merges.iter().map(|p| p.1))
        .min()
        .unwrap();
    let maxr = grid
        .keys()
        .map(|p| p.0)
        .chain(merges.iter().map(|p| p.2))
        .max()
        .unwrap();
    let maxc = grid
        .keys()
        .map(|p| p.1)
        .chain(merges.iter().map(|p| p.3))
        .max()
        .unwrap();
    let slots = u64::from(maxr - minr + 1) * u64::from(maxc - minc + 1);
    if slots > ctx.options.limits.max_table_cells as u64 {
        return Err(Error::Limit("max_table_cells"));
    }
    let mut covered = HashSet::new();
    let mut spans = HashMap::new();
    for (r, c, er, ec) in merges {
        if r < minr || c < minc || er < r || ec < c {
            return Err(bad("invalid merged range"));
        }
        if (u64::from(er - r) + 1) * (u64::from(ec - c) + 1)
            > ctx.options.limits.max_table_cells as u64
        {
            return Err(Error::Limit("max_table_cells"));
        }
        for row in r..=er {
            for col in c..=ec {
                if !covered.insert((row, col)) {
                    return Err(bad("overlapping merged ranges"));
                }
                if (row, col) != (r, c) {
                    if let Some(blocks) = grid.remove(&(row, col)).filter(|b| !b.is_empty()) {
                        let source = Source {
                            row: Some(row),
                            cell: Some(col),
                            ..src.clone()
                        };
                        warn(ctx,"office_merge_content","Content stored in a covered cell retained inside the merged anchor, with its original block provenance.",&source);
                        grid.entry((r, c))
                            .or_default()
                            .append((r, c), (row, col), blocks, src);
                    }
                }
            }
        }
        spans.insert((r, c), (er - r + 1, ec - c + 1));
    }
    let height = (maxr - minr + 1) as usize;
    let width = (maxc - minc + 1) as usize;
    let header_rows = header_rows.min(height);
    let native = output.is_some()
        && ((header_rows == 0 && src.page.is_some())
            || header_rows > 1
            || spans.values().any(|&(rs, cs)| rs > 1 || cs > 1)
            || grid.values().any(TableContent::needs_native));
    if let Some(writer) = output.as_deref_mut() {
        // Charge the complete cell/table budget once, before decorations and
        // output, in the same order as the structured path. XLS recovery keeps
        // completed sheets, so a failed sheet must emit nothing. The streaming
        // row loop only checks cancellation; it must not charge these again.
        ctx.account_cells(slots as usize)?;
        drop(ctx.block(
            BlockContent::Table {
                rows: Vec::new(),
                header_rows,
            },
            src.clone(),
        )?);
        if let Some(prefix) = prefix {
            for block in prefix.drain(..) {
                writer.block(&block)?;
            }
        }
        writer.table_start(native)?;
    }
    let mut rows = if output.is_none() {
        Vec::with_capacity(height)
    } else {
        Vec::new()
    };
    for r in minr..=maxr {
        ctx.check()?;
        let mut row = if covered.is_empty() {
            Vec::with_capacity((maxc - minc + 1) as usize)
        } else {
            Vec::new()
        };
        for c in minc..=maxc {
            if output.is_none() {
                ctx.cell()?;
            }
            if covered.contains(&(r, c)) && !spans.contains_key(&(r, c)) {
                continue;
            }
            let s = Source {
                row: Some(r),
                cell: Some(c),
                ..src.clone()
            };
            let (rs, cs) = spans.get(&(r, c)).copied().unwrap_or((1, 1));
            let blocks = grid
                .remove(&(r, c))
                .unwrap_or_default()
                .into_blocks((r, c), src);
            row.push(Cell {
                blocks,
                row_span: rs,
                col_span: cs,
                source: s,
            });
        }
        if let Some(writer) = output.as_deref_mut() {
            decorate(&mut row, ctx)?;
            writer.table_row(
                &row,
                (r - minr) as usize,
                header_rows,
                width,
                native,
                src.page.is_some() && src.part.is_none(),
            )?;
        } else {
            rows.push(row)
        }
    }
    if let Some(writer) = output {
        writer.table_end(native)?;
        Ok(None)
    } else {
        let mut block = ctx.block(BlockContent::Table { rows, header_rows }, src.clone())?;
        // Structured XLSX decoration used to run after table construction had
        // returned. Release its layout maps first, preserving that lifetime and
        // avoiding overlap with rich-text/link allocations.
        drop(grid);
        drop(covered);
        drop(spans);
        if let BlockContent::Table { rows, .. } = &mut block.content {
            for row in rows {
                decorate(row, ctx)?;
            }
        }
        Ok(Some(block))
    }
}
pub fn detect(bytes: &[u8], ctx: &mut Context<'_>) -> Result<Option<Format>, Error> {
    if bytes.starts_with(b"{\\rtf") {
        return Ok(Some(Format::Rtf));
    }
    if bytes.starts_with(b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1") {
        return legacy::detect(bytes, ctx);
    }
    if !bytes.starts_with(b"PK\x03\x04") && !bytes.starts_with(b"PK\x05\x06") {
        return Ok(None);
    }
    let mut p = Package::new(bytes, ctx)?;
    if let Some(s) = p.read("mimetype")? {
        return Ok(match s.trim() {
            "application/epub+zip" => Some(Format::Epub),
            "application/vnd.oasis.opendocument.text" => Some(Format::Odt),
            "application/vnd.oasis.opendocument.spreadsheet" => Some(Format::Ods),
            "application/vnd.oasis.opendocument.presentation" => Some(Format::Odp),
            _ => None,
        });
    }
    // The root relationship identifies the actual main part. Inspect its root
    // before fallback content-type hints (which can be stale in producer files).
    let rels = p.rels("", ctx)?;
    if let Some(rel) = rels.values().find(|r| r.kind.ends_with("/officeDocument")) {
        if rel.external {
            return Err(bad("external main document"));
        }
        let main = resolve("", &rel.target)?;
        if main.ends_with(".xml") {
            if let Some(s) = p.read(&main)? {
                let d = xml(&s, ctx)?;
                let f = match d.root_element().tag_name().name() {
                    "presentation" => Some(Format::Pptx),
                    "workbook" => Some(Format::Xlsx),
                    "document" => Some(Format::Docx),
                    _ => None,
                };
                if f.is_some() {
                    return Ok(f);
                }
            }
        }
    }
    if let Some(s) = p.read("[Content_Types].xml")? {
        let d = xml(&s, ctx)?;
        let mut found = None;
        for n in d.descendants().filter(|n| named(*n, "Override")) {
            let t = n.attribute("ContentType").unwrap_or("");
            let f = if t.contains("wordprocessingml.document.main")
                || t.contains("ms-word.document.macroEnabled.main")
            {
                Some(Format::Docx)
            } else if t.contains("spreadsheetml.sheet.main")
                || t.contains("ms-excel.sheet.macroEnabled.main")
            {
                Some(Format::Xlsx)
            } else if t.contains("ms-excel.sheet.binary.macroEnabled.main") {
                Some(Format::Xlsb)
            } else if (t.contains("presentationml.") || t.contains("ms-powerpoint."))
                && t.contains("main+xml")
            {
                Some(Format::Pptx)
            } else {
                None
            };
            if let Some(f) = f {
                if found.is_some_and(|old| old != f) {
                    return Err(bad("ambiguous Office package"));
                }
                found = Some(f)
            }
        }
        if found.is_some() {
            return Ok(found);
        }
    }
    for (path, f) in [
        ("word/document.xml", Format::Docx),
        ("xl/workbook.xml", Format::Xlsx),
        ("xl/workbook.bin", Format::Xlsb),
        ("ppt/presentation.xml", Format::Pptx),
    ] {
        if p.has(path) {
            return Ok(Some(f));
        }
    }
    Ok(None)
}
pub fn parse(format: Format, bytes: &[u8], ctx: &mut Context<'_>) -> Result<Vec<Block>, Error> {
    match format {
        Format::Xlsx => xlsx::parse(bytes, ctx),
        Format::Pptx => pptx::parse(bytes, ctx),
        Format::Odt | Format::Ods | Format::Odp => odf::parse(format, bytes, ctx),
        Format::Epub => epub::parse(bytes, ctx),
        Format::Rtf => rtf::parse(bytes, ctx),
        Format::Doc | Format::Ppt | Format::Xls => legacy::parse(format, bytes, ctx),
        Format::Xlsb => spreadsheet::xlsb(bytes, ctx),
        _ => Err(Error::Unsupported("not an Office format".into())),
    }
}

#[cfg(test)]
mod table_budget_tests {
    use super::*;
    use crate::{CancellationToken, Options};

    #[test]
    fn merged_grid_limits_include_both_cells_and_merge_extent() {
        let mut options = Options::default();
        options.limits.max_table_cells = 4;
        let cancel = CancellationToken::default();
        for (position, merge) in [((1, 1), (1, 1, 3, 2)), ((4, 2), (1, 1, 2, 2))] {
            let grid = BTreeMap::from([(position, "value".to_owned())]);
            let mut ctx = Context::new(&options, &cancel);
            assert!(matches!(
                table(grid, vec![merge], &Source::default(), &mut ctx),
                Err(Error::Limit("max_table_cells"))
            ));
            // Rejection happens before spending the real context's text/nodes.
            assert!(ctx.fits(options.limits.max_nodes, 0, options.limits.max_text_bytes));
        }
    }

    #[test]
    fn merged_grid_preflight_keeps_error_precedence() {
        let mut options = Options::default();
        options.limits.max_table_cells = 1;
        options.limits.max_text_bytes = 1;
        let cancel = CancellationToken::default();
        let grid = BTreeMap::from([((1, 1), "value".to_owned())]);
        let mut ctx = Context::new(&options, &cancel);
        assert!(matches!(
            table(
                grid.clone(),
                vec![(1, 1, 2, 2)],
                &Source::default(),
                &mut ctx
            ),
            Err(Error::Limit("max_text_bytes"))
        ));
        options.limits.max_text_bytes = 10;
        // The old table builder checked the bounding-box cap before malformed
        // merge validation. Keep that order, even for an inverted merge.
        let mut ctx = Context::new(&options, &cancel);
        assert!(matches!(
            table(
                grid.clone(),
                vec![(3, 1, 2, 2)],
                &Source::default(),
                &mut ctx
            ),
            Err(Error::Limit("max_table_cells"))
        ));
        options.limits.max_table_cells = 10;
        let mut ctx = Context::new(&options, &cancel);
        assert!(matches!(
            table(grid, vec![(3, 1, 2, 2)], &Source::default(), &mut ctx),
            Err(Error::Malformed(_))
        ));
    }
}
