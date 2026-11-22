use crate::{Block, BlockContent, Diagnostic, Document, Inline};
use serde::{Deserialize, Serialize};
use std::io::{self, Write};

#[derive(Debug, Serialize, Deserialize)]
pub struct MarkdownOutput {
    pub text: String,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn render(document: &Document) -> MarkdownOutput {
    let mut bytes = Vec::new();
    let mut writer = Writer::new(&mut bytes);
    writer
        .blocks(&document.blocks)
        .expect("Vec writes cannot fail");
    let diagnostics = writer.diagnostics;
    let text = String::from_utf8(bytes).expect("renderer writes UTF-8");
    MarkdownOutput { text, diagnostics }
}

/// Incremental top-level Markdown rendering. Tables allocate at most one row's
/// rendered text, rather than a second complete table-sized string.
pub(crate) struct Writer<'a> {
    output: &'a mut dyn Write,
    pub diagnostics: Vec<Diagnostic>,
    emitted: bool,
}
impl<'a> Writer<'a> {
    pub fn new(output: &'a mut dyn Write) -> Self {
        Self {
            output,
            diagnostics: vec![],
            emitted: false,
        }
    }
    fn begin(&mut self) -> io::Result<()> {
        if self.emitted {
            self.output.write_all(b"\n")?;
        }
        self.emitted = true;
        Ok(())
    }
    pub fn blocks(&mut self, items: &[Block]) -> io::Result<()> {
        for block in items {
            self.block(block)?;
        }
        Ok(())
    }
    pub fn table_diagnostics(&mut self, source: &crate::Source, spans: bool, header_rows: usize) {
        table_diagnostics(&mut self.diagnostics, source, spans, header_rows);
    }
    pub fn table_row(
        &mut self,
        cells: &[crate::Cell],
        index: usize,
        table: &mut TableState,
    ) -> io::Result<()> {
        if table.width == 0 {
            return Ok(());
        }
        if index == 0 {
            self.begin()?;
            if table.header_rows == 0 {
                self.row(&[], table.width)?;
                self.delimiter(table.width)?;
            }
        }
        if !table.spans && !table.positioned {
            self.row(cells, table.width)?;
        } else {
            let width = table.width;
            let positioned = table.positioned;
            let mut positions = table.positions(cells, index).peekable();
            self.output.write_all(b"| ")?;
            for column in 0..width {
                if column > 0 {
                    self.output.write_all(b" | ")?;
                }
                if positions.peek().is_some_and(|(c, _)| *c == column) {
                    let (_, cell) = positions.next().unwrap();
                    self.cell(cell, positioned)?;
                }
            }
            self.output.write_all(b" |\n")?;
        }
        if index == 0 && table.header_rows > 0 {
            self.delimiter(table.width)?;
        }
        Ok(())
    }
    pub fn block(&mut self, block: &Block) -> io::Result<()> {
        if let BlockContent::Table { rows, header_rows } = &block.content {
            let mut table = TableState::for_rows(rows, *header_rows, &block.source);
            self.table_diagnostics(&block.source, table.spans, *header_rows);
            for (i, row) in rows.iter().enumerate() {
                self.table_row(row, i, &mut table)?;
            }
        } else {
            let text = blocks(std::slice::from_ref(block), &mut self.diagnostics);
            if !text.is_empty() {
                self.begin()?;
                self.output.write_all(text.as_bytes())?;
            }
        }
        Ok(())
    }
    /// Rows must have their final width and order. CSV can send and release each
    /// record immediately; workbook readers can release completed worksheets.
    pub fn record(
        &mut self,
        cells: &[crate::Cell],
        index: usize,
        header: bool,
        width: usize,
    ) -> io::Result<()> {
        if index == 0 {
            self.begin()?;
            if header {
                self.row(cells, width)?;
            } else {
                self.row(&[], width)?;
            }
            self.delimiter(width)?;
            if header {
                return Ok(());
            }
        }
        self.row(cells, width)
    }
    fn delimiter(&mut self, width: usize) -> io::Result<()> {
        self.output.write_all(b"| ")?;
        for i in 0..width {
            if i > 0 {
                self.output.write_all(b" | ")?;
            }
            self.output.write_all(b"---")?;
        }
        self.output.write_all(b" |\n")
    }
    fn row(&mut self, cells: &[crate::Cell], width: usize) -> io::Result<()> {
        self.output.write_all(b"| ")?;
        for i in 0..width {
            if i > 0 {
                self.output.write_all(b" | ")?;
            }
            if let Some(cell) = cells.get(i) {
                self.cell(cell, false)?;
            }
        }
        self.output.write_all(b" |\n")
    }
    fn cell(&mut self, cell: &crate::Cell, positioned: bool) -> io::Result<()> {
        if let [Block {
            content: BlockContent::Paragraph { inlines },
            ..
        }] = cell.blocks.as_slice()
        {
            if let [Inline::Text { text }] = inlines.as_slice() {
                // Keep the allocation-free path for ordinary CSV/workbook cells.
                return plain_cell(self.output, text.trim_end());
            }
        }
        let text = cell_blocks(&cell.blocks, &mut self.diagnostics, positioned);
        write_cell_text(self.output, text.trim_end())
    }
}

