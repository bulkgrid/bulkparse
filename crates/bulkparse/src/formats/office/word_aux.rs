//! Recover text from binary Word auxiliary stories without retaining another
//! document model or decoding image/object payloads. Layout stays explicitly partial.
use super::{legacy::u32le, *};
struct Story {
    label: &'static str,
    start: u32,
    end: u32,
    text: String,
    pictures: BTreeMap<usize, u32>,
}
pub(super) struct Stories {
    parts: Vec<Story>,
    bytes: usize,
}
impl Stories {
    pub fn new(fib: &[u8], main: u32) -> Result<Self, Error> {
        let mut cursor = main;
        let mut parts = Vec::new();
        for (offset, label) in [
            (0x50, "Footnotes"),
            (0x54, "Headers and footers"),
            (0x58, "Macro text"),
            (0x5c, "Comments"),
            (0x60, "Endnotes"),
            (0x64, "Text boxes"),
            (0x68, "Header text boxes"),
        ] {
            let len = u32le(fib, offset)?;
            let end = cursor
                .checked_add(len)
                .ok_or_else(|| bad("Word story range overflow"))?;
            if len > 0 {
                parts.push(Story {
                    label,
                    start: cursor,
                    end,
                    text: String::new(),
                    pictures: BTreeMap::new(),
                });
            }
            cursor = end;
        }
        Ok(Self { parts, bytes: 0 })
    }
    pub fn append_piece(
        &mut self,
        word: &[u8],
        range: (u32, u32),
        encoded_offset: u32,
        encoding: &'static encoding_rs::Encoding,
        main_bytes: usize,
        ctx: &Context<'_>,
    ) -> Result<(), Error> {
        let compressed = encoded_offset & 0x40000000 != 0;
        let width = if compressed { 1usize } else { 2usize };
        let base = (encoded_offset & 0x3fffffff) as usize / if compressed { 2 } else { 1 };
        for story in &mut self.parts {
            let start = range.0.max(story.start);
            let end = range.1.min(story.end);
            if start >= end {
                continue;
            }
            ctx.check()?;
            let units = (end - start) as usize;
            if units
                > ctx
                    .options
                    .limits
                    .max_text_bytes
                    .saturating_sub(main_bytes.saturating_add(self.bytes))
            {
                return Err(Error::Limit("max_text_bytes"));
            }
            let displacement = ((start - range.0) as usize)
                .checked_mul(width)
                .ok_or_else(|| bad("Word story offset overflow"))?;
            let first = base
                .checked_add(displacement)
                .ok_or_else(|| bad("Word story offset overflow"))?;
            let len = units
                .checked_mul(width)
                .ok_or_else(|| bad("Word story length overflow"))?;
            let slice = word
                .get(
                    first
                        ..first
                            .checked_add(len)
                            .ok_or_else(|| bad("Word story length overflow"))?,
                )
                .ok_or_else(|| bad("Word auxiliary story outside stream"))?;
            let text = if compressed {
                let (value, errors) = encoding.decode_without_bom_handling(slice);
                if errors {
                    return Err(bad("invalid Word auxiliary encoding"));
                }
                value.into_owned()
            } else {
                super::legacy::utf16(slice)?
            };
            self.bytes = self.bytes.saturating_add(text.len());
            if main_bytes.saturating_add(self.bytes) > ctx.options.limits.max_text_bytes {
                return Err(Error::Limit("max_text_bytes"));
            }
            // Keep only sparse image-character positions, not a second offset per glyph.
            let file_positions: Box<dyn Iterator<Item = usize>> = if compressed {
                Box::new(
                    slice
                        .iter()
                        .enumerate()
                        .filter(|(_, b)| matches!(**b, 1 | 8))
                        .map(|(i, _)| first + i),
                )
            } else {
                Box::new(
                    slice
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .enumerate()
                        .filter(|(_, b)| matches!(u16::from_le_bytes([b[0], b[1]]), 1 | 8))
                        .map(|(i, _)| first + i * 2),
                )
            };
            for ((byte, _), file) in text
                .char_indices()
                .filter(|(_, c)| matches!(c, '\u{1}' | '\u{8}'))
                .zip(file_positions)
            {
                if let Ok(file) = u32::try_from(file) {
                    story.pictures.insert(story.text.len() + byte, file);
                }
            }
            story.text.push_str(&text);
        }
        Ok(())
    }
    pub fn emit(
        self,
        final_cp: u32,
        formatting: &super::word_styles::Formatting,
        ctx: &mut Context<'_>,
        out: &mut Vec<Block>,
    ) -> Result<(), Error> {
        for story in self.parts {
            let source = Source {
                part: Some(format!("WordDocument/{}", story.label).into()),
                ..Default::default()
            };
            if final_cp < story.end {
                warn(
                    ctx,
                    "doc_auxiliary_incomplete",
                    "Auxiliary story range is incomplete; available text retained.",
                    &source,
                );
            }
            let mut blocks = Vec::new();
            let mut line = String::new();
            let mut fields = Vec::new();
            let mut inlines = Vec::new();
            for (byte, c) in story.text.char_indices() {
                match c {
                    '\u{13}' => {
                        ctx.depth(fields.len() + 1)?;
                        fields.push(false);
                    }
                    '\u{14}' => {
                        if let Some(v) = fields.last_mut() {
                            *v = true;
                        }
                    }
                    '\u{15}' => {
                        fields.pop();
                    }
                    _ if fields.iter().all(|v| *v) => match c {
                        '\r' => {
                            emit_line(&mut line, &mut inlines, &source, ctx, &mut blocks)?;
                        }
                        '\u{1}' | '\u{8}' => {
                            let image = story
                                .pictures
                                .get(&byte)
                                .and_then(|fc| formatting.picture_at(*fc))
                                .and_then(|offset| {
                                    ctx.image_inline(&format!("Data/picture-{offset}"), "")
                                });
                            if let Some(image) = image {
                                if !line.is_empty() {
                                    ctx.text(&line)?;
                                    inlines.push(Inline::text(std::mem::take(&mut line)));
                                }
                                inlines.push(image);
                            } else {
                                warn(ctx, "doc_object", "Inline object is not reconstructed in the text flow; supported image payloads are retained separately when enabled.", &source);
                            }
                        }
                        '\u{7}' => line.push('\t'),
                        '\u{b}' | '\u{c}' => line.push('\n'),
                        _ if c >= ' ' || c == '\t' => line.push(c),
                        _ => {}
                    },
                    _ => {}
                }
            }
            emit_line(&mut line, &mut inlines, &source, ctx, &mut blocks)?;
            if !blocks.is_empty() {
                out.push(heading(story.label.into(), 3, &source, ctx)?);
                out.extend(blocks);
                warn(ctx,"doc_auxiliary_layout","Auxiliary text appended by story; individual note links, page placement, rich styles and table layout are not reconstructed.",&source);
            }
        }
        Ok(())
    }
}

