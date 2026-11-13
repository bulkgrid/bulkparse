use crate::{Block, BlockContent, Cell, Context, Error, Inline, Source};
use roxmltree::{Document, Node, ParsingOptions};
use std::collections::HashMap;
use std::io::{Cursor, Read};
mod auxiliary;
mod numbering;
mod table;
use numbering::{Item, Numbering};

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const W_STRICT: &str = "http://purl.oclc.org/ooxml/wordprocessingml/main";
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const R_STRICT: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships";

fn w(node: Node<'_, '_>, name: &str) -> bool {
    node.is_element()
        && node.tag_name().name() == name
        && matches!(node.tag_name().namespace(), Some(W | W_STRICT))
}
fn attr<'a>(node: Node<'a, '_>, name: &str) -> Option<&'a str> {
    node.attribute((W, name))
        .or_else(|| node.attribute((W_STRICT, name)))
        .or_else(|| node.attribute(name))
}
fn child<'a, 'b>(node: Node<'a, 'b>, name: &str) -> Option<Node<'a, 'b>> {
    node.children().find(|n| w(*n, name))
}
fn metadata(n: Node<'_, '_>) -> bool {
    [
        "bookmarkStart",
        "bookmarkEnd",
        "proofErr",
        "commentRangeStart",
        "commentRangeEnd",
        "moveFromRangeStart",
        "moveFromRangeEnd",
        "moveToRangeStart",
        "moveToRangeEnd",
        "permStart",
        "permEnd",
    ]
    .iter()
    .any(|name| w(n, name))
}
fn empty_block(b: &Block) -> bool {
    matches!(&b.content, BlockContent::Paragraph { inlines } if inlines.is_empty() ||
        inlines.iter().all(|i| matches!(i, Inline::Text {text} if text.trim().is_empty())))
}
const MC: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";
fn alternate<'a, 'b>(n: Node<'a, 'b>) -> Option<Node<'a, 'b>> {
    // Use a single branch, never both the modern shape and its VML fallback.
    n.children()
        .find(|c| {
            c.is_element()
                && c.tag_name().name() == "Choice"
                && c.descendants().any(|d| {
                    w(d, "txbxContent") || matches!(d.tag_name().name(), "oMath" | "oMathPara")
                })
        })
        .or_else(|| {
            n.children()
                .find(|c| c.is_element() && c.tag_name().name() == "Fallback")
        })
        .or_else(|| {
            n.children()
                .find(|c| c.is_element() && c.tag_name().name() == "Choice")
        })
}
fn drawing_text_key(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn text_boxes<'a, 'b>(n: Node<'a, 'b>, out: &mut Vec<Node<'a, 'b>>) {
    if w(n, "txbxContent") {
        out.push(n);
        return;
    }
    if n.tag_name().namespace() == Some(MC) && n.tag_name().name() == "AlternateContent" {
        if let Some(c) = alternate(n) {
            text_boxes(c, out);
        }
    } else {
        for c in n.children().filter(Node::is_element) {
            text_boxes(c, out);
        }
    }
}
fn malformed(error: impl std::fmt::Display) -> Error {
    Error::Malformed(error.to_string())
}

struct Package<'a> {
    zip: zip::ZipArchive<Cursor<&'a [u8]>>,
    remaining: usize,
}
impl<'a> Package<'a> {
    fn new(bytes: &'a [u8], ctx: &Context<'_>) -> Result<Self, Error> {
        let zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(malformed)?;
        if zip.len() > ctx.options.limits.max_archive_entries {
            return Err(Error::Limit("max_archive_entries"));
        }
        Ok(Self {
            zip,
            remaining: ctx.options.limits.max_expanded_bytes,
        })
    }
    fn read(&mut self, name: &str) -> Result<Option<String>, Error> {
        let mut file = match self.zip.by_name(name) {
            Ok(f) => f,
            Err(zip::result::ZipError::FileNotFound) => return Ok(None),
            Err(e) => return Err(malformed(e)),
        };
        if file.size() > self.remaining as u64 {
            return Err(Error::Limit("max_expanded_bytes"));
        }
        let mut bytes = Vec::new();
        (&mut file)
            .take((self.remaining as u64).saturating_add(1))
            .read_to_end(&mut bytes)?;
        if bytes.len() > self.remaining {
            return Err(Error::Limit("max_expanded_bytes"));
        }
        self.remaining -= bytes.len();
        Ok(Some(super::utf8(&bytes)?.to_owned()))
    }
}