/// Retain only active row spans, never a second table-sized grid. Unmerged
/// tables allocate no layout storage. Both streaming and model rendering use
/// the same placement rules, independent of optional source coordinates.
pub(crate) struct TableState {
    width: usize,
    header_rows: usize,
    positioned: bool,
    spans: bool,
    occupied: Vec<usize>,
}
impl TableState {
    pub fn new(width: usize, header_rows: usize, spans: bool, source: &crate::Source) -> Self {
        Self {
            width,
            header_rows,
            spans,
            positioned: source.page.is_some() && source.part.is_none(),
            occupied: Vec::new(),
        }
    }
    fn for_rows(rows: &[Vec<crate::Cell>], header_rows: usize, source: &crate::Source) -> Self {
        let spans = rows
            .iter()
            .flatten()
            .any(|c| c.col_span != 1 || c.row_span != 1);
        let mut table = Self::new(
            rows.iter().map(Vec::len).max().unwrap_or(0),
            header_rows,
            spans,
            source,
        );
        if spans {
            for (index, row) in rows.iter().enumerate() {
                let mut width = table.width;
                for (column, cell) in table.positions(row, index) {
                    width = width.max(column + cell.col_span.max(1) as usize);
                }
                table.width = width;
            }
            table.occupied.fill(0);
        }
        table
    }
    fn positions<'a, 'b>(
        &'a mut self,
        cells: &'b [crate::Cell],
        index: usize,
    ) -> impl Iterator<Item = (usize, &'b crate::Cell)> + 'a
    where
        'b: 'a,
    {
        let mut column = 0;
        cells.iter().map(move |cell| {
            while self.occupied.get(column).is_some_and(|&end| end > index) {
                column += 1;
            }
            let start = column;
            column += cell.col_span.max(1) as usize;
            if cell.row_span > 1 {
                self.occupied.resize(self.occupied.len().max(column), 0);
                self.occupied[start..column].fill(index.saturating_add(cell.row_span as usize));
            }
            (start, cell)
        })
    }
}

fn table_diagnostics(
    diagnostics: &mut Vec<Diagnostic>,
    source: &crate::Source,
    spans: bool,
    header_rows: usize,
) {
    if spans {
        diagnostics.push(Diagnostic::new("markdown_table_spans", "Merged cells are flattened into a GFM grid with blank covered positions; use JSON to preserve spans.", source.clone(), true));
    }
    if header_rows > 1 {
        diagnostics.push(Diagnostic::new("markdown_table_headers", "Only the first header row is marked as a GFM header; remaining rows retain their text.", source.clone(), true));
    }
}

#[derive(Clone, Copy, PartialEq)]
enum TextContext {
    Block,
    Inline,
    Heading,
    Cell,
    CellFragment,
}

// Only a short line prefix can introduce a list/heading/quote: at most three
// spaces followed by at most nine ordered-list digits. Inspect the pending
// span and already-written output without rescanning an entire paragraph.
fn line_prefix(out: &str, pending: &str, context: TextContext) -> Option<u8> {
    if matches!(context, TextContext::Cell | TextContext::CellFragment) {
        return None;
    }
    let mut bytes = pending.bytes().rev().chain(out.bytes().rev());
    let mut digits = 0;
    let mut spaces = 0;
    loop {
        match bytes.next() {
            Some(b'0'..=b'9') if spaces == 0 && digits < 9 => digits += 1,
            Some(b' ') if spaces < 3 => spaces += 1,
            Some(b'\n' | b'\r') => return Some(digits),
            None if context == TextContext::Block => return Some(digits),
            _ => return None,
        }
    }
}

// Adjacent text runs share Markdown syntax even when the document model splits
// them. Borrow their bytes; stop at generated markup. A link gets a distinct
// sentinel because a preceding literal ! would otherwise turn it into an image.
// Other formatting may collapse to empty text/whitespace; keep escapes when
// syntax would otherwise depend on that unknown boundary.
fn following_bytes<'a, I: Iterator<Item = &'a Inline> + Clone>(
    items: I,
) -> impl Iterator<Item = u8> + Clone + use<'a, I> {
    items
        .scan(false, |done, item| {
            if *done {
                return None;
            }
            Some(match item {
                Inline::Text { text } => text.as_bytes(),
                Inline::Link { .. } => {
                    *done = true;
                    b"\x01"
                }
                Inline::Break => {
                    *done = true;
                    b"\n"
                }
                _ => {
                    *done = true;
                    b"\x00"
                }
            })
        })
        .flatten()
        .copied()
}

// An ATX heading can consume a trailing run of literal hashes. Protect that
// run once after rendering, rather than looking ahead from every hash in a long
// title. Only the first physical line is part of the generated ATX heading.
fn protect_heading_end(out: &mut String) {
    let lf = out.find('\n').unwrap_or(out.len());
    let cr = out[..lf].find('\r').unwrap_or(lf);
    let end = out[..cr].trim_end_matches([' ', '\t']).len();
    let start = out[..end].trim_end_matches('#').len();
    if start < end && (start == 0 || matches!(out.as_bytes()[start - 1], b' ' | b'\t')) {
        out.insert(start, '\\');
    }
}

// A small fixed character set keeps the unchanged-text scan cheap. All these
// bytes are ASCII, so candidate positions are also UTF-8 boundaries.
fn escape_candidate(b: u8) -> bool {
    matches!(
        b,
        b'\\'
            | b'`'
            | b'*'
            | b'_'
            | b'['
            | b']'
            | b'<'
            | b'>'
            | b'#'
            | b'!'
            | b'|'
            | b'~'
            | b'+'
            | b'-'
            | b'='
            | b'.'
            | b')'
            | b'&'
    )
}

