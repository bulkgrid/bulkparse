use super::*;
fn anchor(part: &str, fragment: &str) -> String {
    // Length-delimited UTF-8 bytes avoid slug collisions, including case and punctuation.
    let mut id = format!("epub-{}-", part.len());
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for b in part.bytes().chain(fragment.bytes()) {
        id.push(HEX[usize::from(b >> 4)] as char);
        id.push(HEX[usize::from(b & 15)] as char);
    }
    id
}

#[cfg(test)]
mod anchor_tests {
    use super::anchor;

    #[test]
    fn anchors_keep_utf8_encoding_and_part_boundaries() {
        assert_eq!(anchor("a.xhtml", "b"), "epub-7-612e7868746d6c62");
        assert_eq!(anchor("é", "Ω"), "epub-2-c3a9cea9");
        assert_ne!(anchor("ab", "c"), anchor("a", "bc"));
        assert_ne!(anchor("A", ""), anchor("a", ""));
    }
}
fn target(part: &str, href: &str) -> Result<String, Error> {
    if href.contains(':') || href.starts_with("//") {
        return Ok(href.into());
    }
    let (path, fragment) = href.split_once('#').unwrap_or((href, ""));
    let decode = |s: &str| -> Result<String, Error> {
        let mut bytes = vec![];
        let mut i = 0;
        while i < s.len() {
            if s.as_bytes()[i] == b'%' {
                let h = s
                    .get(i + 1..i + 3)
                    .ok_or_else(|| bad("invalid URI escape"))?;
                bytes.push(u8::from_str_radix(h, 16).map_err(bad)?);
                i += 3;
            } else {
                bytes.push(s.as_bytes()[i]);
                i += 1;
            }
        }
        String::from_utf8(bytes).map_err(bad)
    };
    let path = if path.is_empty() {
        part.into()
    } else {
        resolve(part, &decode(path)?)?
    };
    let fragment = decode(fragment)?;
    Ok(if fragment.is_empty() {
        path
    } else {
        format!("{path}#{fragment}")
    })
}
fn own_anchor(
    n: Node<'_, '_>,
    src: &Source,
    ctx: &mut Context<'_>,
) -> Result<Option<Inline>, Error> {
    let result = n
        .attribute("id")
        .or_else(|| {
            if named(n, "a") {
                n.attribute("name")
            } else {
                None
            }
        })
        .map(|id| Inline::Anchor {
            id: anchor(src.part.as_deref().unwrap_or(""), id),
        });
    if let Some(Inline::Anchor { id }) = &result {
        ctx.text(id)?;
    }
    Ok(result)
}
fn visit(blocks: &mut [Block], f: &mut impl FnMut(&mut Inline, &Source)) {
    fn spans(items: &mut [Inline], src: &Source, f: &mut impl FnMut(&mut Inline, &Source)) {
        for i in items {
            f(i, src);
            match i {
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::Strike { children }
                | Inline::Link { children, .. } => spans(children, src, f),
                _ => {}
            }
        }
    }
    for block in blocks {
        match &mut block.content {
            BlockContent::Paragraph { inlines } | BlockContent::Heading { inlines, .. } => {
                spans(inlines, &block.source, f)
            }
            BlockContent::Quote { blocks } => visit(blocks, f),
            BlockContent::List { items, .. } => {
                for item in items {
                    visit(item, f)
                }
            }
            BlockContent::Table { rows, .. } => {
                for cell in rows.iter_mut().flatten() {
                    visit(&mut cell.blocks, f)
                }
            }
            _ => {}
        }
    }
}

