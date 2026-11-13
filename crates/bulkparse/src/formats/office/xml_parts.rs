//! Partition large XML parts into validated row/string fragments. A small
//! projection preserves the original ancestor tree and metadata. Comment
//! placeholders prevent adjacent text nodes from coalescing when a fragment is
//! removed, allowing exact accounting of the original DOM node budget.
use super::*;
use quick_xml::{events::Event, Reader, Writer};
use std::borrow::Cow;
use std::io::{BufReader, BufWriter, Seek, SeekFrom, Write};
use std::sync::Arc;

type FragmentCapture = (Writer<Vec<u8>>, usize, Vec<Vec<u8>>);
type WrapperCache = (Option<Arc<Frame>>, Vec<Vec<u8>>, Arc<Wrapper>);

#[derive(Clone)]
struct Frame {
    attributes: Vec<(Vec<u8>, Vec<u8>)>,
    name: Vec<u8>,
    local: String,
    parent: Option<Arc<Frame>>,
    projection_start: usize,
    depth: usize,
}
impl Frame {
    fn wrapper(parent: &Option<Arc<Self>>, prefixes: &[Vec<u8>]) -> Wrapper {
        let mut chain = vec![];
        let mut next = parent.as_ref();
        while let Some(frame) = next {
            chain.push(frame);
            next = frame.parent.as_ref();
        }
        let mut out = Vec::new();
        // Ancestors are validated in the projection. Keep all their ordinary
        // attributes, but only namespace declarations used by this fragment or
        // its ancestors. Large ODF namespace lists otherwise get parsed for
        // every row, several times. Default namespace declarations always stay.
        let mut needed: Vec<&[u8]> = prefixes.iter().map(Vec::as_slice).collect();
        for frame in &chain {
            if let Some(p) = prefix(&frame.name) {
                needed.push(p);
            }
            for (key, _) in &frame.attributes {
                if !key.starts_with(b"xmlns:") {
                    if let Some(p) = prefix(key) {
                        needed.push(p);
                    }
                }
            }
        }
        for frame in chain.iter().rev() {
            out.push(b'<');
            out.extend_from_slice(&frame.name);
            for (key, value) in &frame.attributes {
                if key
                    .strip_prefix(b"xmlns:")
                    .is_some_and(|p| !needed.contains(&p))
                {
                    continue;
                }
                // Preserve raw entity references and whitespace normalization.
                // A valid quoted XML value cannot contain both quote characters.
                let quote = if value.contains(&b'"') { b'\'' } else { b'"' };
                out.push(b' ');
                out.extend_from_slice(key);
                out.push(b'=');
                out.push(quote);
                out.extend_from_slice(value);
                out.push(quote);
            }
            out.push(b'>');
        }
        let start = out;
        let mut out = Vec::new();
        for frame in chain {
            out.extend_from_slice(b"</");
            out.extend_from_slice(&frame.name);
            out.push(b'>');
        }
        Wrapper {
            start,
            end: out,
            depth: parent.as_ref().map_or(0, |p| p.depth),
        }
    }
}
struct Wrapper {
    start: Vec<u8>,
    end: Vec<u8>,
    depth: usize,
}
impl Wrapper {
    fn wrap(&self, bytes: &[u8]) -> Result<String, Error> {
        let mut out = Vec::with_capacity(self.start.len() + bytes.len() + self.end.len());
        out.extend_from_slice(&self.start);
        out.extend_from_slice(bytes);
        out.extend_from_slice(&self.end);
        super::xml_stream::owned_utf8(out)
    }
}
fn prefix(name: &[u8]) -> Option<&[u8]> {
    name.iter().position(|c| *c == b':').map(|i| &name[..i])
}