// Keep the scan for long unchanged text separate from the context-sensitive
// slow path, so punctuation handling does not burden the common plain-text loop.
#[inline(never)]
fn first_escape_candidate(bytes: &[u8]) -> Option<usize> {
    let mut offset = 0;
    let (chunks, remainder) = bytes.as_chunks::<32>();
    for chunk in chunks {
        // Unlike early-exit iteration, this fixed-size fold can be vectorized.
        // Non-ASCII bytes cannot introduce Markdown syntax either.
        let punctuation = chunk
            .iter()
            .fold(false, |any, b| any | b.is_ascii_punctuation());
        if punctuation {
            if let Some(i) = chunk.iter().position(|&b| escape_candidate(b)) {
                return Some(offset + i);
            }
        }
        offset += chunk.len();
    }
    remainder
        .iter()
        .position(|&b| escape_candidate(b))
        .map(|i| offset + i)
}

fn needs_escape(
    b: u8,
    out: &str,
    pending: &str,
    context: TextContext,
    after: impl Iterator<Item = u8> + Clone,
) -> bool {
    let whitespace_or_end =
        |b: Option<u8>| b.is_none_or(|b| matches!(b, 0 | b' ' | b'\t' | b'\r' | b'\n'));
    match b {
        // Inline delimiters remain protected, including when another inline
        // supplies their matching delimiter. Braces and ordinary ! are literal.
        b'~' if matches!(context, TextContext::Cell | TextContext::CellFragment) => {
            after.clone().any(|b| b == b'~')
        }
        b'`' | b'*' | b'[' | b']' | b'|' | b'~' => true,
        b'\\' => after
            .clone()
            .next()
            .is_none_or(|b| b.is_ascii_punctuation() || matches!(b, 0 | 1 | b'\r' | b'\n')),
        b'_' => {
            let previous = pending.as_bytes().last().or_else(|| out.as_bytes().last());
            !(previous.is_some_and(u8::is_ascii_alphanumeric)
                && after
                    .clone()
                    .next()
                    .is_some_and(|b| b.is_ascii_alphanumeric()))
        }
        b'<' => after.clone().next() == Some(0) || !whitespace_or_end(after.clone().next()),
        b'!' => matches!(after.clone().next(), Some(0 | 1)),
        b'&' => {
            // Entity names/numeric references are bounded. Escaping their &
            // preserves literal source text instead of decoding it on display.
            for (length, c) in after.take(33).enumerate() {
                if c == 0 {
                    return true;
                }
                if c == b';' {
                    return length > 0;
                }
                if !(c.is_ascii_alphanumeric() || (length == 0 && c == b'#')) {
                    break;
                }
            }
            false
        }
        b'.' | b')' => {
            line_prefix(out, pending, context).is_some_and(|n| n > 0)
                && whitespace_or_end(after.clone().next())
        }
        b'+' => {
            line_prefix(out, pending, context) == Some(0) && whitespace_or_end(after.clone().next())
        }
        b'-' => {
            line_prefix(out, pending, context) == Some(0)
                && (whitespace_or_end(after.clone().next())
                    || after
                        .take_while(|b| !matches!(b, b'\r' | b'\n'))
                        .all(|b| matches!(b, 0 | b'-' | b' ' | b'\t')))
        }
        b'=' => {
            line_prefix(out, pending, context) == Some(0)
                && after
                    .take_while(|b| !matches!(b, b'\r' | b'\n'))
                    .all(|b| matches!(b, 0 | b'=' | b' ' | b'\t'))
        }
        b'>' => line_prefix(out, pending, context) == Some(0),
        b'#' => {
            if line_prefix(out, pending, context) != Some(0) {
                return false;
            }
            let mut hashes = 1;
            for b in after.take(7) {
                if b != b'#' {
                    return hashes <= 6 && whitespace_or_end(Some(b));
                }
                hashes += 1;
            }
            hashes <= 6
        }
        _ => false,
    }
}

// Pipes are parsed before inline markup in GFM. Text can use an entity,
// but code spans need a backslash escape because entities stay literal there.
fn write_cell_text(output: &mut dyn Write, text: &str) -> io::Result<()> {
    let bytes = text.as_bytes();
    let (mut start, mut i, mut slashes, mut code_fence) = (0, 0, 0, 0);
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'`' && (code_fence > 0 || slashes % 2 == 0) {
            let run = bytes[i..].iter().take_while(|&&b| b == b'`').count();
            if code_fence == 0 {
                code_fence = run;
            } else if code_fence == run {
                code_fence = 0;
            }
            i += run;
            slashes = 0;
            continue;
        }
        if b == b'|' {
            let remove = if code_fence > 0 { 0 } else { slashes % 2 };
            output.write_all(&bytes[start..i - remove])?;
            if code_fence > 0 {
                output.write_all(b"\\|")?;
            } else {
                output.write_all(b"&#124;")?;
            }
            start = i + 1;
        } else if b == b'\n' {
            output.write_all(&bytes[start..i])?;
            output.write_all(b"<br>")?;
            start = i + 1;
        }
        slashes = if b == b'\\' { slashes + 1 } else { 0 };
        i += 1;
    }
    output.write_all(&bytes[start..])
}

