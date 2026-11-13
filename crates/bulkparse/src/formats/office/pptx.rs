use super::*;

#[derive(Clone, Copy, Default)]
struct Style {
    bold: Option<bool>,
    italic: Option<bool>,
    strike: Option<bool>,
}
impl Style {
    fn read(n: Option<Node<'_, '_>>) -> Self {
        let v = |key| {
            n.and_then(|n| n.attribute(key))
                .map(|s| matches!(s, "1" | "true" | "sngStrike" | "dblStrike"))
        };
        Self {
            bold: v("b"),
            italic: v("i"),
            strike: v("strike"),
        }
    }
    fn merge(self, other: Self) -> Self {
        Self {
            bold: other.bold.or(self.bold),
            italic: other.italic.or(self.italic),
            strike: other.strike.or(self.strike),
        }
    }
    fn apply(self, mut value: Inline) -> Inline {
        if self.bold == Some(true) {
            value = Inline::Strong {
                children: vec![value],
            }
        }
        if self.italic == Some(true) {
            value = Inline::Emphasis {
                children: vec![value],
            }
        }
        if self.strike == Some(true) {
            value = Inline::Strike {
                children: vec![value],
            }
        }
        value
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Bullet {
    Off,
    Unordered,
    Number(u64, &'static str, &'static str, &'static str),
}
#[derive(Clone, Copy, Default)]
struct Rule {
    style: Style,
    bullet: Option<Bullet>,
}
impl Rule {
    fn read(n: Option<Node<'_, '_>>) -> Result<Self, Error> {
        let bullet = if n.and_then(|n| child(n, "buNone")).is_some() {
            Some(Bullet::Off)
        } else if let Some(b) = n.and_then(|n| child(n, "buAutoNum")) {
            let kind = b.attribute("type").unwrap_or("arabicPeriod");
            let format = if kind.starts_with("romanLc") {
                "i"
            } else if kind.starts_with("romanUc") {
                "I"
            } else if kind.starts_with("alphaLc") {
                "a"
            } else if kind.starts_with("alphaUc") {
                "A"
            } else {
                "1"
            };
            let prefix = if kind.ends_with("ParenBoth") { "(" } else { "" };
            let suffix = if kind.ends_with("ParenBoth") || kind.ends_with("ParenR") {
                ")"
            } else if kind.ends_with("Plain") {
                ""
            } else {
                "."
            };
            Some(Bullet::Number(
                count(b.attribute("startAt"), 1)? as u64,
                format,
                prefix,
                suffix,
            ))
        } else if n.and_then(|n| child(n, "buChar")).is_some() {
            Some(Bullet::Unordered)
        } else {
            None
        };
        Ok(Self {
            style: Style::read(n.and_then(|n| child(n, "defRPr"))),
            bullet,
        })
    }
    fn merge(self, r: Self) -> Self {
        Self {
            style: self.style.merge(r.style),
            bullet: r.bullet.or(self.bullet),
        }
    }
}
fn rules(n: Option<Node<'_, '_>>) -> Result<[Rule; 9], Error> {
    let mut result = [Rule::default(); 9];
    let base = Rule::read(n.and_then(|n| child(n, "defPPr")))?;
    for (i, r) in result.iter_mut().enumerate() {
        *r = base.merge(Rule::read(
            n.and_then(|n| child(n, &format!("lvl{}pPr", i + 1))),
        )?);
    }
    Ok(result)
}
fn bounds(n: Node<'_, '_>) -> Option<super::layout::Rect> {
    let p = child(n, "spPr")
        .or_else(|| child(n, "grpSpPr"))
        .unwrap_or(n);
    let x = child(p, "xfrm")?;
    if x.attribute("rot").is_some_and(|r| r != "0")
        || ["flipH", "flipV"]
            .iter()
            .any(|k| matches!(x.attribute(*k), Some("1" | "true")))
    {
        return None;
    }
    let off = child(x, "off")?;
    let ext = child(x, "ext")?;
    super::layout::Rect::new(
        off.attribute("x")?.parse().ok()?,
        off.attribute("y")?.parse().ok()?,
        ext.attribute("cx")?.parse().ok()?,
        ext.attribute("cy")?.parse().ok()?,
    )
}
struct Placeholder {
    bounds: Option<super::layout::Rect>,
    idx: String,
    kind: String,
    rules: [Rule; 9],
}
fn placeholders(n: Node<'_, '_>) -> Result<Vec<Placeholder>, Error> {
    let mut out = vec![];
    for shape in n.descendants().filter(|n| named(*n, "sp")) {
        if let Some(ph) = shape.descendants().find(|n| named(*n, "ph")) {
            let body = child(shape, "txBody");
            let mut levels = rules(body.and_then(|n| child(n, "lstStyle")))?;
            for p in body
                .into_iter()
                .flat_map(|n| n.children())
                .filter(|n| named(*n, "p"))
            {
                let prop = child(p, "pPr");
                let level = count(prop.and_then(|n| n.attribute("lvl")), 0)?.min(8);
                levels[level] = levels[level].merge(Rule::read(prop)?);
            }
            out.push(Placeholder {
                bounds: bounds(shape),
                idx: ph.attribute("idx").unwrap_or("0").into(),
                kind: ph.attribute("type").unwrap_or("obj").into(),
                rules: levels,
            });
        }
    }
    Ok(out)
}
struct Cascade {
    base: [Rule; 9],
    layout: Vec<Placeholder>,
    master: Vec<Placeholder>,
    classes: HashMap<String, [Rule; 9]>,
}
impl Cascade {
    fn bounds(&self, n: Node<'_, '_>) -> Option<super::layout::Rect> {
        if let Some(b) = bounds(n) {
            return Some(b);
        }
        if !named(n, "sp") {
            return None;
        }
        // A local transform that is present but unsafe is not replaced by inherited geometry.
        if child(n, "spPr").is_some_and(|p| child(p, "xfrm").is_some())
            || child(n, "xfrm").is_some()
        {
            return None;
        }
        let ph = n.descendants().find(|n| named(*n, "ph"))?;
        let idx = ph.attribute("idx").unwrap_or("0");
        let layout = self.layout.iter().find(|p| p.idx == idx);
        let kind = ph
            .attribute("type")
            .or_else(|| layout.map(|p| p.kind.as_str()))
            .unwrap_or("obj");
        layout.and_then(|p| p.bounds).or_else(|| {
            self.master
                .iter()
                .find(|p| p.kind == kind)
                .and_then(|p| p.bounds)
        })
    }
    fn shape(&self, n: Node<'_, '_>) -> (bool, [Rule; 9]) {
        let ph = n.descendants().find(|n| named(*n, "ph"));
        let idx = ph.and_then(|n| n.attribute("idx")).unwrap_or("0");
        let layout = ph.and_then(|_| self.layout.iter().find(|p| p.idx == idx));
        let kind = ph
            .and_then(|n| n.attribute("type"))
            .or_else(|| layout.map(|p| p.kind.as_str()))
            .unwrap_or("obj");
        let title = matches!(kind, "title" | "ctrTitle");
        let class = if title {
            "titleStyle"
        } else if matches!(kind, "body" | "obj") {
            "bodyStyle"
        } else {
            "otherStyle"
        };
        let master = ph.and_then(|_| {
            self.master.iter().find(|p| p.kind == kind).or_else(|| {
                self.master
                    .iter()
                    .find(|p| title && matches!(p.kind.as_str(), "title" | "ctrTitle"))
            })
        });
        let mut result = self.base;
        for (i, r) in result.iter_mut().enumerate() {
            if let Some(c) = self.classes.get(class) {
                *r = r.merge(c[i]);
            }
            if let Some(p) = master {
                *r = r.merge(p.rules[i]);
            }
            if let Some(p) = layout {
                *r = r.merge(p.rules[i]);
            }
        }
        (title, result)
    }
}
fn cascade(
    pkg: &mut Package<'_>,
    part: &str,
    rels: &HashMap<String, Rel>,
    base: [Rule; 9],
    ctx: &mut Context<'_>,
) -> Result<Cascade, Error> {
    let mut result = Cascade {
        base,
        layout: vec![],
        master: vec![],
        classes: HashMap::new(),
    };
    if let Some(rel) = rels
        .values()
        .find(|r| r.kind.ends_with("/slideLayout") && !r.external)
    {
        let lp = resolve(part, &rel.target)?;
        let s = pkg.required(&lp)?;
        let layout = xml(&s, ctx)?;
        result.layout = placeholders(layout.root_element())?;
        let lr = pkg.rels(&lp, ctx)?;
        if let Some(rel) = lr
            .values()
            .find(|r| r.kind.ends_with("/slideMaster") && !r.external)
        {
            let mp = resolve(&lp, &rel.target)?;
            let s = pkg.required(&mp)?;
            let master = xml(&s, ctx)?;
            result.master = placeholders(master.root_element())?;
            for kind in ["titleStyle", "bodyStyle", "otherStyle"] {
                result.classes.insert(
                    kind.into(),
                    rules(master.descendants().find(|n| named(*n, kind)))?,
                );
            }
        }
    }
    Ok(result)
}
fn inlines(
    p: Node<'_, '_>,
    base: Style,
    rels: &HashMap<String, Rel>,
    src: &Source,
    ctx: &mut Context<'_>,
) -> Result<Vec<Inline>, Error> {
    let mut out = vec![];
    let pp = child(p, "pPr");
    let base = base.merge(Style::read(pp.and_then(|n| child(n, "defRPr"))));
    for n in p.children().filter(Node::is_element) {
        match n.tag_name().name() {
            "r" | "fld" => {
                let text = child(n, "t").map(content).unwrap_or_default();
                ctx.text(&text)?;
                let prop = child(n, "rPr");
                let style = base.merge(Style::read(prop));
                let mut i = style.apply(Inline::text(text));
                if let Some(link) = prop
                    .and_then(|p| child(p, "hlinkClick"))
                    .and_then(|n| attr(n, "id"))
                    .and_then(|id| rels.get(id))
                {
                    if link.external {
                        i = Inline::Link {
                            url: link.target.clone(),
                            children: vec![i],
                        }
                    } else {
                        warn(
                            ctx,
                            "pptx_internal_link",
                            "Internal slide link text retained; target not represented.",
                            src,
                        );
                    }
                }
                out.push(i)
            }
            "br" => out.push(Inline::Break),
            "pPr" | "endParaRPr" => {}
            "oMath" | "oMathPara" => out.push(super::math::inline(n, ctx, src)?),
            "m" | "AlternateContent" => {
                if let Some(math) = n
                    .descendants()
                    .find(|n| named(*n, "oMathPara") || named(*n, "oMath"))
                {
                    out.push(super::math::inline(math, ctx, src)?);
                } else {
                    warn(
                        ctx,
                        "pptx_inline",
                        "Unsupported alternate presentation content.",
                        src,
                    );
                }
            }
            _ => warn(
                ctx,
                "pptx_inline",
                "Unsupported presentation inline content.",
                src,
            ),
        }
    }
    if out.iter().any(|i| {
        !matches!(i, Inline::Math { .. } | Inline::Anchor { .. })
            && !super::style::plain(std::slice::from_ref(i))
                .trim()
                .is_empty()
    }) {
        for i in &mut out {
            if let Inline::Math { display, .. } = i {
                *display = false;
            }
        }
    }
    Ok(out)
}
fn list_groups(
    entries: &[(usize, Option<Bullet>, Block)],
    ctx: &mut Context<'_>,
    src: &Source,
    depth: usize,
) -> Result<Vec<Block>, Error> {
    ctx.depth(depth)?;
    let mut out = vec![];
    let mut pos = 0;
    while pos < entries.len() {
        let base = entries[pos].0;
        let bullet = entries[pos].1;
        let start = match bullet {
            Some(Bullet::Number(n, _, _, _)) => Some(n),
            _ => None,
        };
        let mut items = vec![];
        while pos < entries.len() && entries[pos].0 >= base {
            if entries[pos].0 == base {
                if entries[pos].1 != bullet && !items.is_empty() {
                    break;
                }
                items.push(vec![entries[pos].2.clone()]);
                pos += 1;
            } else {
                let first = pos;
                while pos < entries.len() && entries[pos].0 > base {
                    pos += 1
                }
                let nested = list_groups(&entries[first..pos], ctx, src, depth + 1)?;
                if let Some(item) = items.last_mut() {
                    item.extend(nested)
                }
            }
        }
        if let Some(Bullet::Number(n, kind, prefix, suffix)) = bullet {
            if kind != "1" || !prefix.is_empty() || suffix != "." {
                for (i, mut item) in items.into_iter().enumerate() {
                    if let Some(Block {
                        content: BlockContent::Paragraph { inlines },
                        ..
                    }) = item.first_mut()
                    {
                        inlines.insert(
                            0,
                            Inline::text(format!(
                                "{prefix}{}{suffix} ",
                                super::style::label(
                                    n.saturating_add(i as u64).min(i64::MAX as u64) as i64,
                                    kind
                                )
                            )),
                        );
                    }
                    out.extend(item);
                }
                continue;
            }
        }
        out.push(ctx.block(BlockContent::List { start, items }, src.clone())?);
    }
    Ok(out)
}
fn text_body(
    body: Node<'_, '_>,
    title: bool,
    inherited: [Rule; 9],
    rels: &HashMap<String, Rel>,
    src: &Source,
    ctx: &mut Context<'_>,
) -> Result<Vec<Block>, Error> {
    let mut out = vec![];
    let mut pending = vec![];
    for p in body.children().filter(|n| named(*n, "p")) {
        let props = child(p, "pPr");
        let level = count(props.and_then(|p| p.attribute("lvl")), 0)?.min(8);
        let key = format!("lvl{}pPr", level + 1);
        let rule = inherited[level]
            .merge(Rule::read(
                child(body, "lstStyle").and_then(|s| child(s, &key)),
            )?)
            .merge(Rule::read(props)?);
        let inlines = inlines(p, rule.style, rels, src, ctx)?;
        if inlines.is_empty() {
            continue;
        }
        let bullet = matches!(rule.bullet, Some(Bullet::Unordered | Bullet::Number(..)));
        if !title && bullet {
            let start = rule.bullet;
            let block = ctx.block(BlockContent::Paragraph { inlines }, src.clone())?;
            pending.push((level, start, block));
        } else {
            out.extend(list_groups(&pending, ctx, src, 0)?);
            pending.clear();
            out.push(ctx.block(
                if title {
                    BlockContent::Heading { level: 2, inlines }
                } else {
                    BlockContent::Paragraph { inlines }
                },
                src.clone(),
            )?);
        }
    }
    out.extend(list_groups(&pending, ctx, src, 0)?);
    Ok(out)
}
fn shapes(
    root: Node<'_, '_>,
    defaults: &Cascade,
    pkg: &mut Package<'_>,
    rels: &HashMap<String, Rel>,
    src: &Source,
    ctx: &mut Context<'_>,
    depth: usize,
) -> Result<Vec<Block>, Error> {
    ctx.depth(depth)?;
    let mut out = vec![];
    let nodes: Vec<_> = root.children().filter(Node::is_element).collect();
    let positions: Vec<_> = nodes.iter().map(|n| defaults.bounds(*n)).collect();
    let permutation = if named(root, "grpSp") && bounds(root).is_none() {
        (0..nodes.len()).collect()
    } else {
        super::layout::order(&positions)
    };
    for index in permutation {
        let n = nodes[index];
        ctx.check()?;
        match n.tag_name().name() {
            "sp" => {
                let (title, inherited) = defaults.shape(n);
                if let Some(body) = child(n, "txBody") {
                    out.extend(text_body(body, title, inherited, rels, src, ctx)?);
                } else if let Some(alt) = n
                    .descendants()
                    .find(|n| named(*n, "cNvPr"))
                    .and_then(|n| n.attribute("descr"))
                {
                    out.push(para(alt.into(), src, ctx)?);
                }
            }
            "grpSp" => out.extend(shapes(n, defaults, pkg, rels, src, ctx, depth + 1)?),
            "graphicFrame" => {
                if let Some(t) = n.descendants().find(|n| named(*n, "tbl")) {
                    let mut grid = BTreeMap::new();
                    let mut merges = vec![];
                    let mut occupied = HashSet::new();
                    let width = child(t, "tblGrid")
                        .map(|g| g.children().filter(|n| named(*n, "gridCol")).count())
                        .unwrap_or(0);
                    if width > ctx.options.limits.max_table_cells {
                        return Err(Error::Limit("max_table_cells"));
                    }
                    for (row_index, row) in t.children().filter(|n| named(*n, "tr")).enumerate() {
                        let cells: Vec<_> = row.children().filter(|n| named(*n, "tc")).collect();
                        let fixed = width != 0 && cells.len() == width;
                        let mut col = 1u32;
                        for cell in cells {
                            let continuation =
                                matches!(cell.attribute("hMerge"), Some("1" | "true"))
                                    || matches!(cell.attribute("vMerge"), Some("1" | "true"));
                            let r = row_index as u32 + 1;
                            if !fixed && !continuation {
                                while occupied.contains(&(r, col)) {
                                    col = col
                                        .checked_add(1)
                                        .ok_or_else(|| bad("table column overflow"))?;
                                }
                            }
                            let rs = count(cell.attribute("rowSpan"), 1)?;
                            let cs = count(cell.attribute("gridSpan"), 1)?;
                            if rs == 0
                                || cs == 0
                                || rs.saturating_mul(cs) > ctx.options.limits.max_table_cells
                            {
                                return Err(Error::Limit("max_table_cells"));
                            }
                            let source = Source {
                                row: Some(r),
                                cell: Some(col),
                                ..src.clone()
                            };
                            let contents = if let Some(body) = child(cell, "txBody") {
                                text_body(body, false, [Rule::default(); 9], rels, &source, ctx)?
                            } else {
                                vec![]
                            };
                            if !contents.is_empty() || !continuation {
                                grid.insert((r, col), contents);
                            }
                            if !continuation && (rs > 1 || cs > 1) {
                                let endr = r
                                    .checked_add(rs as u32 - 1)
                                    .ok_or_else(|| bad("row span overflow"))?;
                                let endc = col
                                    .checked_add(cs as u32 - 1)
                                    .ok_or_else(|| bad("column span overflow"))?;
                                if occupied.len().saturating_add(rs.saturating_mul(cs))
                                    > ctx.options.limits.max_table_cells
                                {
                                    return Err(Error::Limit("max_table_cells"));
                                }
                                merges.push((r, col, endr, endc));
                                for rr in r..=endr {
                                    for cc in col..=endc {
                                        if !occupied.insert((rr, cc)) {
                                            return Err(bad("overlapping table merges"));
                                        }
                                    }
                                }
                            }
                            col = col
                                .checked_add(if fixed || continuation { 1 } else { cs as u32 })
                                .ok_or_else(|| bad("column overflow"))?;
                        }
                    }
                    if let Some(mut table) = structured_table(grid, merges, src, ctx)? {
                        if let BlockContent::Table { header_rows, .. } = &mut table.content {
                            *header_rows = usize::from(child(t, "tblPr").is_some_and(|p| {
                                matches!(p.attribute("firstRow"), Some("1" | "true"))
                            }));
                        }
                        out.push(table);
                    }
                } else if let Some(chart) = n.descendants().find(|n| named(*n, "chart")) {
                    let id = attr(chart, "id").ok_or_else(|| bad("chart without relationship"))?;
                    let path = linked(rels, id, src.part.as_deref().unwrap_or(""))?;
                    out.extend(super::chart::read(pkg, &path, src, ctx)?);
                } else {
                    warn(
                        ctx,
                        "pptx_graphic",
                        "Chart, SmartArt or embedded object is not reconstructed.",
                        src,
                    );
                    for text in n.descendants().filter(|n| named(*n, "t")) {
                        out.push(para(content(text), src, ctx)?);
                    }
                }
            }
            "pic" => {
                if let Some(id) = n
                    .descendants()
                    .find(|n| named(*n, "blip"))
                    .and_then(|n| attr(n, "embed"))
                {
                    if let Ok(part) = linked(rels, id, src.part.as_deref().unwrap_or("")) {
                        if let Some(image) = ctx.image_inline(&part, "") {
                            out.push(ctx.block(
                                BlockContent::Paragraph {
                                    inlines: vec![image],
                                },
                                src.clone(),
                            )?);
                        }
                    }
                }
                if let Some(alt) = n
                    .descendants()
                    .find(|n| named(*n, "cNvPr"))
                    .and_then(|n| n.attribute("descr"))
                {
                    if !alt.is_empty() {
                        out.push(para(alt.into(), src, ctx)?);
                    }
                }
                warn(
                    ctx,
                    "pptx_image",
                    "Image placement and visual interpretation are not reconstructed; available description retained. Supported original image files are retained separately when enabled.",
                    src,
                );
            }
            "cxnSp" => {}
            "nvGrpSpPr" | "grpSpPr" | "extLst" => {}
            "AlternateContent" => {
                let branch = n
                    .children()
                    .find(|n| named(*n, "Choice") && n.descendants().any(|n| named(n, "oMath")))
                    .or_else(|| n.children().find(|n| named(*n, "Fallback")))
                    .or_else(|| n.children().find(|n| named(*n, "Choice")));
                if let Some(branch) = branch {
                    out.extend(shapes(branch, defaults, pkg, rels, src, ctx, depth + 1)?);
                }
                warn(
                    ctx,
                    "pptx_alternate",
                    "One compatibility branch selected.",
                    src,
                );
            }
            _ => warn(ctx, "pptx_shape", "Unsupported slide object omitted.", src),
        }
    }
    Ok(out)
}
pub fn parse(bytes: &[u8], ctx: &mut Context<'_>) -> Result<Vec<Block>, Error> {
    let mut pkg = Package::new(bytes, ctx)?;
    let part = pkg.main("ppt/presentation.xml", ctx)?;
    let pres = pkg.required(&part)?;
    let doc = xml(&pres, ctx)?;
    if !named(doc.root_element(), "presentation") {
        return Err(bad("main part is not a presentation"));
    }
    let rels = pkg.rels(&part, ctx)?;
    let mut out = vec![];
    let ids: Vec<_> = doc.descendants().filter(|n| named(*n, "sldId")).collect();
    if ids.len() > ctx.options.limits.max_pages {
        return Err(Error::Limit("max_pages"));
    }
    let defaults = rules(doc.descendants().find(|n| named(*n, "defaultTextStyle")))?;
    for (i, id) in ids.iter().enumerate() {
        let path = linked(
            &rels,
            attr(*id, "id").ok_or_else(|| bad("slide without relationship"))?,
            &part,
        )?;
        let src = Source {
            part: Some(path.clone().into()),
            page: Some(i as u32 + 1),
            ..Default::default()
        };
        let s = pkg.required(&path)?;
        let slide = xml(&s, ctx)?;
        let sr = pkg.rels(&path, ctx)?;
        if !named(slide.root_element(), "sld") {
            return Err(bad("invalid slide root"));
        }
        if matches!(slide.root_element().attribute("show"), Some("0" | "false")) {
            ctx.diagnostic(
                "office_hidden_content",
                "Hidden slide omitted by visible-content policy.",
                src,
                false,
            );
            continue;
        }
        out.push(heading(format!("Slide {}", i + 1), 1, &src, ctx)?);
        let tree = slide
            .descendants()
            .find(|n| named(*n, "spTree"))
            .ok_or_else(|| bad("slide without shape tree"))?;
        let inherited = cascade(&mut pkg, &path, &sr, defaults, ctx)?;
        out.extend(shapes(tree, &inherited, &mut pkg, &sr, &src, ctx, 0)?);
        if sr.values().any(|r| r.kind.ends_with("/slideLayout")) {
            warn(ctx,"pptx_layout_inheritance","Text styles and title placeholders inherit through layout/master; background graphics and exact visual positioning are not reconstructed.",&src);
        }
        for rel in sr
            .values()
            .filter(|r| r.kind.ends_with("/notesSlide") && !r.external)
        {
            let np = resolve(&path, &rel.target)?;
            let ns = pkg.required(&np)?;
            let nd = xml(&ns, ctx)?;
            let nr = pkg.rels(&np, ctx)?;
            let nsrc = Source {
                part: Some(np.into()),
                ..src.clone()
            };
            let mut notes = vec![];
            for shape in nd.descendants().filter(|n| named(*n, "sp")) {
                let kind = shape
                    .descendants()
                    .find(|n| named(*n, "ph"))
                    .and_then(|n| n.attribute("type"));
                if matches!(kind, Some("sldImg" | "sldNum" | "hdr" | "ftr" | "dt")) {
                    continue;
                }
                if let Some(body) = child(shape, "txBody") {
                    notes.extend(text_body(
                        body,
                        false,
                        [Rule::default(); 9],
                        &nr,
                        &nsrc,
                        ctx,
                    )?);
                }
            }
            if !notes.is_empty() {
                out.push(ctx.block(BlockContent::Quote { blocks: notes }, nsrc)?)
            }
        }
    }
    Ok(out)
}