pub(super) struct Store {
    file: BufWriter<tempfile::SpooledTempFile>,
    positions: Vec<(u64, usize)>,
    end: u64,
    writing: bool,
    cache: Vec<u8>,
    cache_start: u64,
}
impl Store {
    pub fn new() -> Self {
        Self {
            file: BufWriter::with_capacity(64 * 1024, tempfile::spooled_tempfile(64 * 1024)),
            positions: vec![],
            end: 0,
            writing: true,
            cache: vec![],
            cache_start: 0,
        }
    }
    pub fn push(&mut self, bytes: &[u8]) -> Result<(), Error> {
        // Appends are sequential: seeking for every record flushes the write
        // buffer and turns a worksheet into thousands of tiny system calls.
        if !self.writing {
            self.file.seek(SeekFrom::Start(self.end))?;
            self.cache.clear();
            self.writing = true;
        }
        self.file.write_all(bytes)?;
        self.positions.push((self.end, bytes.len()));
        self.end += bytes.len() as u64;
        Ok(())
    }
    pub fn get(&mut self, index: usize) -> Result<Cow<'_, [u8]>, Error> {
        let &(offset, len) = self
            .positions
            .get(index)
            .ok_or_else(|| bad("shared string outside table"))?;
        if self.writing {
            self.file.flush()?;
            self.writing = false;
        }
        // Shared strings need random access; rows are normally sequential.
        // A fixed-size read window serves both without keeping a whole part.
        if len <= 64 * 1024 {
            let cache_end = self.cache_start + self.cache.len() as u64;
            if offset < self.cache_start || offset + len as u64 > cache_end {
                self.cache_start = offset;
                self.cache
                    .resize((self.end - offset).min(64 * 1024) as usize, 0);
                self.file.get_mut().seek(SeekFrom::Start(offset))?;
                self.file.get_mut().read_exact(&mut self.cache)?;
            }
            let start = (offset - self.cache_start) as usize;
            // The caller consumes the record before the next read. Borrow the
            // existing window instead of allocating and copying every row or
            // shared string. Oversized records remain owned and uncached.
            return Ok(Cow::Borrowed(&self.cache[start..start + len]));
        }
        self.file.get_mut().seek(SeekFrom::Start(offset))?;
        let mut bytes = vec![0; len];
        self.file.get_mut().read_exact(&mut bytes)?;
        Ok(Cow::Owned(bytes))
    }
}