fn plain_cell(output: &mut dyn Write, text: &str) -> io::Result<()> {
    let bytes = text.as_bytes();
    let mut start = 0;
    for (i, &b) in bytes.iter().enumerate() {
        let replacement: &[u8] = match b {
            b'|' => b"&#124;",
            b'\n' => b"<br>",
            _ if escape_candidate(b)
                && needs_escape(
                    b,
                    "",
                    &text[..i],
                    TextContext::Cell,
                    bytes[i + 1..].iter().copied(),
                ) =>
            {
                b"\\"
            }
            _ => continue,
        };
        output.write_all(&bytes[start..i])?;
        output.write_all(replacement)?;
        if replacement == b"\\" {
            output.write_all(&bytes[i..i + 1])?;
        }
        start = i + 1;
    }
    output.write_all(&bytes[start..])
}

// ASCII syntax checks and bounded prefix lookback keep this linear. Unchanged
// UTF-8 spans go straight into the output; there is no parser or temporary copy.
#[inline]
fn escape_into<'a>(
    text: &str,
    out: &mut String,
    context: TextContext,
    following: impl Iterator<Item = &'a Inline> + Clone,
) {
    let scan_start = if text.len() >= 64 {
        let Some(i) = first_escape_candidate(text.as_bytes()) else {
            out.push_str(text);
            return;
        };
        i
    } else {
        0
    };
    let mut start = 0;
    for (i, b) in text.bytes().enumerate().skip(scan_start) {
        if !escape_candidate(b) {
            continue;
        }
        if b == b'#' && line_prefix(out, &text[start..i], context) != Some(0) {
            continue;
        }
        let after = text.as_bytes()[i + 1..]
            .iter()
            .copied()
            .chain(following_bytes(following.clone()));
        if needs_escape(b, out, &text[start..i], context, after) {
            out.push_str(&text[start..i]);
            out.push('\\');
            out.push(b as char);
            start = i + 1;
        }
    }
    out.push_str(&text[start..]);
}
fn escape(text: &str) -> String {
    let mut out = String::new();
    escape_into(text, &mut out, TextContext::Inline, std::iter::empty());
    out
}

fn safe_url(url: &str) -> bool {
    let normalized: String = url
        .chars()
        .filter(|c| !c.is_ascii_control() && !c.is_whitespace())
        .collect();
    let lower = normalized.to_ascii_lowercase();
    if lower.starts_with("//") {
        return false;
    }
    match lower.split_once(':') {
        Some((scheme, _)) => matches!(scheme, "https" | "http" | "mailto"),
        None => true,
    }
}

fn destination(url: &str) -> String {
    let mut out = String::new();
    let mut start = 0;
    for (i, b) in url.bytes().enumerate() {
        let replacement = match b {
            b' ' => "%20",
            b'(' => "%28",
            b')' => "%29",
            b'<' => "%3C",
            b'>' => "%3E",
            b'"' => "%22",
            b'\\' => "%5C",
            b'\n' => "%0A",
            b'\r' => "%0D",
            _ => continue,
        };
        out.push_str(&url[start..i]);
        out.push_str(replacement);
        start = i + 1;
    }
    out.push_str(&url[start..]);
    out
}

fn marked(value: String, marker: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return value;
    }
    let leading = value.len() - value.trim_start().len();
    let trailing = value.trim_end().len();
    format!(
        "{}{marker}{trimmed}{marker}{}",
        &value[..leading],
        &value[trailing..]
    )
}

fn formatting(item: &Inline) -> Option<(&'static str, &[Inline])> {
    match item {
        Inline::Strong { children } => Some(("**", children)),
        Inline::Emphasis { children } => Some(("*", children)),
        Inline::Strike { children } => Some(("~~", children)),
        _ => None,
    }
}

fn inlines(items: &[Inline]) -> String {
    inline_iter(items.iter(), TextContext::Block)
}