pub fn is_docx(bytes: &[u8]) -> Result<bool, Error> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(malformed)?;
    if zip.by_name("word/document.xml").is_ok() {
        return Ok(true);
    }
    let mut types = match zip.by_name("[Content_Types].xml") {
        Ok(f) => f,
        Err(_) => return Ok(false),
    };
    let mut text = String::new();
    (&mut types).take(128 * 1024).read_to_string(&mut text)?;
    Ok(text.contains("wordprocessingml.document.main+xml")
        || text.contains("ms-word.document.macroEnabled.main+xml"))
}

fn xml<'a>(text: &'a str, ctx: &Context<'_>) -> Result<Document<'a>, Error> {
    let doc = Document::parse_with_options(
        text,
        ParsingOptions {
            allow_dtd: false,
            nodes_limit: ctx.options.limits.max_nodes.min(u32::MAX as usize) as u32,
            ..Default::default()
        },
    )
    .map_err(|error| match error {
        roxmltree::Error::NodesLimitReached => Error::Limit("max_nodes"),
        error => malformed(error),
    })?;
    // Iterative traversal; validate depth before our semantic recursion.
    let mut next = Some((doc.root(), 1));
    while let Some((node, depth)) = next {
        ctx.depth(depth)?;
        next = if let Some(child) = node.first_child() {
            Some((child, depth + 1))
        } else {
            let mut cursor = node;
            let mut level = depth;
            loop {
                if let Some(sibling) = cursor.next_sibling() {
                    break Some((sibling, level));
                }
                let Some(parent) = cursor.parent() else {
                    break None;
                };
                cursor = parent;
                level -= 1;
            }
        };
    }
    Ok(doc)
}

fn resolve(base: &str, target: &str) -> Result<String, Error> {
    if target.contains(['\\', '\0', ':']) {
        return Err(malformed("invalid package target"));
    }
    let combined = if target.starts_with('/') {
        target.to_string()
    } else {
        format!(
            "{}{target}",
            base.rsplit_once('/')
                .map(|(p, _)| format!("{p}/"))
                .unwrap_or_default()
        )
    };
    let mut parts = Vec::new();
    for part in combined.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err(malformed("package target escapes root"));
                }
            }
            _ => parts.push(part),
        }
    }
    Ok(parts.join("/"))
}
fn rels_name(part: &str) -> String {
    match part.rsplit_once('/') {
        Some((dir, name)) => format!("{dir}/_rels/{name}.rels"),
        None => format!("_rels/{part}.rels"),
    }
}

#[derive(Clone)]
struct Relationship {
    target: String,
    kind: String,
    external: bool,
}
fn relationships(
    text: Option<&str>,
    ctx: &Context<'_>,
) -> Result<HashMap<String, Relationship>, Error> {
    let Some(text) = text else {
        return Ok(HashMap::new());
    };
    let doc = xml(text, ctx)?;
    let mut out = HashMap::new();
    for node in doc.root_element().children().filter(Node::is_element) {
        if node.tag_name().name() != "Relationship" {
            continue;
        }
        if let (Some(id), Some(target), Some(kind)) = (
            node.attribute("Id"),
            node.attribute("Target"),
            node.attribute("Type"),
        ) {
            out.insert(
                id.to_owned(),
                Relationship {
                    target: target.to_owned(),
                    kind: kind.to_owned(),
                    external: node.attribute("TargetMode") == Some("External"),
                },
            );
        }
    }
    Ok(out)
}