pub(super) struct Fragment {
    wrapper: Arc<Wrapper>,
    pub table_start: Option<usize>,
}
pub(super) struct Parts {
    pub projection: String,
    pub fragments: Vec<Fragment>,
    store: Store,
    plans: Option<Store>,
    pub formatting: bool,
    pub visual: bool,
    pub styled_odf: bool,
    extra_nodes: usize,
}
impl Parts {
    pub fn record_count(&self) -> usize {
        self.store.positions.len()
    }
    pub fn record(&mut self, index: usize) -> Result<Cow<'_, [u8]>, Error> {
        self.plans.as_mut().unwrap_or(&mut self.store).get(index)
    }

    pub fn into_store(self) -> Store {
        self.store
    }
    pub fn account(&self, ctx: &mut Context<'_>) -> Result<(), Error> {
        ctx.account_nodes(self.extra_nodes)
    }
    pub fn get(&mut self, index: usize) -> Result<String, Error> {
        self.fragments[index].wrapper.wrap(&self.store.get(index)?)
    }
    #[cfg(test)]
    pub fn target<'a, 'b>(&self, index: usize, doc: &'a Document<'b>) -> Node<'a, 'b> {
        let ancestors = self.fragments[index].wrapper.depth;
        doc.descendants()
            .filter(Node::is_element)
            .nth(ancestors)
            .expect("validated fragment root")
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Kind {
    Rows,
    Strings,
    OdfRows,
}

/// Small parts retain the direct DOM path. Unusual metadata inside a row falls
/// back to it too, rather than changing document-wide formatting semantics.
#[cfg(test)]
pub(super) fn partition(
    pkg: &mut Package<'_>,
    name: &str,
    kind: Kind,
    ctx: &Context<'_>,
) -> Result<Option<Parts>, Error> {
    partition_mapped(pkg, name, kind, ctx, None)
}

type Mapper<'a> = dyn FnMut(Node<'_, '_>) -> Result<Vec<u8>, Error> + 'a;
pub(super) fn partition_mapped(
    pkg: &mut Package<'_>,
    name: &str,
    kind: Kind,
    ctx: &Context<'_>,
    mut map: Option<&mut Mapper<'_>>,
) -> Result<Option<Parts>, Error> {
    let cap = pkg.remaining;
    let f = pkg.zip.by_name(name).map_err(bad)?;
    if f.size() > cap as u64 {
        return Err(Error::Limit("max_expanded_bytes"));
    }
    if f.size() <= 256 * 1024 {
        return Ok(None);
    }
    // Validate and charge while partitioning, in one decompression pass. The
    // loop bounds source nodes/bytes/depth before retaining the next fragment;
    // each fragment and the projection still undergo authoritative DOM checks.
    // A separate whole-part preflight doubled scanning work for accepted input.
    let mut reader = Reader::from_reader(BufReader::new(f.take(cap as u64 + 1)));
    reader.config_mut().check_comments = true;
    let mut buf = Vec::new();
    let mut projection = Writer::new(Vec::new());
    let mut parent: Option<Arc<Frame>> = None;
    let mut capture: Option<FragmentCapture> = None;
    let mut result = Parts {
        projection: String::new(),
        fragments: vec![],
        store: Store::new(),
        plans: (kind == Kind::OdfRows && map.is_some()).then(Store::new),
        formatting: false,
        visual: false,
        styled_odf: false,
        extra_nodes: 0,
    };
    let mut last_wrapper: Option<WrapperCache> = None;
    let mut extra = 0usize;
    let mut lower_nodes = 1usize;
    loop {
        ctx.check()?;
        let event = reader.read_event_into(&mut buf).map_err(bad)?;
        if reader.buffer_position() > cap as u64 {
            return Err(Error::Limit("max_expanded_bytes"));
        }
        if matches!(
            event,
            Event::Start(_) | Event::Empty(_) | Event::Comment(_) | Event::PI(_)
        ) {
            lower_nodes = lower_nodes.saturating_add(1);
            if !ctx.fits(lower_nodes, 0, 0) {
                return Err(Error::Limit("max_nodes"));
            }
        }
        if matches!(event, Event::DocType(_)) {
            return Err(bad("XML DTD is not allowed"));
        }
        if let Event::Start(e) | Event::Empty(e) = &event {
            let local_name = e.local_name();
            let local = local_name.as_ref();
            if kind == Kind::Rows {
                result.formatting |=
                    matches!(local, b"r" | b"hyperlinks" | b"conditionalFormatting");
                result.visual |= matches!(
                    local,
                    b"drawing" | b"legacyDrawing" | b"oleObjects" | b"extLst"
                );
            }
            if kind == Kind::OdfRows && !result.styled_odf {
                result.styled_odf = local == b"span"
                    || e.attributes()
                        .with_checks(false)
                        .any(|a| a.is_ok_and(|a| a.key.local_name().as_ref() == b"style-name"));
            }
            if capture.is_some()
                && ((kind == Kind::Rows && matches!(local, b"col" | b"mergeCell" | b"hyperlink"))
                    || (kind == Kind::OdfRows
                        && matches!(
                            local,
                            b"style" | b"default-style" | b"list-style" | b"object"
                        )))
            {
                return Ok(None);
            }
            let target = match kind {
                Kind::Rows => local == b"row" && parent.is_some(),
                Kind::Strings => local == b"si" && parent.as_ref().is_some_and(|p| p.depth == 1),
                Kind::OdfRows => local == b"table-row" && parent.is_some(),
            };
            if target && capture.is_none() {
                capture = Some((Writer::new(Vec::new()), 0, vec![]));
            }
        }
        if let Some((writer, depth, prefixes)) = capture.as_mut() {
            if let Event::Start(e) | Event::Empty(e) = &event {
                let mut remember = |key: &[u8]| {
                    if let Some(p) = prefix(key) {
                        if p != b"xmlns" && !prefixes.iter().any(|v| v == p) {
                            prefixes.push(p.to_vec());
                        }
                    }
                };
                remember(e.name().as_ref());
                for a in e.attributes().with_checks(false) {
                    remember(a.map_err(bad)?.key.as_ref());
                }
            }
            match event {
                Event::Start(_) => *depth += 1,
                Event::End(_) => *depth -= 1,
                Event::Eof => return Err(bad("unclosed XML fragment")),
                _ => {}
            }
            ctx.depth(parent.as_ref().map_or(0, |p| p.depth) + *depth + 1)?;
            writer.write_event(event.borrow()).map_err(bad)?;
            if *depth == 0 {
                let (writer, _, prefixes) = capture.take().unwrap();
                let raw = writer.into_inner();
                // Consecutive rows usually have the same ancestor/namespace scope.
                // Share the immutable wrapper instead of allocating prefix and
                // ancestor strings for every row and rebuilding them on each read.
                let same_scope = last_wrapper.as_ref().is_some_and(|(p, used, _)| {
                    used == &prefixes
                        && match (p, &parent) {
                            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                            (None, None) => true,
                            _ => false,
                        }
                });
                if !same_scope {
                    let wrapper = Arc::new(Frame::wrapper(&parent, &prefixes));
                    last_wrapper = Some((parent.clone(), prefixes, wrapper));
                }
                let wrapper = last_wrapper.as_ref().unwrap().2.clone();
                let wrapped = wrapper.wrap(&raw)?;
                let ancestors = wrapper.depth;
                let doc = parse_fragment(&wrapped, ctx, ancestors)?;
                let count = doc
                    .descendants()
                    .count()
                    .checked_sub(ancestors + 2)
                    .ok_or_else(|| bad("invalid XML fragment"))?;
                extra = extra.saturating_add(count);
                if !ctx.fits(extra, 0, 0) {
                    return Err(Error::Limit("max_nodes"));
                }
                let mut table = parent.as_ref();
                while table.is_some_and(|p| p.local != "table") {
                    table = table.unwrap().parent.as_ref();
                }
                if let Some(map) = map.as_deref_mut() {
                    let node = doc
                        .descendants()
                        .filter(Node::is_element)
                        .nth(ancestors)
                        .ok_or_else(|| bad("missing XML fragment root"))?;
                    let record = map(node)?;
                    if let Some(plans) = &mut result.plans {
                        plans.push(&record)?;
                    } else {
                        result.store.push(&record)?;
                    }
                }
                if map.is_none() || result.plans.is_some() {
                    result.fragments.push(Fragment {
                        wrapper,
                        table_start: table.map(|p| p.projection_start),
                    });
                    result.store.push(&raw)?;
                }
                projection
                    .write_event(Event::Comment(quick_xml::events::BytesText::new(
                        "fragment",
                    )))
                    .map_err(bad)?;
            }
        } else {
            match &event {
                Event::Start(e) => {
                    let attributes = e
                        .attributes()
                        .map(|a| {
                            let a = a.map_err(bad)?;
                            Ok((a.key.as_ref().to_vec(), a.value.into_owned()))
                        })
                        .collect::<Result<Vec<_>, Error>>()?;
                    parent = Some(Arc::new(Frame {
                        attributes,
                        name: e.name().as_ref().to_vec(),
                        local: std::str::from_utf8(e.local_name().as_ref())
                            .map_err(bad)?
                            .into(),
                        projection_start: projection.get_ref().len(),
                        depth: parent.as_ref().map_or(1, |p| p.depth + 1),
                        parent: parent.clone(),
                    }));
                    ctx.depth(parent.as_ref().unwrap().depth + 1)?;
                }
                Event::End(_) => {
                    parent = parent.as_ref().and_then(|p| p.parent.clone());
                }
                Event::Eof => break,
                _ => {}
            }
            projection.write_event(event.borrow()).map_err(bad)?;
        }
        buf.clear();
    }
    if parent.is_some() {
        return Err(bad("unclosed XML part"));
    }
    let remaining = reader.into_inner().into_inner().limit() as usize;
    let consumed = cap + 1 - remaining;
    if consumed > cap {
        return Err(Error::Limit("max_expanded_bytes"));
    }
    result.projection = super::xml_stream::owned_utf8(projection.into_inner())?;
    let doc = parse_fragment(&result.projection, ctx, 0)?;
    if !ctx.fits(extra.saturating_add(doc.descendants().count()), 0, 0) {
        return Err(Error::Limit("max_nodes"));
    }
    // Charge exactly once. The caller's normal xml() call accounts for the
    // projection (including one placeholder per original fragment).
    result.extra_nodes = extra;
    pkg.remaining -= consumed;
    Ok(Some(result))
}

pub(super) fn parse_fragment<'a>(
    source: &'a str,
    ctx: &Context<'_>,
    ancestors: usize,
) -> Result<Document<'a>, Error> {
    let doc = Document::parse_with_options(
        source,
        ParsingOptions {
            allow_dtd: false,
            nodes_limit: ctx
                .options
                .limits
                .max_nodes
                .saturating_add(ancestors + 1)
                .min(u32::MAX as usize) as u32,
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
    // Walk each edge once instead of recounting every node's ancestor chain.
    let mut node = doc.root();
    let mut depth = 1;
    'walk: loop {
        ctx.depth(depth)?;
        if let Some(child) = node.first_child() {
            node = child;
            depth += 1;
            continue;
        }
        loop {
            if let Some(sibling) = node.next_sibling() {
                node = sibling;
                break;
            }
            match node.parent() {
                Some(parent) => {
                    node = parent;
                    depth -= 1;
                }
                None => break 'walk,
            }
        }
    }
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CancellationToken, Options};
    #[test]
    fn buffered_store_handles_spill_random_access_and_appends_after_reads() {
        let mut store = Store::new();
        let records: Vec<_> = [0, 7, 65_530, 17, 130_000, 33, 9]
            .into_iter()
            .enumerate()
            .map(|(i, len)| vec![i as u8; len])
            .collect();
        for bytes in &records {
            store.push(bytes).unwrap();
        }
        for i in [4, 0, 6, 2, 3, 5, 1, 2] {
            assert_eq!(store.get(i).unwrap(), records[i]);
        }
        store.push(b"after reading").unwrap();
        assert_eq!(store.get(records.len()).unwrap().as_ref(), b"after reading");
        assert_eq!(store.get(2).unwrap(), records[2]);
    }
    fn package(source: &str) -> Vec<u8> {
        let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
        z.start_file("sheet.xml", zip::write::SimpleFileOptions::default())
            .unwrap();
        z.write_all(source.as_bytes()).unwrap();
        z.finish().unwrap().into_inner()
    }
    fn source(last: &str) -> String {
        format!("<?xml version=\"1.0\"?><x:worksheet xmlns:x=\"urn:x\"><x:sheetData>\n{}{last}</x:sheetData></x:worksheet>", "<x:row><x:c><x:t>a&amp;b<![CDATA[c]]>&#32;d</x:t></x:c></x:row>\n".repeat(5000))
    }
    #[test]
    fn fragments_preserve_namespace_scope_and_exact_dom_node_budget() {
        let source = source("");
        let count = Document::parse(&source).unwrap().descendants().count();
        let mut options = Options::default();
        options.limits.max_nodes = count + 7;
        let cancel = CancellationToken::default();
        let mut ctx = Context::new(&options, &cancel);
        for _ in 0..7 {
            ctx.node().unwrap();
        }
        let bytes = package(&source);
        let mut pkg = Package::new(&bytes, &ctx).unwrap();
        let mut parts = partition(&mut pkg, "sheet.xml", Kind::Rows, &ctx)
            .unwrap()
            .unwrap();
        assert_eq!(parts.fragments.len(), 5000);
        parts.account(&mut ctx).unwrap();
        xml(&parts.projection, &mut ctx).unwrap();
        assert!(ctx.fits(0, 0, 0));
        assert!(!ctx.fits(1, 0, 0));
        for i in [0, 4999] {
            let text = parts.get(i).unwrap();
            let doc = Document::parse(&text).unwrap();
            let row = parts.target(i, &doc);
            assert_eq!(row.tag_name().namespace(), Some("urn:x"));
            assert_eq!(content(row), "a&bc d");
        }
        assert_eq!(
            pkg.remaining,
            options.limits.max_expanded_bytes - source.len()
        );
    }
    #[test]
    fn late_invalid_xml_is_rejected_and_unusual_row_metadata_falls_back() {
        let options = Options::default();
        let cancel = CancellationToken::default();
        let ctx = Context::new(&options, &cancel);
        for tail in [
            "<x:row><x:c>&unknown;</x:c></x:row>",
            "<x:row><q:cell/></x:row>",
            "<x:row a='1' a='2'/>",
        ] {
            let bytes = package(&source(tail));
            let mut pkg = Package::new(&bytes, &ctx).unwrap();
            assert!(matches!(
                partition(&mut pkg, "sheet.xml", Kind::Rows, &ctx),
                Err(Error::Malformed(_))
            ));
        }
        let text = source("<x:row><mergeCell ref='A1:B1'/></x:row>");
        let bytes = package(&text);
        let mut pkg = Package::new(&bytes, &ctx).unwrap();
        assert!(partition(&mut pkg, "sheet.xml", Kind::Rows, &ctx)
            .unwrap()
            .is_none());
        assert_eq!(pkg.remaining, options.limits.max_expanded_bytes);
        assert_eq!(pkg.required_xml("sheet.xml", &ctx).unwrap(), text);
    }
}
