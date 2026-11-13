//! Bounded semantic CSS subset: selectors, cascade, emphasis and display:none.
use super::style::TextStyle;
use super::*;
#[derive(Default)]
pub(super) struct Css {
    rules: Vec<Rule>,
    pub incomplete: bool,
    uncertain_visibility: bool,
    // XML node IDs are dense; direct lookup avoids hashing on every inherited span.
    computed: Vec<TextStyle>,
}
struct Rule {
    selector: String,
    rank: (bool, usize, usize, usize, usize),
    style: TextStyle,
}
fn declarations(s: &str) -> (TextStyle, TextStyle, bool) {
    let mut normal = TextStyle::default();
    let mut important = TextStyle::default();
    let mut incomplete = false;
    for declaration in s.split(';').filter(|s| !s.trim().is_empty()) {
        let Some((key, val)) = declaration.split_once(':') else {
            incomplete = true;
            continue;
        };
        let val = val.trim().to_ascii_lowercase();
        let imp = val.ends_with("!important");
        let v = val.trim_end_matches("!important").trim();
        let style = if imp { &mut important } else { &mut normal };
        match key.trim().to_ascii_lowercase().as_str() {
            "font-weight" => {
                style.bold = match v {
                    "normal" => Some(false),
                    "bold" => Some(true),
                    _ => v.parse::<u32>().ok().map(|n| n >= 600),
                }
            }
            "font-style" => {
                style.italic = match v {
                    "normal" => Some(false),
                    "italic" | "oblique" => Some(true),
                    _ => None,
                }
            }
            "text-decoration" | "text-decoration-line" => {
                style.strike = Some(v.split_whitespace().any(|x| x == "line-through"))
            }
            "display" => style.hidden = Some(v == "none"),
            "white-space" => style.pre = Some(matches!(v, "pre" | "pre-wrap" | "break-spaces")),
            "font-family" if v.contains("monospace") => style.code = Some(true),
            _ => incomplete = true,
        }
    }
    (normal, important, incomplete)
}
fn simple(n: Node<'_, '_>, s: &str) -> bool {
    let mut rest = s;
    let end = rest.find(['.', '#']).unwrap_or(rest.len());
    let tag = &rest[..end];
    if !tag.is_empty() && tag != "*" && n.tag_name().name() != tag {
        return false;
    }
    rest = &rest[end..];
    while !rest.is_empty() {
        let marker = rest.as_bytes()[0];
        rest = &rest[1..];
        let end = rest.find(['.', '#']).unwrap_or(rest.len());
        let value = &rest[..end];
        if value.is_empty() {
            return false;
        }
        if marker == b'#' {
            if n.attribute("id") != Some(value) {
                return false;
            }
        } else if !n
            .attribute("class")
            .is_some_and(|v| v.split_whitespace().any(|c| c == value))
        {
            return false;
        }
        rest = &rest[end..];
    }
    true
}
fn matches(n: Node<'_, '_>, s: &str) -> bool {
    let mut parts = s.split_whitespace().rev();
    let Some(last) = parts.next() else {
        return false;
    };
    if !simple(n, last) {
        return false;
    }
    let mut ancestors = n.ancestors().skip(1);
    for part in parts {
        if !ancestors.any(|a| a.is_element() && simple(a, part)) {
            return false;
        }
    }
    true
}
// Index each selector by one required component of its rightmost element.
// Full matching still handles compound selectors and ancestor relationships.
#[derive(Hash, Eq, PartialEq)]
enum Key<'a> {
    Tag(&'a str),
    Class(&'a str),
    Id(&'a str),
    Any,
}
fn key(selector: &str) -> Key<'_> {
    let last = selector.split_whitespace().last().unwrap_or("");
    if let Some((_, id)) = last.split_once('#') {
        return Key::Id(id.split(['.', '#']).next().unwrap_or(""));
    }
    if let Some((_, class)) = last.split_once('.') {
        return Key::Class(class.split(['.', '#']).next().unwrap_or(""));
    }
    match last {
        "" | "*" => Key::Any,
        tag => Key::Tag(tag),
    }
}
impl Css {
    pub fn add(&mut self, s: &str, ctx: &mut Context<'_>) -> Result<(), Error> {
        // Remove comments before tokenization; at-rules/complex selectors remain gaps.
        let mut clean = String::new();
        let mut rest = s;
        while let Some(i) = rest.find("/*") {
            clean.push_str(&rest[..i]);
            let Some(j) = rest[i + 2..].find("*/") else {
                self.incomplete = true;
                return Ok(());
            };
            rest = &rest[i + 2 + j + 2..];
        }
        clean.push_str(rest);
        let mut pos = 0;
        while let Some(open) = clean[pos..].find('{') {
            let open = pos + open;
            let selector = clean[pos..open].trim();
            let mut level = 1usize;
            let mut end = open + 1;
            while end < clean.len() && level > 0 {
                match clean.as_bytes()[end] {
                    b'{' => level += 1,
                    b'}' => level -= 1,
                    _ => {}
                }
                end += 1
            }
            if level != 0 {
                self.incomplete = true;
                break;
            }
            let body = &clean[open + 1..end - 1];
            pos = end;
            if selector.starts_with('@') || body.contains('{') {
                // A skipped conditional can restore content hidden by a base rule.
                // Preserve text when visibility cannot be decided reliably.
                self.uncertain_visibility |= body.to_ascii_lowercase().contains("display");
                self.incomplete = true;
                continue;
            }
            let (normal, important, incomplete) = declarations(body);
            self.incomplete |= incomplete;
            for selector in selector.split(',').map(str::trim) {
                ctx.node()?;
                if selector.is_empty()
                    || !selector
                        .chars()
                        .all(|c| c.is_alphanumeric() || "_-.*# ".contains(c))
                {
                    self.incomplete = true;
                    self.uncertain_visibility |=
                        normal.hidden.is_some() || important.hidden.is_some();
                    continue;
                }
                let ids = selector.matches('#').count();
                let classes = selector.matches('.').count();
                let tags = selector
                    .split_whitespace()
                    .filter(|s| !s.starts_with(['.', '#', '*']))
                    .count();
                let order = self.rules.len();
                self.rules.push(Rule {
                    selector: selector.into(),
                    rank: (false, ids, classes, tags, order),
                    style: normal,
                });
                self.rules.push(Rule {
                    selector: selector.into(),
                    rank: (true, ids, classes, tags, order),
                    style: important,
                });
            }
        }
        Ok(())
    }
    pub fn compute(&mut self, doc: &Document<'_>, ctx: &mut Context<'_>) -> Result<(), Error> {
        self.rules.sort_by_key(|r| r.rank);
        self.computed = vec![TextStyle::default(); doc.descendants().len()];
        let mut candidates: HashMap<Key<'_>, Vec<usize>> = HashMap::new();
        for (i, rule) in self.rules.iter().enumerate() {
            if rule.style != TextStyle::default() {
                candidates.entry(key(&rule.selector)).or_default().push(i);
            }
        }
        let mut selected = Vec::new();
        let mut work = 0usize;
        for n in doc.descendants().filter(Node::is_element) {
            ctx.check()?;
            let inherited = n.parent_element().map(|p| self.get(p)).unwrap_or_default();
            let mut s = inherited;
            match n.tag_name().name() {
                "b" | "strong" => s.bold = Some(true),
                "em" | "i" => s.italic = Some(true),
                "s" | "del" => s.strike = Some(true),
                "code" => s.code = Some(true),
                "pre" => s.pre = Some(true),
                _ => {}
            }
            let (normal, important, gap) = declarations(n.attribute("style").unwrap_or(""));
            self.incomplete |= gap;
            // Charge the original all-rule work budget even though the index
            // avoids impossible matches. Resource limits keep their semantics.
            work = work
                .checked_add(self.rules.len())
                .ok_or(Error::Limit("max_nodes"))?;
            if work > ctx.options.limits.max_nodes.saturating_mul(64) {
                return Err(Error::Limit("max_nodes"));
            }
            selected.clear();
            for k in [Key::Any, Key::Tag(n.tag_name().name())] {
                if let Some(ids) = candidates.get(&k) {
                    selected.extend_from_slice(ids);
                }
            }
            if let Some(id) = n.attribute("id") {
                if let Some(ids) = candidates.get(&Key::Id(id)) {
                    selected.extend_from_slice(ids);
                }
            }
            for class in n.attribute("class").unwrap_or("").split_whitespace() {
                if let Some(ids) = candidates.get(&Key::Class(class)) {
                    selected.extend_from_slice(ids);
                }
            }
            selected.sort_unstable();
            selected.dedup();
            let mut inline_applied = false;
            for &i in &selected {
                let rule = &self.rules[i];
                if rule.rank.0 && !inline_applied {
                    s = s.merge(normal);
                    inline_applied = true;
                }
                if matches(n, &rule.selector) {
                    s = s.merge(rule.style);
                }
            }
            if !inline_applied {
                s = s.merge(normal);
            }
            s = s.merge(important);
            if self.uncertain_visibility {
                s.hidden = None;
            }
            if inherited.hidden == Some(true) || n.attribute("hidden").is_some() {
                s.hidden = Some(true)
            }
            self.computed[n.id().get_usize()] = s;
        }
        Ok(())
    }
    pub fn get(&self, n: Node<'_, '_>) -> TextStyle {
        self.computed
            .get(n.id().get_usize())
            .copied()
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexed_cascade_matches_full_scan_with_compounds_and_inheritance() {
        let options = crate::Options::default();
        let token = crate::CancellationToken::default();
        let mut ctx = Context::new(&options, &token);
        let doc = Document::parse(r#"<html><body><section class="chapter"><div id="box" class="note"><p id="target" class="note note strong" style="font-weight: normal; font-style: normal !important">Text <strong>nested</strong></p><p hidden="">hidden <em>child</em></p></div><p class="note">outside</p></section></body></html>"#).unwrap();
        let mut css = Css::default();
        let selectors = [
            "*",
            "p",
            "section",
            ".note",
            "#target",
            "p.note",
            "p#target.note",
            ".note.strong",
            "div p",
            "section .note strong",
            "#box .note",
            "p#absent",
            "div.note",
            "html body p",
            "#target#other",
            ".missing.note",
        ];
        for (i, selector) in selectors.iter().enumerate() {
            let property = [
                "font-weight: bold",
                "font-style: italic !important",
                "display: none",
                "font-weight: normal !important",
                "white-space: pre",
                "text-decoration: line-through",
            ][i % 6];
            css.add(&format!("{selector} {{{property}}}"), &mut ctx)
                .unwrap();
        }
        css.compute(&doc, &mut ctx).unwrap();
        let mut reference: HashMap<roxmltree::NodeId, TextStyle> = HashMap::new();
        for n in doc.descendants().filter(Node::is_element) {
            let inherited = n
                .parent_element()
                .and_then(|p| reference.get(&p.id()))
                .copied()
                .unwrap_or_default();
            let mut s = inherited;
            match n.tag_name().name() {
                "b" | "strong" => s.bold = Some(true),
                "em" | "i" => s.italic = Some(true),
                "s" | "del" => s.strike = Some(true),
                "code" => s.code = Some(true),
                "pre" => s.pre = Some(true),
                _ => {}
            }
            let (normal, important, _) = declarations(n.attribute("style").unwrap_or(""));
            for rule in css.rules.iter().filter(|r| !r.rank.0) {
                if matches(n, &rule.selector) {
                    s = s.merge(rule.style);
                }
            }
            s = s.merge(normal);
            for rule in css.rules.iter().filter(|r| r.rank.0) {
                if matches(n, &rule.selector) {
                    s = s.merge(rule.style);
                }
            }
            s = s.merge(important);
            if inherited.hidden == Some(true) || n.attribute("hidden").is_some() {
                s.hidden = Some(true);
            }
            assert_eq!(
                css.get(n),
                s,
                "element {} {:?}",
                n.tag_name().name(),
                n.attribute("id")
            );
            reference.insert(n.id(), s);
        }
        assert_eq!(
            css.get(
                doc.descendants()
                    .find(|n| n.attribute("id") == Some("target"))
                    .unwrap()
            )
            .italic,
            Some(false)
        );
    }

    #[test]
    fn indexed_cascade_keeps_work_budget_for_unmatched_and_empty_rules() {
        let options = crate::Options::default();
        let token = crate::CancellationToken::default();
        let mut ctx = Context::new(&options, &token);
        let mut css = Css::default();
        for i in 0..20 {
            css.add(&format!(".absent{i} {{ color: red }}"), &mut ctx)
                .unwrap();
        }
        let mut limited = options.clone();
        limited.limits.max_nodes = 1;
        let mut ctx = Context::new(&limited, &token);
        let doc = Document::parse("<body><p>visible</p></body>").unwrap();
        assert!(matches!(
            css.compute(&doc, &mut ctx),
            Err(Error::Limit("max_nodes"))
        ));
    }
}