fn inline_iter<'a>(
    items: impl Iterator<Item = &'a Inline> + Clone,
    context: TextContext,
) -> String {
    let mut out = String::new();
    let mut items = items.peekable();
    while let Some(item) = items.next() {
        if let Some((marker, children)) = formatting(item) {
            // Word runs can split even a single word. Share delimiters across
            // matching spans before rendering, including recursively nested styles.
            let mut merged: Option<Vec<&Inline>> = None;
            let mut spaces = Vec::new();
            while let Some(next) = items.peek() {
                if let Some((next_marker, next_children)) = formatting(next) {
                    if next_marker != marker {
                        break;
                    }
                    let merged = merged.get_or_insert_with(|| children.iter().collect());
                    merged.append(&mut spaces);
                    merged.extend(next_children);
                } else if matches!(next, Inline::Text { text } if text.chars().all(|c| matches!(c, ' ' | '\t')))
                {
                    spaces.push(*next);
                } else {
                    break;
                }
                items.next();
            }
            let child_context = if matches!(context, TextContext::Cell | TextContext::CellFragment)
            {
                context
            } else {
                TextContext::Inline
            };
            let value = match &merged {
                Some(children) => inline_iter(children.iter().copied(), child_context),
                None => inline_iter(children.iter(), child_context),
            };
            out.push_str(&marked(value, marker));
            // Whitespace followed by different content belongs outside the span.
            for space in spaces {
                if let Inline::Text { text } = space {
                    escape_into(text, &mut out, context, items.clone());
                }
            }
            continue;
        }
        match item {
            Inline::Text { text } => escape_into(text, &mut out, context, items.clone()),
            Inline::Strong { .. } | Inline::Emphasis { .. } | Inline::Strike { .. } => {
                unreachable!("formatting spans handled above")
            }
            Inline::Code { text } => {
                let n = text.split(|c| c != '`').map(str::len).max().unwrap_or(0) + 1;
                let fence = "`".repeat(n);
                out.push_str(&format!("{fence} {} {fence}", text.replace('\n', " ")));
            }
            Inline::Link { url, children } => {
                if safe_url(url) {
                    out.push_str(&format!(
                        "[{}]({})",
                        inline_iter(
                            children.iter(),
                            if matches!(context, TextContext::Cell | TextContext::CellFragment) {
                                context
                            } else {
                                TextContext::Inline
                            }
                        ),
                        destination(url)
                    ));
                } else {
                    out.push_str(&inlines(children));
                }
            }
            Inline::Image { url, alt } => {
                if safe_url(url) {
                    let embed = !matches!(
                        url.rsplit('.').next(),
                        Some("svg" | "emf" | "wmf" | "tif" | "jp2")
                    );
                    let label = if alt.is_empty() && !embed {
                        "Image"
                    } else {
                        alt
                    };
                    out.push_str(&format!(
                        "{}[{}]({})",
                        if embed { "!" } else { "" },
                        escape(label),
                        destination(url)
                    ));
                } else {
                    escape_into(alt, &mut out, context, items.clone());
                }
            }
            Inline::Break => out.push_str(match context {
                TextContext::Cell => "<br>",
                TextContext::CellFragment => " ",
                _ => "  \n",
            }),
            Inline::Math { tex, display } => {
                let d = if *display { "$$" } else { "$" };
                out.push_str(&format!("{d}{tex}{d}"));
            }
            Inline::Anchor { id } => {
                out.push_str(&format!("<a id=\"{}\"></a>", html_text(&destination(id))))
            }
        }
    }
    if context == TextContext::Heading {
        protect_heading_end(&mut out);
    }
    out
}

fn fenced(text: &str, language: &str) -> String {
    let n = (text.split(|c| c != '`').map(str::len).max().unwrap_or(0) + 1).max(3);
    let fence = "`".repeat(n);
    let language: String = language
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '+'))
        .collect();
    format!(
        "{fence}{language}\n{}\n{fence}",
        text.strip_suffix('\n').unwrap_or(text)
    )
}

fn html_text(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
fn invisible_asset_block(block: &Block) -> bool {
    matches!(&block.content, BlockContent::Paragraph { inlines } | BlockContent::Heading { inlines, .. }
        if !inlines.is_empty() && inlines.iter().all(|i| matches!(i, Inline::Image { url, alt } if url.starts_with("asset:") && alt.is_empty())))
}

// GFM cells accept inline content only. Preserve text and inline formatting,
// using line breaks for authored blocks and spaces for positioned PDF fragments.
fn cell_blocks(items: &[Block], diagnostics: &mut Vec<Diagnostic>, positioned: bool) -> String {
    let mut out = String::new();
    for block in items {
        if invisible_asset_block(block) {
            continue;
        }
        let text = match &block.content {
            BlockContent::Paragraph { inlines } | BlockContent::Heading { inlines, .. } => {
                inline_iter(
                    inlines.iter(),
                    if positioned {
                        TextContext::CellFragment
                    } else {
                        TextContext::Cell
                    },
                )
            }
            BlockContent::Quote { blocks } => cell_blocks(blocks, diagnostics, positioned),
            BlockContent::List { start, items } => {
                let mut text = String::new();
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        text.push_str("<br>");
                    }
                    if let Some(start) = start {
                        use std::fmt::Write;
                        let _ = write!(text, "{}. ", start.saturating_add(i as u64));
                    } else {
                        text.push_str("• ");
                    }
                    text.push_str(&cell_blocks(item, diagnostics, positioned));
                }
                text
            }
            BlockContent::Table { rows, header_rows } => {
                let mut table = TableState::for_rows(rows, *header_rows, &block.source);
                table_diagnostics(diagnostics, &block.source, table.spans, *header_rows);
                let mut text = String::new();
                let width = table.width;
                for (i, row) in rows.iter().enumerate() {
                    if i > 0 {
                        text.push_str("<br>");
                    }
                    let mut positions = table.positions(row, i).peekable();
                    for column in 0..width {
                        if column > 0 {
                            text.push_str(" / ");
                        }
                        if positions.peek().is_some_and(|(c, _)| *c == column) {
                            let (_, cell) = positions.next().unwrap();
                            text.push_str(&cell_blocks(&cell.blocks, diagnostics, positioned));
                        }
                    }
                }
                text
            }
            BlockContent::Code { text, .. } | BlockContent::Raw { text, .. } => {
                let n = text.split(|c| c != '`').map(str::len).max().unwrap_or(0) + 1;
                let fence = "`".repeat(n);
                format!("{fence} {} {fence}", text.replace('\n', " "))
            }
            BlockContent::ThematicBreak => "—".into(),
        };
        let text = text.trim_end();
        if text.trim().is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push_str(if positioned { " " } else { "<br>" });
        }
        if positioned {
            for (i, word) in text.split_whitespace().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                out.push_str(word);
            }
        } else {
            out.push_str(text);
        }
    }
    out
}

