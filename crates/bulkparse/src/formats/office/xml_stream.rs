//! Resource preflight, not a second XML interpretation. The DOM parser remains
//! authoritative for namespaces, entities, text normalization and validity.
use super::bad;
use crate::{Context, Error};
use quick_xml::{events::Event, Reader};
use std::io::{BufReader, Read};

pub(super) fn owned_utf8(bytes: Vec<u8>) -> Result<String, Error> {
    let mut text =
        String::from_utf8(bytes).map_err(|_| Error::Malformed("expected UTF-8 text".into()))?;
    if text.starts_with('\u{feff}') {
        text.drain(..3);
    }
    Ok(text)
}

/// Count decompressed bytes without retaining the XML prefix.
struct Counted<R> {
    inner: R,
    consumed: usize,
}
impl<R: Read> Read for Counted<R> {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(out)?;
        self.consumed += n;
        Ok(n)
    }
}

pub(super) fn check(input: impl Read, cap: usize, ctx: &Context<'_>) -> Result<(), Error> {
    let counted = Counted {
        inner: input.take((cap as u64).saturating_add(1)),
        consumed: 0,
    };
    let mut reader = Reader::from_reader(BufReader::new(counted));
    let mut buffer = Vec::new();
    // Count guaranteed distinct DOM slots. Consecutive text, CDATA and entity
    // events coalesce into one text node; empty CDATA is conservatively omitted.
    let mut nodes = 1usize; // virtual document root
    let mut depth = 0usize;
    let mut after_text = false;
    loop {
        ctx.check()?;
        let event = reader.read_event_into(&mut buffer);
        if reader.get_ref().get_ref().consumed > cap {
            return Err(Error::Limit("max_expanded_bytes"));
        }
        let node = match event.map_err(bad)? {
            Event::Start(_) => {
                depth += 1;
                after_text = false;
                true
            }
            Event::Empty(_) | Event::Comment(_) | Event::PI(_) => {
                after_text = false;
                true
            }
            Event::End(_) => {
                depth = depth.saturating_sub(1);
                after_text = false;
                false
            }
            Event::Text(e) if depth > 0 && !e.is_empty() => {
                let new = !after_text;
                after_text = true;
                new
            }
            Event::CData(e) if depth > 0 && !e.is_empty() => {
                let new = !after_text;
                after_text = true;
                new
            }
            Event::GeneralRef(_) if depth > 0 => {
                let new = !after_text;
                after_text = true;
                new
            }
            Event::DocType(_) => return Err(bad("XML DTD is not allowed")),
            Event::Eof => break,
            _ => false,
        };
        if node {
            nodes = nodes.saturating_add(1);
            if !ctx.fits(nodes, 0, 0) {
                return Err(Error::Limit("max_nodes"));
            }
        }
        buffer.clear();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CancellationToken, Options};
    use std::cell::Cell;

    #[test]
    fn rejects_large_xml_before_reading_it_all() {
        struct Counted<'a> {
            input: &'a [u8],
            read: &'a Cell<usize>,
        }
        impl Read for Counted<'_> {
            fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
                let n = self.input.read(out)?;
                self.read.set(self.read.get() + n);
                Ok(n)
            }
        }
        let xml = format!("<root>{}</root>", "<cell>value</cell>".repeat(100_000));
        let consumed = Cell::new(0);
        let mut options = Options::default();
        options.limits.max_nodes = 100;
        let cancel = CancellationToken::default();
        let ctx = Context::new(&options, &cancel);
        let result = check(
            Counted {
                input: xml.as_bytes(),
                read: &consumed,
            },
            xml.len(),
            &ctx,
        );
        assert!(matches!(result, Err(Error::Limit("max_nodes"))));
        assert!(consumed.get() < xml.len() / 100);
    }

    #[test]
    fn preserves_text_entities_cdata_bom_and_cumulative_budget() {
        let xml = "\u{feff}<r>a&amp;b<![CDATA[c]]>&#100;<!-- x --><?pi x?><e/></r>";
        let mut options = Options::default();
        options.limits.max_nodes = 7;
        let cancel = CancellationToken::default();
        let mut ctx = Context::new(&options, &cancel);
        ctx.node().unwrap(); // previous part consumed one slot
        check(xml.as_bytes(), xml.len(), &ctx).unwrap();
        let text = owned_utf8(xml.as_bytes().to_vec()).unwrap();
        let doc = super::super::xml(&text, &mut ctx).unwrap();
        assert_eq!(doc.root_element().text(), Some("a&bcd"));
        assert!(ctx.fits(0, 0, 0));
        assert!(!ctx.fits(1, 0, 0));
        assert!(matches!(
            check(b"<r><a/><b/></r>".as_slice(), 100, &ctx),
            Err(Error::Limit("max_nodes"))
        ));
    }

    #[test]
    fn checks_expansion_cancellation_and_invalid_utf8() {
        let options = Options::default();
        let cancel = CancellationToken::default();
        let ctx = Context::new(&options, &cancel);
        assert!(matches!(
            check(b"<root/>".as_slice(), 5, &ctx),
            Err(Error::Limit("max_expanded_bytes"))
        ));
        assert!(matches!(owned_utf8(vec![255]), Err(Error::Malformed(_))));
        cancel.cancel();
        assert!(matches!(
            check(b"<root/>".as_slice(), 100, &ctx),
            Err(Error::Cancelled)
        ));
    }
}
