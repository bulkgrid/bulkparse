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
    pub fn table_start(&mut self, native: bool) -> io::Result<()> {
        if native {
            self.begin()?;
            self.output.write_all(b"<table>\n")?;
        }
        Ok(())
    }
    pub fn table_row(
        &mut self,
        row: &Vec<crate::Cell>,
        index: usize,
        header_rows: usize,
        width: usize,
        native: bool,
        positioned: bool,
    ) -> io::Result<()> {
        if native {
            let text = native_table(
                std::slice::from_ref(row),
                usize::from(index < header_rows),
                positioned,
                &mut self.diagnostics,
            );
            self.output.write_all(
                text.strip_prefix("<table>\n")
                    .unwrap()
                    .strip_suffix("</table>")
                    .unwrap()
                    .as_bytes(),
            )
        } else {
            self.record(row, index, header_rows > 0, width)
        }
    }
    pub fn table_end(&mut self, native: bool) -> io::Result<()> {
        if native {
            self.output.write_all(b"</table>\n")?;
        }
        Ok(())
    }
    pub fn block(&mut self, block: &Block) -> io::Result<()> {
        if let BlockContent::Table { rows, header_rows } = &block.content {
            let native = (*header_rows == 0 && block.source.page.is_some())
                || *header_rows > 1
                || rows.iter().flatten().any(|c| {
                    c.row_span > 1
                        || c.col_span > 1
                        || c.blocks.iter().any(|b| {
                            !matches!(
                                b.content,
                                BlockContent::Paragraph { .. } | BlockContent::Heading { .. }
                            )
                        })
                });
            if native {
                self.begin()?;
                self.output.write_all(b"<table>\n")?;
                for (i, row) in rows.iter().enumerate() {
                    let text = native_table(
                        std::slice::from_ref(row),
                        usize::from(i < *header_rows),
                        block.source.page.is_some() && block.source.part.is_none(),
                        &mut self.diagnostics,
                    );
                    self.output.write_all(
                        text.strip_prefix("<table>\n")
                            .unwrap()
                            .strip_suffix("</table>")
                            .unwrap()
                            .as_bytes(),
                    )?;
                }
                return self.output.write_all(b"</table>\n");
            }
            let width = rows.iter().map(Vec::len).max().unwrap_or(0);
            if width == 0 {
                return Ok(());
            }
            // Preserve the renderer's diagnostic for externally supplied models
            // containing invalid zero spans. Parser-produced spans are positive.
            if rows
                .iter()
                .flatten()
                .any(|c| c.col_span != 1 || c.row_span != 1)
            {
                self.diagnostics.push(Diagnostic::new("markdown_table_spans", "Merged cells are flattened to one GFM cell per source cell; use JSON to preserve spans.", block.source.clone(), true));
            }
            for (i, row) in rows.iter().enumerate() {
                self.record(row, i, *header_rows > 0, width)?;
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
            self.output.write_all(b"| ")?;
            for i in 0..width {
                if i > 0 {
                    self.output.write_all(b" | ")?;
                }
                self.output.write_all(b"---")?;
            }
            self.output.write_all(b" |\n")?;
            if header {
                return Ok(());
            }
        }
        self.row(cells, width)
    }
    fn row(&mut self, cells: &[crate::Cell], width: usize) -> io::Result<()> {
        self.output.write_all(b"| ")?;
        for i in 0..width {
            if i > 0 {
                self.output.write_all(b" | ")?;
            }
            if let Some(cell) = cells.get(i) {
                if let [Block {
                    content: BlockContent::Paragraph { inlines },
                    ..
                }] = cell.blocks.as_slice()
                {
                    if let [Inline::Text { text }] = inlines.as_slice() {
                        // Plain spreadsheet/CSV values need no intermediate
                        // Markdown strings. Escape directly into the writer,
                        // preserving the same trimming, pipe and newline rules.
                        plain_cell(self.output, text.trim_end())?;
                        continue;
                    }
                }
                let text = blocks(&cell.blocks, &mut self.diagnostics);
                let text = text
                    .trim_end()
                    .replace("\\|", "&#124;")
                    .replace('|', "&#124;")
                    .replace('\n', "<br>");
                self.output.write_all(text.as_bytes())?;
            }
        }
        self.output.write_all(b" |\n")
    }
}