fn blocks(items: &[Block], diagnostics: &mut Vec<Diagnostic>) -> String {
    blocks_in(items, diagnostics, TextContext::Block)
}

fn blocks_in(items: &[Block], diagnostics: &mut Vec<Diagnostic>, context: TextContext) -> String {
    let mut out = String::new();
    for block in items {
        if invisible_asset_block(block) {
            continue;
        }
        let content = match &block.content {
            BlockContent::Paragraph { inlines: content } => inline_iter(content.iter(), context),
            BlockContent::Heading {
                level,
                inlines: content,
            } => format!(
                "{} {}",
                "#".repeat((*level).clamp(1, 6) as usize),
                inline_iter(
                    content.iter(),
                    if matches!(context, TextContext::Cell | TextContext::CellFragment) {
                        context
                    } else {
                        TextContext::Heading
                    }
                )
            ),
            BlockContent::Code { language, text } => fenced(text, language),
            BlockContent::Raw { media_type, text } => fenced(
                text,
                if media_type == "text/html" {
                    "html"
                } else {
                    "text"
                },
            ),
            BlockContent::ThematicBreak => "---".into(),
            BlockContent::Quote { blocks: children } => blocks(children, diagnostics)
                .trim_end()
                .lines()
                .map(|line| format!("> {line}"))
                .collect::<Vec<_>>()
                .join("\n"),
            BlockContent::List { start, items } => {
                let mut lines = Vec::new();
                for (i, item) in items.iter().enumerate() {
                    let marker = start
                        .map(|n| format!("{}. ", n.saturating_add(i as u64)))
                        .unwrap_or_else(|| "- ".into());
                    let rendered = blocks(item, diagnostics);
                    let mut item_lines = rendered.trim_end().lines();
                    lines.push(format!("{marker}{}", item_lines.next().unwrap_or("")));
                    for line in item_lines {
                        lines.push(format!("{}{line}", " ".repeat(marker.len())));
                    }
                }
                lines.join("\n")
            }
            BlockContent::Table { .. } => {
                let mut bytes = Vec::new();
                let mut writer = Writer::new(&mut bytes);
                writer.block(block).expect("Vec writes cannot fail");
                diagnostics.extend(writer.diagnostics);
                String::from_utf8(bytes).expect("renderer writes UTF-8")
            }
        };
        if content.is_empty()
            && matches!(&block.content, BlockContent::Paragraph { inlines } if !inlines.is_empty() && inlines.iter().all(|i| matches!(i, Inline::Image { url, .. } if url.starts_with("asset:"))))
        {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(content.trim_end());
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod escape_tests {
    use super::*;

    #[test]
    fn adjacent_matching_styles_share_delimiters() {
        for (wrap, marker) in [
            (strong as fn(Vec<Inline>) -> Inline, "**"),
            (emphasis, "*"),
            (strike, "~~"),
        ] {
            let items = [
                wrap(vec![Inline::text("P")]),
                wrap(vec![Inline::text("rotokoll & Diskussionsunderlag")]),
            ];
            assert_eq!(
                inlines(&items),
                format!("{marker}Protokoll & Diskussionsunderlag{marker}")
            );
        }
        assert_eq!(
            inlines(&[
                strong(vec![Inline::text("On-")]),
                strong(vec![Inline::text("boarding")]),
            ]),
            r"**On-boarding**"
        );
    }

    #[test]
    fn matching_styles_bridge_spaces_and_preserve_outer_whitespace() {
        assert_eq!(
            inlines(&[
                strong(vec![Inline::text("  Avsnitt ")]),
                Inline::text(" "),
                strong(vec![Inline::text("Vinum")]),
                strong(vec![Inline::text(" Veritas  ")]),
                Inline::text("\t"),
                Inline::text("plain"),
            ]),
            "  **Avsnitt  Vinum Veritas**  \tplain"
        );
        assert_eq!(
            inlines(&[strong(vec![Inline::text("title")]), Inline::text("  ")]),
            "**title**  "
        );
    }

    #[test]
    fn matching_nested_styles_merge_without_consuming_plain_text_or_breaks() {
        assert_eq!(
            inlines(&[
                strong(vec![emphasis(vec![Inline::text("Pro")])]),
                strong(vec![emphasis(vec![Inline::text("tokoll")])]),
                Inline::text(" plain "),
                strong(vec![Inline::text("bold")]),
                Inline::Break,
                strong(vec![Inline::text("next")]),
            ]),
            "***Protokoll*** plain **bold**  \n**next**"
        );
        assert_eq!(
            inlines(&[
                strong(vec![Inline::text("bold")]),
                Inline::text(" "),
                emphasis(vec![Inline::text("italic")]),
            ]),
            "**bold** *italic*"
        );
    }

    fn strong(children: Vec<Inline>) -> Inline {
        Inline::Strong { children }
    }

    fn emphasis(children: Vec<Inline>) -> Inline {
        Inline::Emphasis { children }
    }

    fn strike(children: Vec<Inline>) -> Inline {
        Inline::Strike { children }
    }

    #[test]
    fn destinations_preserve_all_ascii_pairs_and_unicode() {
        for a in 0u8..=127 {
            for b in 0u8..=127 {
                let text = format!("é{}🦀{}中", a as char, b as char);
                let expected_url = text
                    .replace(' ', "%20")
                    .replace('(', "%28")
                    .replace(')', "%29")
                    .replace('<', "%3C")
                    .replace('>', "%3E")
                    .replace('"', "%22")
                    .replace('\\', "%5C")
                    .replace('\n', "%0A")
                    .replace('\r', "%0D");
                assert_eq!(destination(&text), expected_url);
            }
        }
        assert_eq!(destination("a%20b%2Fc"), "a%20b%2Fc");
        assert_eq!(escape(""), "");
        assert_eq!(destination(""), "");
    }

    fn plain_text(markdown: &str) -> String {
        use pulldown_cmark::{Event, Tag, TagEnd};
        let mut out = String::new();
        for event in pulldown_cmark::Parser::new_ext(
            markdown,
            pulldown_cmark::Options::ENABLE_TABLES | pulldown_cmark::Options::ENABLE_STRIKETHROUGH,
        ) {
            match event {
                Event::Start(Tag::Paragraph) | Event::End(TagEnd::Paragraph) => {}
                Event::Text(text) => out.push_str(&text),
                Event::SoftBreak => out.push('\n'),
                other => panic!("unexpected structure {other:?} in {markdown:?}"),
            }
        }
        out
    }

    #[test]
    fn ordinary_punctuation_stays_readable_across_text_runs() {
        for text in [
            "inkl. moms",
            "kl.14:00",
            "hel- och halvårsskifte",
            "+20%",
            "Kaffe+bulle",
            "Hello!",
            "3.14",
            "-12.5",
            "2026-11-21",
            "a > b",
            "C# and #tags",
            "x = {y}",
            "--option=value",
            "7.5 kg + 2.5 kg",
            "my_file.txt",
            r"C:\Users\Erik",
            "a < b",
        ] {
            assert_eq!(inlines(&[Inline::text(text)]), text);
            let split: Vec<_> = text.chars().map(|c| Inline::text(c.to_string())).collect();
            assert_eq!(inlines(&split), text, "split runs: {text}");
        }
        assert_eq!(
            inlines(&[strong(vec![Inline::text("1. title")])]),
            "**1. title**"
        );
        assert_eq!(
            inlines(&[emphasis(vec![Inline::text("- word")])]),
            "*- word*"
        );
    }

    #[test]
    fn literal_block_syntax_round_trips_without_creating_structure() {
        for text in [
            "# title",
            "###### title",
            "> quoted",
            "+ item",
            "- item",
            "1. item",
            "42) item",
            "123456789. item",
            "---",
            "- - -",
            "text\n---",
            "text\n===",
            "text\n+ item",
            "text\n1) item",
            "   - indented",
            "#",
            "+",
            "1.",
            "[link](https://example.com)",
            "![image](https://example.com/x.png)",
            "<b>literal</b>",
            "&copy; &#169; &#xA9;",
            "**bold** _italic_ ~~strike~~ `code`",
            "a | b\n--- | ---",
        ] {
            let expected = text.trim_start();
            assert_eq!(
                plain_text(&inlines(&[Inline::text(text)])),
                expected,
                "{text}"
            );
            let split: Vec<_> = text.chars().map(|c| Inline::text(c.to_string())).collect();
            assert_eq!(plain_text(&inlines(&split)), expected, "split: {text}");
        }
    }

    #[test]
    fn generated_links_stay_links_after_literal_exclamation_marks() {
        let items = [
            Inline::text("!"),
            Inline::text(""),
            Inline::Link {
                url: "https://example.com".into(),
                children: vec![Inline::text("example!")],
            },
        ];
        let md = inlines(&items);
        let events: Vec<_> = pulldown_cmark::Parser::new(&md).collect();
        assert!(events.iter().any(|e| matches!(
            e,
            pulldown_cmark::Event::Start(pulldown_cmark::Tag::Link { .. })
        )));
        assert!(!events.iter().any(|e| matches!(
            e,
            pulldown_cmark::Event::Start(pulldown_cmark::Tag::Image { .. })
        )));
        let text: String = events
            .iter()
            .filter_map(|e| {
                if let pulldown_cmark::Event::Text(t) = e {
                    Some(t.as_ref())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(text, "!example!");
    }

    #[test]
    fn heading_text_keeps_literal_closing_hashes() {
        for title in [
            "C# title #",
            "#",
            "###",
            "Title ###",
            "# start",
            "#tag",
            "Title # # #",
        ] {
            let mut parsed = crate::Parser::default()
                .parse_bytes(title.as_bytes(), Some("x.txt"))
                .unwrap();
            let BlockContent::Paragraph { inlines } = parsed.document.blocks[0].content.clone()
            else {
                panic!()
            };
            parsed.document.blocks[0].content = BlockContent::Heading { level: 2, inlines };
            let md = parsed.to_markdown().text;
            let events: Vec<_> = pulldown_cmark::Parser::new(&md).collect();
            assert!(matches!(
                events[0],
                pulldown_cmark::Event::Start(pulldown_cmark::Tag::Heading { .. })
            ));
            let actual: String = events
                .iter()
                .filter_map(|e| {
                    if let pulldown_cmark::Event::Text(t) = e {
                        Some(t.as_ref())
                    } else {
                        None
                    }
                })
                .collect();
            assert_eq!(actual, title, "{md}");
        }
    }

    #[test]
    fn printable_ascii_pairs_preserve_rendered_text_and_unicode() {
        for a in 32u8..=126 {
            for b in 32u8..=126 {
                let text = format!("é{}🦀{}中", a as char, b as char);
                assert_eq!(plain_text(&inlines(&[Inline::text(&text)])), text, "{text}");
            }
        }
    }

    #[test]
    fn long_text_scan_preserves_syntax_across_byte_chunk_boundaries() {
        for offset in [0, 31, 32, 63, 64, 127, 128] {
            for marker in [
                "*literal*",
                "&copy;",
                "<b>text</b>",
                "[x](url)",
                "_x_",
                "é🦀中",
                "3.14",
                "\\",
            ] {
                let text = format!(
                    "{}{}{}",
                    "a".repeat(offset),
                    marker,
                    " ordinary é🦀中".repeat(16)
                );
                assert_eq!(plain_text(&inlines(&[Inline::text(&text)])), text);
            }
        }
        let text = "Ordinary text é🦀中 without syntax ".repeat(64);
        assert_eq!(inlines(&[Inline::text(&text)]), text);
    }

    #[test]
    fn table_cells_keep_punctuation_and_protect_pipes_and_entities() {
        for value in [
            "1. label + 20%",
            "- label",
            "A > B",
            "my_file.txt",
            r"C:\Users\Erik",
            "# heading",
            "!",
            "&copy;",
            "&amp;#124;",
            "a|b",
            "\\|",
            "x\ny",
        ] {
            let mut output = Vec::new();
            plain_cell(&mut output, value).unwrap();
            let fast = String::from_utf8(output).unwrap();
            let csv = format!("\"{value}\"\n");
            let mut result = crate::Parser::default()
                .parse_bytes(csv.as_bytes(), Some("x.csv"))
                .unwrap();
            let normal = result.to_markdown().text;
            for block in &mut result.document.blocks {
                if let BlockContent::Table { rows, .. } = &mut block.content {
                    for cell in rows.iter_mut().flatten() {
                        for b in &mut cell.blocks {
                            if let BlockContent::Paragraph { inlines } = &mut b.content {
                                let chars: Vec<_> =
                                    value.chars().map(|c| Inline::text(c.to_string())).collect();
                                *inlines = chars;
                            }
                        }
                    }
                }
            }
            assert_eq!(result.to_markdown().text, normal, "{value}");
            let markdown = format!("| Value |\n| --- |\n| {fast} |\n");
            let mut cells = Vec::new();
            let mut cell = String::new();
            for event in
                pulldown_cmark::Parser::new_ext(&markdown, pulldown_cmark::Options::ENABLE_TABLES)
            {
                match event {
                    pulldown_cmark::Event::Start(pulldown_cmark::Tag::TableCell) => cell.clear(),
                    pulldown_cmark::Event::End(pulldown_cmark::TagEnd::TableCell) => {
                        cells.push(cell.clone())
                    }
                    pulldown_cmark::Event::Text(t) => cell.push_str(&t),
                    pulldown_cmark::Event::InlineHtml(t) if t.as_ref() == "<br>" => cell.push('\n'),
                    pulldown_cmark::Event::Start(
                        pulldown_cmark::Tag::Emphasis
                        | pulldown_cmark::Tag::Strong
                        | pulldown_cmark::Tag::Link { .. },
                    ) => panic!("unexpected markup: {markdown}"),
                    _ => {}
                }
            }
            assert_eq!(cells, vec!["Value", value], "{markdown}");
        }
    }
    #[test]
    fn empty_or_whitespace_styles_cannot_expose_literal_markdown() {
        for (left, right) in [
            ("-", "--"),
            ("text\n=", "=="),
            ("1.", " item"),
            ("#", " title"),
            ("&co", "py;"),
            ("<b", ">text</b>"),
            ("\\", "*"),
            ("!", "[image](url)"),
        ] {
            let items = [Inline::text(left), strong(vec![]), Inline::text(right)];
            assert_eq!(plain_text(&inlines(&items)), format!("{left}{right}"));
        }
        assert_eq!(
            plain_text(&inlines(&[
                Inline::text("+"),
                strong(vec![Inline::text(" ")]),
                Inline::text("item")
            ])),
            "+ item"
        );
        let md = inlines(&[
            Inline::text("\\"),
            Inline::Link {
                url: "https://example.com".into(),
                children: vec![Inline::text("link")],
            },
        ]);
        assert!(pulldown_cmark::Parser::new(&md).any(|e| matches!(
            e,
            pulldown_cmark::Event::Start(pulldown_cmark::Tag::Link { .. })
        )));
        let actual: String = pulldown_cmark::Parser::new(&md)
            .filter_map(|e| {
                if let pulldown_cmark::Event::Text(t) = e {
                    Some(t.into_string())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(actual, "\\link");
    }
    #[test]
    fn long_heading_with_separated_literal_hashes_stays_literal() {
        let source = format!("# {}", "\\# ".repeat(10000));
        let parsed = crate::Parser::default()
            .parse_bytes(source.as_bytes(), Some("x.md"))
            .unwrap();
        let markdown = parsed.to_markdown().text;
        let text: String = pulldown_cmark::Parser::new(&markdown)
            .filter_map(|e| {
                if let pulldown_cmark::Event::Text(t) = e {
                    Some(t.into_string())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(text, "# ".repeat(10000).trim_end());
    }
}