fn inline(
    n: Node<'_, '_>,
    ctx: &mut Context<'_>,
    src: &Source,
    css: &super::css::Css,
) -> Result<Vec<Inline>, Error> {
    let mut out = vec![];
    let style = css.get(n);
    if style.hidden == Some(true) {
        return Ok(out);
    }
    if let Some(id) = own_anchor(n, src, ctx)? {
        out.push(id);
    }
    if named(n, "math") {
        out.push(super::math::inline(n, ctx, src)?);
        return Ok(out);
    }
    for c in n.children() {
        if c.is_text() {
            let raw = c.text().unwrap_or("");
            let text = if style.pre == Some(true) {
                raw.into()
            } else {
                let mut t = String::new();
                let mut whitespace = false;
                for ch in raw.chars() {
                    if ch.is_whitespace() {
                        if !whitespace {
                            t.push(' ')
                        }
                        whitespace = true;
                    } else {
                        t.push(ch);
                        whitespace = false
                    }
                }
                t
            };
            ctx.text(&text)?;
            out.push(style.text(text));
            continue;
        }
        if !c.is_element()
            || css.get(c).hidden == Some(true)
            || matches!(c.tag_name().name(), "script" | "style")
        {
            continue;
        }
        match c.tag_name().name() {
            "br" => out.push(Inline::Break),
            "img" => {
                let alt = c.attribute("alt").unwrap_or("");
                ctx.text(alt)?;
                let image = c
                    .attribute("src")
                    .and_then(|href| resolve(src.part.as_deref().unwrap_or(""), href).ok())
                    .and_then(|path| ctx.image_inline(&path, alt));
                if let Some(image) = image {
                    out.push(image);
                } else if !alt.is_empty() {
                    out.push(Inline::text(alt));
                }
                warn(
                    ctx,
                    "epub_image",
                    "Image placement is not reconstructed; alternative text retained. Supported package image files are retained separately when enabled.",
                    src,
                );
            }
            _ => {
                let children = inline(c, ctx, src, css)?;
                if named(c, "a") {
                    if let Some(url) = c.attribute("href") {
                        let url = match target(src.part.as_deref().unwrap_or(""), url) {
                            Ok(v) => v,
                            Err(_) => {
                                warn(
                                    ctx,
                                    "epub_link_target",
                                    "Malformed internal link retained without resolution.",
                                    src,
                                );
                                url.into()
                            }
                        };
                        ctx.text(&url)?;
                        out.push(Inline::Link { url, children });
                    } else {
                        out.extend(children)
                    }
                } else {
                    out.extend(children)
                }
            }
        }
    }
    Ok(out)
}
fn blocks(
    n: Node<'_, '_>,
    src: &Source,
    ctx: &mut Context<'_>,
    depth: usize,
    css: &super::css::Css,
) -> Result<Vec<Block>, Error> {
    ctx.depth(depth)?;
    if css.get(n).hidden == Some(true) {
        return Ok(vec![]);
    }
    let mut out = vec![];
    if let Some(id) = own_anchor(n, src, ctx)? {
        out.push(ctx.block(BlockContent::Paragraph { inlines: vec![id] }, src.clone())?);
    }
    for c in n.children() {
        if c.is_text() {
            let t = c.text().unwrap_or("").trim();
            if !t.is_empty() {
                out.push(para(t.into(), src, ctx)?)
            }
            continue;
        }
        if !c.is_element() {
            continue;
        }
        let name = c.tag_name().name();
        if matches!(name, "script" | "style" | "head") {
            continue;
        }
        if css.get(c).hidden == Some(true) {
            continue;
        }
        if matches!(name, "table" | "pre" | "ul" | "ol" | "hr" | "img") {
            let nodes: Vec<_> = if name == "table" {
                c.descendants().filter(Node::is_element).collect()
            } else {
                vec![c]
            };
            for node in nodes {
                if let Some(id) = own_anchor(node, src, ctx)? {
                    out.push(
                        ctx.block(BlockContent::Paragraph { inlines: vec![id] }, src.clone())?,
                    );
                }
            }
        }
        match name {
            "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let spans = inline(c, ctx, src, css)?;
                out.push(ctx.block(
                    if name.starts_with('h') {
                        BlockContent::Heading {
                            level: name.as_bytes()[1] - b'0',
                            inlines: spans,
                        }
                    } else {
                        BlockContent::Paragraph { inlines: spans }
                    },
                    src.clone(),
                )?);
            }
            "ul" | "ol" => {
                let reversed = c.attribute("reversed").is_some();
                let kind = c.attribute("type").unwrap_or("1");
                let custom = name == "ol"
                    && (reversed
                        || kind != "1"
                        || c.attribute("start").is_some_and(|s| s.starts_with('-'))
                        || c.children().any(|n| n.attribute("value").is_some()));
                let count = c.children().filter(|n| named(*n, "li")).count() as i64;
                let mut number = c
                    .attribute("start")
                    .and_then(|s| s.parse::<i64>().ok())
                    .unwrap_or(if reversed { count } else { 1 });
                let start = number;
                let mut items = vec![];
                for li in c.children().filter(|n| named(*n, "li")) {
                    if css.get(li).hidden == Some(true) {
                        continue;
                    }
                    if let Some(n) = li.attribute("value") {
                        number = n.parse().map_err(bad)?;
                    }
                    let mut item = vec![];
                    // Keep phrasing content together, then nested block/list content.
                    let has_blocks = li.children().any(|n| {
                        n.is_element()
                            && matches!(
                                n.tag_name().name(),
                                "p" | "ul" | "ol" | "div" | "blockquote"
                            )
                    });
                    if has_blocks {
                        item = blocks(li, src, ctx, depth + 1, css)?
                    } else {
                        let inlines = inline(li, ctx, src, css)?;
                        item.push(ctx.block(BlockContent::Paragraph { inlines }, src.clone())?);
                    }
                    if custom {
                        let marker = format!("{}. ", super::style::label(number, kind));
                        ctx.text(&marker)?;
                        if let Some(Block {
                            content: BlockContent::Paragraph { inlines },
                            ..
                        }) = item.first_mut()
                        {
                            inlines.insert(0, Inline::text(marker));
                        } else {
                            item.insert(0, para(marker, src, ctx)?);
                        }
                        out.extend(item);
                    } else {
                        items.push(item)
                    }
                    number = if reversed {
                        number.saturating_sub(1)
                    } else {
                        number.saturating_add(1)
                    };
                }
                if !items.is_empty() {
                    out.push(ctx.block(
                        BlockContent::List {
                            start: if name == "ol" {
                                Some(start.max(0) as u64)
                            } else {
                                None
                            },
                            items,
                        },
                        src.clone(),
                    )?);
                }
            }
            "blockquote" => {
                let b = blocks(c, src, ctx, depth + 1, css)?;
                out.push(ctx.block(BlockContent::Quote { blocks: b }, src.clone())?);
            }
            "pre" => {
                let text = content(c);
                ctx.text(&text)?;
                out.push(ctx.block(
                    BlockContent::Code {
                        language: String::new(),
                        text,
                    },
                    src.clone(),
                )?);
            }
            "table" => {
                let mut grid = BTreeMap::new();
                let mut merges = vec![];
                let mut occupied = HashSet::new();
                for (r, row) in c
                    .descendants()
                    .filter(|n| {
                        named(*n, "tr")
                            && n.ancestors()
                                .find(|n| named(*n, "table"))
                                .is_some_and(|n| n == c)
                    })
                    .enumerate()
                {
                    let r = r as u32 + 1;
                    let mut col = 1u32;
                    for cell in row
                        .children()
                        .filter(|n| named(*n, "td") || named(*n, "th"))
                    {
                        while occupied.contains(&(r, col)) {
                            col += 1
                        }
                        let rs = count(cell.attribute("rowspan"), 1)?;
                        let cs = count(cell.attribute("colspan"), 1)?;
                        if rs == 0
                            || cs == 0
                            || rs.saturating_mul(cs) > ctx.options.limits.max_table_cells
                        {
                            return Err(Error::Limit("max_table_cells"));
                        }
                        let text = visible_text(cell, css);
                        grid.insert((r, col), text);
                        for rr in r..r + rs as u32 {
                            for cc in col..col + cs as u32 {
                                if !occupied.insert((rr, cc)) {
                                    return Err(bad("overlapping HTML table cells"));
                                }
                            }
                        }
                        if rs > 1 || cs > 1 {
                            merges.push((r, col, r + rs as u32 - 1, col + cs as u32 - 1));
                        }
                        col += cs as u32;
                    }
                }
                if let Some(t) = table(grid, merges, src, ctx)? {
                    out.push(t)
                }
            }
            "hr" => out.push(ctx.block(BlockContent::ThematicBreak, src.clone())?),
            "img" => {
                let alt = c.attribute("alt").unwrap_or("");
                let image = c
                    .attribute("src")
                    .and_then(|href| resolve(src.part.as_deref().unwrap_or(""), href).ok())
                    .and_then(|path| ctx.image_inline(&path, alt));
                if let Some(image) = image {
                    ctx.text(alt)?;
                    out.push(ctx.block(
                        BlockContent::Paragraph {
                            inlines: vec![image],
                        },
                        src.clone(),
                    )?);
                } else if !alt.is_empty() {
                    out.push(para(alt.into(), src, ctx)?);
                }
                warn(
                    ctx,
                    "epub_image",
                    "Image placement is not reconstructed; alternative text retained. Supported package image files are retained separately when enabled.",
                    src,
                );
            }
            "div" | "section" | "article" | "body" | "nav" | "main" | "aside" | "figure" | "li" => {
                out.extend(blocks(c, src, ctx, depth + 1, css)?)
            }
            _ => {
                let spans = inline(c, ctx, src, css)?;
                if !spans.is_empty() {
                    out.push(ctx.block(BlockContent::Paragraph { inlines: spans }, src.clone())?);
                }
            }
        }
    }
    Ok(out)
}
pub fn parse(bytes: &[u8], ctx: &mut Context<'_>) -> Result<Vec<Block>, Error> {
    let mut pkg = Package::new(bytes, ctx)?;
    if pkg.has("META-INF/encryption.xml") {
        return Err(Error::Unsupported(
            "EPUB encrypted/obfuscated resources require explicit handling".into(),
        ));
    }
    let container = pkg.required("META-INF/container.xml")?;
    let d = xml(&container, ctx)?;
    let path = d
        .descendants()
        .find(|n| named(*n, "rootfile"))
        .and_then(|n| n.attribute("full-path"))
        .ok_or_else(|| bad("EPUB has no package root"))?;
    let path = resolve("", path)?;
    let s = pkg.required(&path)?;
    let opf = xml(&s, ctx)?;
    let manifest: HashMap<_, _> = opf
        .descendants()
        .filter(|n| named(*n, "item"))
        .filter_map(|n| {
            Some((
                n.attribute("id")?,
                (
                    n.attribute("href")?,
                    n.attribute("media-type").unwrap_or(""),
                    n.attribute("properties").unwrap_or(""),
                ),
            ))
        })
        .collect();
    let mut out = vec![];
    if let Some(title) = opf
        .descendants()
        .find(|n| named(*n, "metadata"))
        .and_then(|n| n.children().find(|n| named(*n, "title")))
    {
        let title = content(title);
        if !title.trim().is_empty() {
            out.push(heading(
                title,
                1,
                &Source {
                    part: Some(path.clone().into()),
                    ..Default::default()
                },
                ctx,
            )?);
        }
    }
    let mut chapters = HashSet::new();
    let mut sequence: Vec<_> = manifest
        .values()
        .filter(|(_, _, properties)| properties.split_whitespace().any(|p| p == "nav"))
        .copied()
        .collect();
    sequence.sort_by_key(|(target, _, _)| *target);
    for item in opf.descendants().filter(|n| named(*n, "itemref")) {
        let id = item
            .attribute("idref")
            .ok_or_else(|| bad("spine entry missing id"))?;
        sequence.push(
            *manifest
                .get(id)
                .ok_or_else(|| bad("missing spine manifest entry"))?,
        );
    }
    for (i, (target, kind, _)) in sequence.into_iter().enumerate() {
        if i >= ctx.options.limits.max_pages {
            return Err(Error::Limit("max_pages"));
        }
        let chapter = resolve(&path, target.split('#').next().unwrap_or(target))?;
        let src = Source {
            part: Some(chapter.clone().into()),
            ..Default::default()
        };
        if !matches!(kind, "application/xhtml+xml" | "text/html") {
            warn(
                ctx,
                "epub_spine_type",
                "Non-XHTML spine item omitted.",
                &src,
            );
            continue;
        }
        if !chapters.insert(chapter.clone()) {
            continue;
        }
        let chapter_anchor = anchor(&chapter, "");
        ctx.text(&chapter_anchor)?;
        out.push(ctx.block(
            BlockContent::Paragraph {
                inlines: vec![Inline::Anchor { id: chapter_anchor }],
            },
            src.clone(),
        )?);
        let s = pkg.required(&chapter)?;
        let s = strip_doctype(&s)?;
        let doc = xml(&s, ctx)?;
        let body = doc
            .descendants()
            .find(|n| named(*n, "body"))
            .ok_or_else(|| bad("EPUB chapter has no body"))?;
        let mut css = super::css::Css::default();
        for n in doc.descendants().filter(Node::is_element) {
            if named(n, "style") {
                css.add(&content(n), ctx)?;
            }
            if named(n, "link")
                && n.attribute("rel")
                    .is_some_and(|v| v.split_whitespace().any(|s| s == "stylesheet"))
            {
                if let Some(href) = n.attribute("href") {
                    if href.contains(':') || href.starts_with("//") {
                        css.incomplete = true;
                        continue;
                    }
                    let cp = resolve(&chapter, href)?;
                    if let Some(s) = pkg.read(&cp)? {
                        css.add(&s, ctx)?;
                    } else {
                        css.incomplete = true;
                    }
                }
            }
        }
        css.compute(&doc, ctx)?;
        out.extend(blocks(body, &src, ctx, 0, &css)?);
        if css.incomplete {
            warn(ctx,"epub_css","Supported semantic CSS applied; some selectors, properties or resources remain unsupported.",&src);
        }
    }
    let mut duplicate = false;
    let mut ids = HashSet::new();
    visit(&mut out, &mut |i, _| {
        if let Inline::Anchor { id } = i {
            if !ids.insert(id.clone()) {
                duplicate = true;
                *i = Inline::text("");
            }
        }
    });
    if duplicate {
        warn(
            ctx,
            "epub_duplicate_anchor",
            "Duplicate source IDs share their first target; later anchors omitted.",
            &Default::default(),
        );
    }
    let mut budget = Ok(());
    let mut gaps = vec![];
    visit(&mut out, &mut |i, src| {
        if let Inline::Link { url, .. } = i {
            if url.contains(':') || url.starts_with("//") {
                return;
            }
            let (part, fragment) = url.split_once('#').unwrap_or((url.as_str(), ""));
            let id = anchor(part, fragment);
            if ids.contains(&id) {
                *url = format!("#{id}");
                if budget.is_ok() {
                    budget = ctx.text(url);
                }
            } else if chapters.contains(part) {
                gaps.push(src.clone());
            }
        }
    });
    budget?;
    for src in gaps {
        warn(
            ctx,
            "epub_link_target",
            "Internal link target is missing or hidden; original package link retained.",
            &src,
        );
    }
    Ok(out)
}