fn plain_cell(output: &mut dyn Write, text: &str) -> io::Result<()> {
    let bytes = text.as_bytes();
    let mut start = 0;
    for (i, &b) in bytes.iter().enumerate() {
        let replacement: &[u8] = match b {
            b'|' => b"&#124;",
            b'\n' => b"<br>",
            b'\\' | b'`' | b'*' | b'_' | b'{' | b'}' | b'[' | b']' | b'<' | b'>' | b'#' | b'!'
            | b'~' | b'+' | b'-' | b'=' | b'.' => b"\\",
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

// Escapable Markdown characters are ASCII, so every split is also a UTF-8
// boundary. Append unchanged spans directly instead of allocating per inline.
fn escape_into(text: &str, out: &mut String) {
    let mut start = 0;
    for (i, b) in text.bytes().enumerate() {
        if matches!(
            b,
            b'\\'
                | b'`'
                | b'*'
                | b'_'
                | b'{'
                | b'}'
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
        ) {
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
    escape_into(text, &mut out);
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

fn marked(children: &[Inline], marker: &str) -> String {
    let value = inlines(children);
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

fn inlines(items: &[Inline]) -> String {
    let mut out = String::new();
    for item in items {
        match item {
            Inline::Text { text } => escape_into(text, &mut out),
            Inline::Strong { children } => out.push_str(&marked(children, "**")),
            Inline::Emphasis { children } => out.push_str(&marked(children, "*")),
            Inline::Strike { children } => out.push_str(&marked(children, "~~")),
            Inline::Code { text } => {
                let n = text.split(|c| c != '`').map(str::len).max().unwrap_or(0) + 1;
                let fence = "`".repeat(n);
                out.push_str(&format!("{fence} {} {fence}", text.replace('\n', " ")));
            }
            Inline::Link { url, children } => {
                if safe_url(url) {
                    out.push_str(&format!("[{}]({})", inlines(children), destination(url)));
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
                    escape_into(alt, &mut out);
                }
            }
            Inline::Break => out.push_str("  \n"),
            Inline::Math { tex, display } => {
                let d = if *display { "$$" } else { "$" };
                out.push_str(&format!("{d}{tex}{d}"));
            }
            Inline::Anchor { id } => {
                out.push_str(&format!("<a id=\"{}\"></a>", html_text(&destination(id))))
            }
        }
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
fn html_inlines(items: &[Inline]) -> String {
    let mut out = String::new();
    for item in items {
        match item {
            Inline::Text { text } => out.push_str(&html_text(text)),
            Inline::Code { text } => out.push_str(&format!("<code>{}</code>", html_text(text))),
            Inline::Strong { children } => {
                out.push_str(&format!("<strong>{}</strong>", html_inlines(children)))
            }
            Inline::Emphasis { children } => {
                out.push_str(&format!("<em>{}</em>", html_inlines(children)))
            }
            Inline::Strike { children } => {
                out.push_str(&format!("<del>{}</del>", html_inlines(children)))
            }
            Inline::Link { url, children } => {
                if safe_url(url) {
                    out.push_str(&format!(
                        "<a href=\"{}\">{}</a>",
                        html_text(&destination(url)),
                        html_inlines(children)
                    ))
                } else {
                    out.push_str(&html_inlines(children))
                }
            }
            Inline::Image { url, alt } => {
                if safe_url(url) {
                    let url = html_text(&destination(url));
                    let alt = html_text(alt).replace('"', "&quot;");
                    out.push_str(&format!("<img src=\"{url}\" alt=\"{alt}\">"));
                } else {
                    out.push_str(&html_text(alt));
                }
            }
            Inline::Break => out.push_str("<br>"),
            Inline::Math { tex, display } => {
                let d = if *display { "$$" } else { "$" };
                out.push_str(&format!("{d}{}{d}", html_text(tex)));
            }
            Inline::Anchor { id } => {
                out.push_str(&format!("<a id=\"{}\"></a>", html_text(&destination(id))))
            }
        }
    }
    out
}
fn invisible_asset_block(block: &Block) -> bool {
    matches!(&block.content, BlockContent::Paragraph { inlines } | BlockContent::Heading { inlines, .. }
        if !inlines.is_empty() && inlines.iter().all(|i| matches!(i, Inline::Image { url, alt } if url.starts_with("asset:") && alt.is_empty())))
}

fn html_blocks(items: &[Block], diagnostics: &mut Vec<Diagnostic>) -> String {
    let mut out = String::new();
    for b in items {
        if invisible_asset_block(b) {
            continue;
        }
        match &b.content {
            BlockContent::Paragraph { inlines } => {
                out.push_str(&format!("<p>{}</p>", html_inlines(inlines)))
            }
            BlockContent::Heading { level, inlines } => {
                let l = level.clamp(&1, &6);
                out.push_str(&format!("<h{l}>{}</h{l}>", html_inlines(inlines)));
            }
            BlockContent::Code { text, .. } | BlockContent::Raw { text, .. } => {
                out.push_str(&format!("<pre><code>{}</code></pre>", html_text(text)))
            }
            BlockContent::Quote { blocks } => out.push_str(&format!(
                "<blockquote>{}</blockquote>",
                html_blocks(blocks, diagnostics)
            )),
            BlockContent::List { start, items } => {
                if let Some(n) = start {
                    out.push_str(&format!("<ol start=\"{n}\">"));
                } else {
                    out.push_str("<ul>");
                }
                for item in items {
                    out.push_str(&format!("<li>{}</li>", html_blocks(item, diagnostics)));
                }
                out.push_str(if start.is_some() { "</ol>" } else { "</ul>" });
            }
            BlockContent::Table { rows, header_rows } => {
                out.push_str(&native_table(rows, *header_rows, false, diagnostics))
            }
            BlockContent::ThematicBreak => out.push_str("<hr>"),
        }
    }
    out
}
fn native_table(
    rows: &[Vec<crate::Cell>],
    header_rows: usize,
    positioned_fragments: bool,
    diagnostics: &mut Vec<Diagnostic>,
) -> String {
    let mut rendered = String::from("<table>\n");
    for (index, row) in rows.iter().enumerate() {
        rendered.push_str("<tr>");
        for cell in row {
            let tag = if index < header_rows { "th" } else { "td" };
            rendered.push('<');
            rendered.push_str(tag);
            if cell.col_span > 1 {
                rendered.push_str(&format!(" colspan=\"{}\"", cell.col_span));
            }
            if cell.row_span > 1 {
                rendered.push_str(&format!(" rowspan=\"{}\"", cell.row_span));
            }
            rendered.push('>');
            // Bare page provenance describes positioned text fragments, not
            // package-authored paragraph boundaries. Keep their existing spacing.
            let mut visible = cell.blocks.iter().filter(|b| !invisible_asset_block(b));
            let first = visible.next();
            let single = visible.next().is_none();
            let content = if positioned_fragments {
                cell.blocks
                    .iter()
                    .map(|block| match &block.content {
                        BlockContent::Paragraph { inlines }
                        | BlockContent::Heading { inlines, .. } => {
                            html_inlines(inlines).replace("<br>", " ")
                        }
                        _ => html_text(blocks(std::slice::from_ref(block), diagnostics).trim()),
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            } else if let Some(Block {
                content: BlockContent::Paragraph { inlines } | BlockContent::Heading { inlines, .. },
                ..
            }) = first.filter(|_| single)
            {
                html_inlines(inlines)
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            } else {
                html_blocks(&cell.blocks, diagnostics)
            };
            rendered.push_str(&content);
            rendered.push_str(&format!("</{tag}>"));
        }
        rendered.push_str("</tr>\n");
    }
    rendered.push_str("</table>");
    rendered
}

fn blocks(items: &[Block], diagnostics: &mut Vec<Diagnostic>) -> String {
    let mut out = String::new();
    for block in items {
        if invisible_asset_block(block) {
            continue;
        }
        let content = match &block.content {
            BlockContent::Paragraph { inlines: content } => inlines(content),
            BlockContent::Heading {
                level,
                inlines: content,
            } => format!(
                "{} {}",
                "#".repeat((*level).clamp(1, 6) as usize),
                inlines(content)
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
            BlockContent::Table { rows, header_rows }
                if (*header_rows == 0 && block.source.page.is_some())
                    || *header_rows > 1
                    || rows.iter().flatten().any(|c| {
                        c.row_span > 1
                            || c.col_span > 1
                            || c.blocks.iter().any(|b| {
                                !matches!(
                                    b.content,
                                    BlockContent::Paragraph { .. } | BlockContent::Heading { .. }
                                )
                            })
                    }) =>
            {
                native_table(
                    rows,
                    *header_rows,
                    block.source.page.is_some() && block.source.part.is_none(),
                    diagnostics,
                )
            }
            BlockContent::Table { rows, header_rows } => {
                let width = rows.iter().map(Vec::len).max().unwrap_or(0);
                if rows.is_empty() || width == 0 {
                    continue;
                }
                if rows
                    .iter()
                    .flatten()
                    .any(|c| c.col_span != 1 || c.row_span != 1)
                {
                    diagnostics.push(Diagnostic::new("markdown_table_spans", "Merged cells are flattened to one GFM cell per source cell; use JSON to preserve spans.", block.source.clone(), true));
                }
                if *header_rows > 1 {
                    diagnostics.push(Diagnostic::new("markdown_table_headers", "Only the first header row is marked as a GFM header; remaining rows retain their text.", block.source.clone(), true));
                }
                let mut rendered = String::new();
                let row_text =
                    |cells: &[crate::Cell], diagnostics: &mut Vec<Diagnostic>| -> String {
                        let mut values = Vec::new();
                        for cell in cells {
                            let text = blocks(&cell.blocks, diagnostics);
                            // Inline rendering already escapes raw HTML and pipes.
                            let text = text
                                .trim_end()
                                .replace("\\|", "&#124;")
                                .replace('|', "&#124;")
                                .replace('\n', "<br>");
                            values.push(text);
                        }
                        values.resize(width, String::new());
                        format!("| {} |\n", values.join(" | "))
                    };
                let first = if *header_rows > 0 {
                    row_text(&rows[0], diagnostics)
                } else {
                    format!("| {} |\n", vec![""; width].join(" | "))
                };
                rendered.push_str(&first);
                rendered.push_str(&format!("| {} |\n", vec!["---"; width].join(" | ")));
                for row in rows.iter().skip(usize::from(*header_rows > 0)) {
                    rendered.push_str(&row_text(row, diagnostics));
                }
                rendered.trim_end().to_owned()
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
    fn span_escaping_preserves_all_ascii_pairs_and_unicode() {
        for a in 0u8..=127 {
            for b in 0u8..=127 {
                let text = format!("é{}🦀{}中", a as char, b as char);
                let mut expected = String::from("prefix:");
                for c in text.chars() {
                    if "\\`*_{}[]<>#!|~+-=.".contains(c) {
                        expected.push('\\');
                    }
                    expected.push(c);
                }
                let mut actual = String::from("prefix:");
                escape_into(&text, &mut actual);
                assert_eq!(actual, expected);
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
}