#[derive(Clone, Copy, Default)]
struct RunStyle {
    bold: Option<bool>,
    italic: Option<bool>,
    strike: Option<bool>,
}
#[derive(Clone, Copy, Default)]
struct RunFormat {
    bold: bool,
    italic: bool,
    strike: bool,
}
impl RunFormat {
    fn apply(&mut self, props: RunStyle, toggle: bool) {
        for (current, value) in [
            (&mut self.bold, props.bold),
            (&mut self.italic, props.italic),
            (&mut self.strike, props.strike),
        ] {
            if let Some(on) = value {
                if toggle {
                    if on {
                        *current = !*current;
                    }
                } else {
                    *current = on;
                }
            }
        }
    }
}
fn run_style(props: Option<Node<'_, '_>>) -> RunStyle {
    let value = |name| {
        props
            .and_then(|p| child(p, name))
            .map(|n| !matches!(attr(n, "val"), Some("0" | "false" | "off")))
    };
    RunStyle {
        bold: value("b"),
        italic: value("i"),
        strike: value("strike"),
    }
}
fn num_props(props: Option<Node<'_, '_>>) -> (Option<u32>, Option<usize>, bool) {
    let num = props.and_then(|p| child(p, "numPr"));
    (
        num.and_then(|n| child(n, "numId"))
            .and_then(|n| attr(n, "val"))
            .and_then(|v| v.parse().ok()),
        num.and_then(|n| child(n, "ilvl"))
            .and_then(|n| attr(n, "val"))
            .and_then(|v| v.parse().ok()),
        num.is_some(),
    )
}
#[derive(Default)]
struct Style {
    parent: Option<String>,
    heading: Option<u8>,
    name: String,
    num_id: Option<u32>,
    num_level: Option<usize>,
    has_num: bool,
    run: RunStyle,
    default_paragraph: bool,
}
fn styles(text: Option<&str>, ctx: &Context<'_>) -> Result<HashMap<String, Style>, Error> {
    let Some(text) = text else {
        return Ok(HashMap::new());
    };
    let doc = xml(text, ctx)?;
    let mut out = HashMap::new();
    for node in doc.root_element().children().filter(|n| w(*n, "style")) {
        let Some(id) = attr(node, "styleId") else {
            continue;
        };
        let heading = child(node, "pPr")
            .and_then(|p| child(p, "outlineLvl"))
            .and_then(|n| attr(n, "val"))
            .and_then(|v| v.parse::<u8>().ok())
            .filter(|n| *n < 6)
            .map(|n| n + 1)
            .or_else(|| {
                child(node, "name")
                    .and_then(|n| attr(n, "val"))
                    .and_then(heading_name)
            });
        out.insert(
            id.into(),
            Style {
                parent: child(node, "basedOn")
                    .and_then(|n| attr(n, "val"))
                    .map(str::to_owned),
                heading,
                name: child(node, "name")
                    .and_then(|n| attr(n, "val"))
                    .unwrap_or(id)
                    .to_owned(),
                num_id: num_props(child(node, "pPr")).0,
                num_level: num_props(child(node, "pPr")).1,
                has_num: num_props(child(node, "pPr")).2,
                run: run_style(child(node, "rPr")),
                default_paragraph: attr(node, "type") == Some("paragraph")
                    && matches!(attr(node, "default"), Some("1" | "true")),
            },
        );
    }
    Ok(out)
}
fn heading_name(name: &str) -> Option<u8> {
    let normalized = name.to_ascii_lowercase().replace(' ', "");
    normalized
        .strip_prefix("heading")
        .and_then(|n| n.parse::<u8>().ok())
        .filter(|n| (1..=6).contains(n))
}