// Permit a declaration, never an internal entity subset. No external DTD is
// fetched; roxmltree continues to reject undeclared entities.
fn strip_doctype(s: &str) -> Result<std::borrow::Cow<'_, str>, Error> {
    if let Some(start) = s.find("<!DOCTYPE") {
        let mut quote = None;
        let mut end = None;
        for (i, c) in s[start + 9..].char_indices() {
            if let Some(q) = quote {
                if c == q {
                    quote = None
                }
            } else {
                match c {
                    '\"' | '\'' => quote = Some(c),
                    '[' => return Err(bad("EPUB internal DTD subsets are not supported")),
                    '>' => {
                        end = Some(start + 9 + i + 1);
                        break;
                    }
                    _ => {}
                }
            }
        }
        let end = end.ok_or_else(|| bad("unterminated DOCTYPE"))?;
        return Ok(format!("{}{}", &s[..start], &s[end..]).into());
    }
    Ok(std::borrow::Cow::Borrowed(s))
}

fn visible_text(n: Node<'_, '_>, css: &super::css::Css) -> String {
    let mut s = String::new();
    for c in n.descendants() {
        if c.is_text() && !c.ancestors().any(|p| css.get(p).hidden == Some(true)) {
            s.push_str(c.text().unwrap_or(""));
        } else if named(c, "br") {
            s.push('\n')
        }
    }
    s
}

#[cfg(test)]
mod doctype_tests {
    use super::*;
    use std::borrow::Cow;

    #[test]
    fn chapter_without_doctype_borrows_its_text() {
        let text = "<html><body>é &amp; 🦀</body></html>";
        assert!(matches!(strip_doctype(text).unwrap(), Cow::Borrowed(s) if s == text));
        assert_eq!(
            strip_doctype("<!DOCTYPE html SYSTEM 'a>b'><html/>").unwrap(),
            "<html/>"
        );
        assert!(strip_doctype("<!DOCTYPE html [<!ENTITY x 'bad'>]><html/>").is_err());
        assert!(strip_doctype("<!DOCTYPE html").is_err());
    }
}
