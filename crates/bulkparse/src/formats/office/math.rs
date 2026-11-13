//! Bounded MathML/OMML trees to a conservative, generated TeX subset.
use super::*;
fn literal(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        out.push_str(&match c {
            '\\' => r"\backslash{}".into(),
            '{' => r"\{".into(),
            '}' => r"\}".into(),
            '$' => r"\$".into(),
            '%' => r"\%".into(),
            '#' => r"\#".into(),
            '&' => r"\&".into(),
            '_' => r"\_".into(),
            '^' => r"\hat{}".into(),
            '~' => r"\sim ".into(),
            '<' => r"\lt ".into(),
            '>' => r"\gt ".into(),
            '−' => "-".into(),
            '±' => r"\pm ".into(),
            '×' => r"\times ".into(),
            '÷' => r"\div ".into(),
            '≤' => r"\le ".into(),
            '≥' => r"\ge ".into(),
            '∞' => r"\infty ".into(),
            '∑' => r"\sum ".into(),
            '∫' => r"\int ".into(),
            _ => c.to_string(),
        });
    }
    out
}
fn walk(
    n: Node<'_, '_>,
    ctx: &mut Context<'_>,
    src: &Source,
    depth: usize,
) -> Result<String, Error> {
    ctx.depth(depth)?;
    ctx.check()?;
    if n.is_text() {
        return Ok(literal(n.text().unwrap_or("")));
    }
    let name = n.tag_name().name();
    if name.ends_with("Pr") || matches!(name, "annotation" | "annotation-xml") {
        return Ok(String::new());
    }
    let children: Vec<_> = n.children().filter(Node::is_element).collect();
    let mut vals = vec![];
    for c in &children {
        vals.push(walk(*c, ctx, src, depth + 1)?);
    }
    let at = |i: usize| vals.get(i).cloned().unwrap_or_default();
    let named_value = |key: &str| {
        children
            .iter()
            .position(|c| named(*c, key))
            .map(at)
            .unwrap_or_default()
    };
    let joined = || vals.join("");
    let output = match name {
        "mi" | "mn" | "mo" | "t" => literal(&content(n)),
        "mtext" => format!("\\text{{{}}}", literal(&content(n))),
        "mfrac" => format!("\\frac{{{}}}{{{}}}", at(0), at(1)),
        "msup" => format!("{}^{{{}}}", at(0), at(1)),
        "msub" => format!("{}_{{{}}}", at(0), at(1)),
        "msubsup" => format!("{}_{{{}}}^{{{}}}", at(0), at(1), at(2)),
        "msqrt" => format!("\\sqrt{{{}}}", joined()),
        "mroot" => format!("\\sqrt[{}]{{{}}}", at(1), at(0)),
        "mfenced" => format!(
            "{}{}{}",
            literal(n.attribute("open").unwrap_or("(")),
            vals.join(&literal(n.attribute("separators").unwrap_or(","))),
            literal(n.attribute("close").unwrap_or(")"))
        ),
        "f" => format!("\\frac{{{}}}{{{}}}", named_value("num"), named_value("den")),
        "sSup" => format!("{}^{{{}}}", named_value("e"), named_value("sup")),
        "sSub" => format!("{}_{{{}}}", named_value("e"), named_value("sub")),
        "sSubSup" => format!(
            "{}_{{{}}}^{{{}}}",
            named_value("e"),
            named_value("sub"),
            named_value("sup")
        ),
        "rad" => {
            let d = if child(n, "radPr")
                .and_then(|p| child(p, "degHide"))
                .and_then(|p| attr(p, "val"))
                .is_some_and(|v| matches!(v, "1" | "true" | "on"))
            {
                String::new()
            } else {
                named_value("deg")
            };
            let e = named_value("e");
            if d.is_empty() {
                format!("\\sqrt{{{e}}}")
            } else {
                format!("\\sqrt[{d}]{{{e}}}")
            }
        }
        "d" => {
            let prop = child(n, "dPr");
            let delim = |key, default| {
                prop.and_then(|p| child(p, key))
                    .and_then(|p| attr(p, "val"))
                    .unwrap_or(default)
            };
            format!(
                "{}{}{}",
                literal(delim("begChr", "(")),
                named_value("e"),
                literal(delim("endChr", ")"))
            )
        }
        "nary" => {
            let op = child(n, "naryPr")
                .and_then(|p| child(p, "chr"))
                .and_then(|p| attr(p, "val"))
                .unwrap_or("∫");
            format!(
                "{}_{{{}}}^{{{}}}{}",
                literal(op).trim(),
                named_value("sub"),
                named_value("sup"),
                named_value("e")
            )
        }
        "func" => format!(
            "\\operatorname{{{}}}{}",
            named_value("fName"),
            named_value("e")
        ),
        "munder" => format!("{}_{{{}}}", at(0), at(1)),
        "mover" => format!("\\overset{{{}}}{{{}}}", at(1), at(0)),
        "munderover" => format!("{}_{{{}}}^{{{}}}", at(0), at(1), at(2)),
        "mtable" | "m" => format!("\\begin{{matrix}}{}\\end{{matrix}}", vals.join(r" \\ ")),
        "mtr" | "mr" => vals.join(" & "),
        "semantics" => at(0),
        "r" if child(n, "rPr")
            .and_then(|p| child(p, "nor"))
            .is_some_and(|p| attr(p, "val") != Some("0")) =>
        {
            format!("\\text{{{}}}", literal(&content(n)))
        }
        "oMathPara" => {
            let expressions: Vec<_> = children
                .iter()
                .zip(&vals)
                .filter(|(c, _)| named(**c, "oMath"))
                .map(|(_, v)| v.as_str())
                .collect();
            if expressions.len() > 1 {
                format!(
                    "\\begin{{gathered}}{}\\end{{gathered}}",
                    expressions.join(r" \\ ")
                )
            } else {
                joined()
            }
        }
        "math" | "mrow" | "mstyle" | "mtd" | "oMath" | "r" | "e" | "num" | "den" | "sub"
        | "sup" | "deg" | "fName" => joined(),
        "mspace" => " ".into(),
        _ => {
            warn(
                ctx,
                "office_math_structure",
                "Unsupported equation structure; available child expressions retained.",
                src,
            );
            if children.is_empty() {
                literal(&content(n))
            } else {
                joined()
            }
        }
    };
    if output.len() > ctx.options.limits.max_text_bytes {
        return Err(Error::Limit("max_text_bytes"));
    }
    Ok(output)
}
pub(super) fn inline(
    n: Node<'_, '_>,
    ctx: &mut Context<'_>,
    src: &Source,
) -> Result<Inline, Error> {
    let tex = walk(n, ctx, src, 0)?;
    ctx.text(&tex)?;
    if tex.trim().is_empty() {
        warn(
            ctx,
            "office_math_empty",
            "Equation has no supported expression content.",
            src,
        );
    }
    Ok(Inline::Math {
        tex,
        display: n.attribute("display") == Some("block")
            || n.descendants().any(|n| named(n, "oMathPara")),
    })
}