fn emit_line(
    line: &mut String,
    inlines: &mut Vec<Inline>,
    source: &Source,
    ctx: &mut Context<'_>,
    blocks: &mut Vec<Block>,
) -> Result<(), Error> {
    if inlines.is_empty() {
        if !line.trim().is_empty() {
            blocks.push(para(std::mem::take(line), source, ctx)?);
        } else {
            line.clear();
        }
    } else {
        // An image-only paragraph may contain spacer whitespace. Keep the image
        // reference without introducing empty paragraphs in text-only Markdown.
        if line.trim().is_empty()
            && inlines.iter().all(|i| {
                matches!(i, Inline::Image { .. })
                    || matches!(i, Inline::Text { text } if text.trim().is_empty())
            })
        {
            line.clear();
            inlines.retain(|i| matches!(i, Inline::Image { .. }));
        }
        if !line.is_empty() {
            ctx.text(line)?;
            inlines.push(Inline::text(std::mem::take(line)));
        }
        blocks.push(ctx.block(
            BlockContent::Paragraph {
                inlines: std::mem::take(inlines),
            },
            source.clone(),
        )?);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn image_only_story_paragraph_ignores_surrounding_spacer_text() {
        let options = crate::Options::default();
        let token = crate::CancellationToken::default();
        let mut ctx = Context::new(&options, &token);
        let mut line = " \t".to_owned();
        let mut inlines = vec![
            Inline::text(" "),
            Inline::Image {
                url: "asset:test.png".into(),
                alt: String::new(),
            },
        ];
        let mut out = vec![];
        emit_line(
            &mut line,
            &mut inlines,
            &Source::default(),
            &mut ctx,
            &mut out,
        )
        .unwrap();
        assert!(
            matches!(&out[0].content,BlockContent::Paragraph {inlines} if inlines.len()==1 && matches!(inlines[0],Inline::Image {..}))
        );
    }
}