struct Walker<'a, 'b> {
    ctx: &'a mut Context<'b>,
    part: String,
    relationships: HashMap<String, Relationship>,
    styles: HashMap<String, Style>,
    numbering: Numbering,
    note_refs: Vec<(String, String)>,
}
impl Walker<'_, '_> {
    fn source(&self, node: Node<'_, '_>) -> Source {
        let range = node.range();
        Source {
            part: Some(self.part.clone().into()),
            byte_range: Some([range.start, range.end].into()),
            ..Default::default()
        }
    }
    fn warn(&mut self, code: &str, message: &str) {
        self.ctx.diagnostic(
            code,
            message,
            Source {
                part: Some(self.part.clone().into()),
                ..Default::default()
            },
            true,
        );
    }
    fn heading(&self, node: Node<'_, '_>) -> Option<u8> {
        let props = child(node, "pPr")?;
        if let Some(level) = child(props, "outlineLvl")
            .and_then(|n| attr(n, "val"))
            .and_then(|s| s.parse::<u8>().ok())
            .filter(|n| *n < 6)
        {
            return Some(level + 1);
        }
        let mut id = child(props, "pStyle").and_then(|n| attr(n, "val"))?;
        for _ in 0..self.ctx.options.limits.max_depth {
            if let Some(level) = heading_name(id) {
                return Some(level);
            }
            let style = self.styles.get(id)?;
            if let Some(level) = style.heading {
                return Some(level);
            }
            id = style.parent.as_deref()?;
        }
        None
    }
    fn chain(&self, id: Option<&str>) -> Result<Vec<String>, Error> {
        let mut chain = Vec::new();
        let mut next = id;
        while let Some(id) = next {
            if chain.len() >= self.ctx.options.limits.max_depth {
                return Err(Error::Limit("max_depth"));
            }
            if chain.iter().any(|s| s == id) {
                return Err(malformed("cyclic DOCX style inheritance"));
            }
            chain.push(id.to_owned());
            next = self.styles.get(id).and_then(|s| s.parent.as_deref());
        }
        Ok(chain)
    }
    fn members<'a, 'b>(
        &mut self,
        node: Node<'a, 'b>,
        depth: usize,
    ) -> Result<Vec<Node<'a, 'b>>, Error> {
        self.ctx.depth(depth)?;
        let mut members = Vec::new();
        for n in node.children().filter(Node::is_element) {
            self.ctx.node()?;
            if w(n, "sdt") {
                if let Some(content) = child(n, "sdtContent") {
                    members.extend(self.members(content, depth + 1)?);
                } else {
                    self.warn(
                        "docx_content_control",
                        "Content control has no supported content container.",
                    );
                }
            } else if n.tag_name().namespace() == Some(MC)
                && n.tag_name().name() == "AlternateContent"
            {
                if let Some(c) = alternate(n) {
                    members.extend(self.members(c, depth + 1)?);
                }
            } else if w(n, "del") || w(n, "moveFrom") {
                // Read the current revision, excluding deleted/moved-away text.
            } else if w(n, "sdtContent") || w(n, "customXml") || w(n, "ins") || w(n, "moveTo") {
                members.extend(self.members(n, depth + 1)?);
            } else {
                members.push(n);
            }
        }
        Ok(members)
    }
    fn blocks(&mut self, node: Node<'_, '_>, depth: usize) -> Result<Vec<Block>, Error> {
        self.ctx.depth(depth)?;
        let mut out = Vec::new();
        let mut pending = Vec::new();
        for n in self.members(node, depth)? {
            if w(n, "p") {
                let ppr = child(n, "pPr");
                let id = ppr
                    .and_then(|p| child(p, "pStyle"))
                    .and_then(|p| attr(p, "val"))
                    .or_else(|| {
                        self.styles
                            .iter()
                            .filter(|(_, s)| s.default_paragraph)
                            .map(|(id, _)| id.as_str())
                            .min()
                    });
                let chain = self.chain(id)?;
                if chain.iter().any(|id| !self.styles.contains_key(id)) {
                    self.warn("docx_missing_style", "Referenced paragraph style is missing; inherited formatting or numbering may be incomplete.");
                }
                let mut format = RunFormat::default();
                for id in chain.iter().rev() {
                    if let Some(style) = self.styles.get(id) {
                        format.apply(style.run, true);
                    }
                }
                let mut attachments = Vec::new();
                let inlines = self.inline(n, depth + 1, format, &mut attachments)?;
                let (mut num_id, mut num_level, mut has_num) = num_props(ppr);
                for id in &chain {
                    if let Some(style) = self.styles.get(id) {
                        num_id = num_id.or(style.num_id);
                        num_level = num_level.or(style.num_level);
                        has_num |= style.has_num;
                    }
                }
                let marker = if let Some(id) = num_id {
                    match self.numbering.marker(id, num_level, &chain) {
                        Ok(m) => m,
                        Err(message) => {
                            self.warn("docx_numbering", message);
                            None
                        }
                    }
                } else {
                    if has_num {
                        self.warn(
                            "docx_numbering",
                            "List properties do not resolve to a numbering instance.",
                        );
                    }
                    None
                };
                let kind = chain.iter().find_map(|id| {
                    self.styles.get(id).and_then(|style| {
                        match style.name.to_ascii_lowercase().replace(' ', "").as_str() {
                            "sourcecode" | "code" | "htmlpreformatted" => Some("code"),
                            "quote" | "quotation" | "intensequote" => Some("quote"),
                            _ => None,
                        }
                    })
                });
                let content = if let Some(level) = self.heading(n) {
                    if marker.is_some() {
                        self.warn(
                            "docx_heading_numbering",
                            "Heading text is retained but its numbering is not rendered.",
                        );
                    }
                    BlockContent::Heading { level, inlines }
                } else if kind == Some("code") {
                    BlockContent::Code {
                        language: String::new(),
                        text: plain(&inlines),
                    }
                } else if kind == Some("quote") {
                    BlockContent::Quote {
                        blocks: vec![self
                            .ctx
                            .block(BlockContent::Paragraph { inlines }, self.source(n))?],
                    }
                } else {
                    BlockContent::Paragraph { inlines }
                };
                let is_heading = matches!(content, BlockContent::Heading { .. });
                let block = self.ctx.block(content, self.source(n))?;
                if let Some(marker) = marker.filter(|_| !is_heading) {
                    pending.push(Item { block, marker });
                } else {
                    out.extend(numbering::lists(std::mem::take(&mut pending), self.ctx)?);
                    out.push(block);
                }
                if !attachments.is_empty() {
                    out.extend(numbering::lists(std::mem::take(&mut pending), self.ctx)?);
                    out.extend(attachments);
                }
            } else if w(n, "tbl") {
                out.extend(numbering::lists(std::mem::take(&mut pending), self.ctx)?);
                out.push(self.table(n, depth)?);
            } else if metadata(n)
                || w(n, "sectPr")
                || w(n, "tcPr")
                || w(n, "sdtPr")
                || w(n, "sdtEndPr")
                || w(n, "bookmarkStart")
                || w(n, "bookmarkEnd")
            {
            } else {
                out.extend(numbering::lists(std::mem::take(&mut pending), self.ctx)?);
                self.warn(
                    "docx_unknown_block",
                    "An unsupported body element was omitted; inspect the source document.",
                );
            }
        }
        out.extend(numbering::lists(pending, self.ctx)?);
        Ok(out)
    }
    fn inline(
        &mut self,
        node: Node<'_, '_>,
        depth: usize,
        base: RunFormat,
        attachments: &mut Vec<Block>,
    ) -> Result<Vec<Inline>, Error> {
        self.ctx.depth(depth)?;
        let mut out = Vec::new();
        for n in node.children().filter(Node::is_element) {
            self.ctx.node()?;
            if w(n, "t") {
                let text = n.text().unwrap_or("");
                self.ctx.text(text)?;
                out.push(Inline::text(text));
            } else if w(n, "tab") {
                self.ctx.text("\t")?;
                out.push(Inline::text("\t"));
            } else if w(n, "br") || w(n, "cr") {
                out.push(Inline::Break);
            } else if w(n, "r") {
                let mut children = self.inline(n, depth + 1, base, attachments)?;
                let props = child(n, "rPr");
                let mut format = base;
                let id = props
                    .and_then(|p| child(p, "rStyle"))
                    .and_then(|n| attr(n, "val"));
                let chain = self.chain(id)?;
                if chain.iter().any(|id| !self.styles.contains_key(id)) {
                    self.warn("docx_missing_style", "Referenced character style is missing; inherited formatting may be incomplete.");
                }
                for id in chain.iter().rev() {
                    if let Some(style) = self.styles.get(id) {
                        format.apply(style.run, true);
                    }
                }
                format.apply(run_style(props), false);
                if format.italic {
                    children = vec![Inline::Emphasis { children }];
                }
                if format.bold {
                    children = vec![Inline::Strong { children }];
                }
                if format.strike {
                    children = vec![Inline::Strike { children }];
                }
                out.extend(children);
            } else if w(n, "ptab") {
                self.warn(
                    "docx_positioned_tab_layout",
                    "Positioned tab alignment is not reconstructed; surrounding text is retained.",
                );
            } else if w(n, "hyperlink") {
                let children = self.inline(n, depth + 1, base, attachments)?;
                let id = n
                    .attribute((R, "id"))
                    .or_else(|| n.attribute((R_STRICT, "id")));
                let url = id
                    .and_then(|id| self.relationships.get(id))
                    .filter(|r| r.external)
                    .map(|r| r.target.clone())
                    .or_else(|| attr(n, "anchor").map(|a| format!("#{a}")));
                if let Some(url) = url {
                    out.push(Inline::Link { url, children });
                } else {
                    out.extend(children);
                }
            } else if metadata(n)
                || w(n, "del")
                || w(n, "moveFrom")
                || w(n, "pPr")
                || w(n, "rPr")
                || w(n, "sdtPr")
                || w(n, "sdtEndPr")
                || w(n, "bookmarkStart")
                || w(n, "bookmarkEnd")
                || w(n, "proofErr")
                || w(n, "lastRenderedPageBreak")
            {
            } else if w(n, "ins")
                || w(n, "sdt")
                || w(n, "sdtContent")
                || w(n, "smartTag")
                || w(n, "moveTo")
            {
                out.extend(self.inline(n, depth + 1, base, attachments)?);
            } else if n.tag_name().namespace() == Some(MC)
                && n.tag_name().name() == "AlternateContent"
            {
                if let Some(c) = alternate(n) {
                    out.extend(self.inline(c, depth + 1, base, attachments)?);
                }
            } else if w(n, "drawing") || w(n, "pict") {
                let mut boxes = Vec::new();
                text_boxes(n, &mut boxes);
                for b in &boxes {
                    attachments.extend(self.blocks(*b, depth + 1)?);
                }
                // Preserve distinct descriptions and titles even when a drawing
                // also contains text boxes. Do not repeat their visible text.
                let visible = if boxes.is_empty() {
                    String::new()
                } else {
                    boxes
                        .iter()
                        .flat_map(|b| b.descendants())
                        .filter(|d| w(*d, "t"))
                        .filter_map(|d| d.text())
                        .flat_map(str::chars)
                        .filter(|c| c.is_alphanumeric())
                        .flat_map(char::to_lowercase)
                        .collect::<String>()
                };
                let mut descriptions = Vec::new();
                for d in n.descendants().filter(Node::is_element) {
                    if matches!(d.tag_name().name(), "docPr" | "imagedata") {
                        for t in [d.attribute("descr"), d.attribute("title")]
                            .into_iter()
                            .flatten()
                        {
                            if !t.trim().is_empty()
                                && !descriptions.contains(&t)
                                && (visible.is_empty() || !visible.contains(&drawing_text_key(t)))
                            {
                                descriptions.push(t);
                            }
                        }
                    }
                }
                for image in n
                    .descendants()
                    .filter(Node::is_element)
                    .filter(|d| matches!(d.tag_name().name(), "blip" | "imagedata"))
                {
                    let id = image
                        .attributes()
                        .find(|a| matches!(a.name(), "embed" | "id" | "link"))
                        .map(|a| a.value());
                    if let Some(rel) = id.and_then(|id| self.relationships.get(id)) {
                        let inline = if rel.external {
                            Some(Inline::Image {
                                url: rel.target.clone(),
                                alt: String::new(),
                            })
                        } else {
                            resolve(&self.part, &rel.target)
                                .ok()
                                .and_then(|part| self.ctx.image_inline(&part, ""))
                        };
                        if let Some(inline) = inline {
                            self.ctx.node()?;
                            out.push(inline);
                        }
                    }
                }
                for (index, t) in descriptions.into_iter().enumerate() {
                    if index > 0 {
                        out.push(Inline::Break);
                    }
                    self.ctx.text(t)?;
                    out.push(Inline::text(t));
                }
                self.warn("docx_drawing_layout", "Text boxes and available descriptions retained; image pixels and exact floating layout are not reconstructed.");
            } else if ["footnoteReference", "endnoteReference", "commentReference"]
                .iter()
                .any(|name| w(n, name))
            {
                let kind = n
                    .tag_name()
                    .name()
                    .trim_end_matches("Reference")
                    .to_string();
                if let Some(id) = attr(n, "id") {
                    let key = (kind.clone(), id.to_string());
                    if !self.note_refs.contains(&key) {
                        self.note_refs.push(key);
                    }
                    out.push(Inline::Link {
                        url: format!("#docx-{kind}-{id}"),
                        children: vec![Inline::text(format!("[{kind} {id}]"))],
                    });
                }
            } else if w(n, "footnoteRef") || w(n, "endnoteRef") || w(n, "annotationRef") {
                // The generated note heading carries the identifier.
            } else if w(n, "fldChar") || w(n, "instrText") {
                self.warn(
                    "docx_fields",
                    "Cached field results retained; field instructions are not evaluated.",
                );
            } else if w(n, "noBreakHyphen") {
                self.ctx.text("‑")?;
                out.push(Inline::text("‑"));
            } else if w(n, "softHyphen") {
                self.ctx.text("\u{ad}")?;
                out.push(Inline::text("\u{ad}"));
            } else if w(n, "fldSimple") {
                self.warn(
                    "docx_fields",
                    "Field results are retained but field instructions are not evaluated.",
                );
                out.extend(self.inline(n, depth + 1, base, attachments)?);
            } else {
                self.warn("docx_unsupported_inline", "Unsupported inline content (such as images, equations, notes or revisions) was omitted.");
            }
        }
        Ok(out)
    }
}

pub fn parse(bytes: &[u8], ctx: &mut Context<'_>) -> Result<Vec<Block>, Error> {
    let mut pkg = Package::new(bytes, ctx)?;
    let root_rels_text = pkg.read("_rels/.rels")?;
    let root_rels = relationships(root_rels_text.as_deref(), ctx)?;
    let main = root_rels
        .values()
        .find(|r| r.kind.ends_with("/officeDocument"));
    let part = if let Some(rel) = main {
        if rel.external {
            return Err(malformed("external document part"));
        }
        resolve("", &rel.target)?
    } else {
        "word/document.xml".into()
    };
    let rels_text = pkg.read(&rels_name(&part))?;
    let rels = relationships(rels_text.as_deref(), ctx)?;
    let styles_part = rels
        .values()
        .find(|r| r.kind.ends_with("/styles") && !r.external)
        .map(|r| resolve(&part, &r.target))
        .transpose()?
        .unwrap_or(resolve(&part, "styles.xml")?);
    let styles_text = pkg.read(&styles_part)?;
    let styles = match styles(styles_text.as_deref(), ctx) {
        Ok(styles) => styles,
        Err(Error::Malformed(_)) => {
            ctx.diagnostic(
                "docx_styles_unreadable",
                "Optional styles could not be parsed; body text is preserved without those styles.",
                Source {
                    part: Some(styles_part.clone().into()),
                    ..Default::default()
                },
                true,
            );
            HashMap::new()
        }
        Err(error) => return Err(error),
    };
    let numbering_part = rels
        .values()
        .find(|r| r.kind.ends_with("/numbering") && !r.external)
        .map(|r| resolve(&part, &r.target))
        .transpose()?
        .unwrap_or(resolve(&part, "numbering.xml")?);
    let numbering_text = pkg.read(&numbering_part)?;
    let numbering = match Numbering::parse(numbering_text.as_deref(), ctx) {
        Ok(numbering) => numbering,
        Err(Error::Malformed(_)) => {
            ctx.diagnostic(
                "docx_numbering_unreadable",
                "Optional numbering definitions could not be parsed.",
                Source {
                    part: Some(numbering_part.into()),
                    ..Default::default()
                },
                true,
            );
            Numbering::default()
        }
        Err(error) => return Err(error),
    };
    let text = pkg
        .read(&part)?
        .ok_or_else(|| malformed("missing main document part"))?;
    let doc = xml(&text, ctx)?;
    if !w(doc.root_element(), "document") {
        return Err(malformed("main part is not a Word document"));
    }
    let body =
        child(doc.root_element(), "body").ok_or_else(|| malformed("missing Word document body"))?;
    let mut walker = Walker {
        ctx,
        part,
        relationships: rels,
        styles,
        numbering,
        note_refs: Vec::new(),
    };
    let mut blocks = walker.blocks(body, 0)?;
    walker.auxiliary(&mut pkg, body, &mut blocks)?;
    Ok(blocks)
}

fn plain(items: &[Inline]) -> String {
    let mut out = String::new();
    for item in items {
        match item {
            Inline::Text { text } | Inline::Code { text } => out.push_str(text),
            Inline::Break => out.push('\n'),
            Inline::Math { tex, .. } => out.push_str(tex),
            Inline::Anchor { .. } => {}
            Inline::Image { alt, .. } => out.push_str(alt),
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Strike { children }
            | Inline::Link { children, .. } => out.push_str(&plain(children)),
        }
    }
    out
}
