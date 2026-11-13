//! Read cached Office chart series as data tables without model inference.
use super::*;
fn points(n: Node<'_, '_>) -> Result<BTreeMap<usize, String>, Error> {
    let mut out = BTreeMap::new();
    for pt in n.descendants().filter(|n| named(*n, "pt")) {
        let index = count(pt.attribute("idx"), 0)?;
        if let Some(v) = child(pt, "v") {
            out.insert(index, content(v));
        }
    }
    Ok(out)
}
pub(super) fn read(
    pkg: &mut Package<'_>,
    part: &str,
    src: &Source,
    ctx: &mut Context<'_>,
) -> Result<Vec<Block>, Error> {
    let s = pkg.required(part)?;
    let doc = xml(&s, ctx)?;
    let source = Source {
        part: Some(part.into()),
        ..src.clone()
    };
    let mut out = vec![];
    if let Some(title) = doc.descendants().find(|n| named(*n, "title")) {
        let title: String = title
            .descendants()
            .filter(|n| named(*n, "t"))
            .map(content)
            .collect();
        if !title.is_empty() {
            out.push(heading(title, 3, &source, ctx)?);
        }
    }
    for series in doc.descendants().filter(|n| named(*n, "ser")) {
        let title = child(series, "tx")
            .map(|n| {
                n.descendants()
                    .filter(|n| named(*n, "v") || named(*n, "t"))
                    .map(content)
                    .collect::<String>()
            })
            .unwrap_or_default();
        if !title.is_empty() {
            out.push(heading(title, 4, &source, ctx)?);
        }
        let cats = child(series, "cat")
            .or_else(|| child(series, "xVal"))
            .map(points)
            .transpose()?
            .unwrap_or_default();
        let values = child(series, "val")
            .or_else(|| child(series, "yVal"))
            .map(points)
            .transpose()?
            .unwrap_or_default();
        if values.is_empty() {
            warn(
                ctx,
                "office_chart_missing_cache",
                "Chart series has no cached values; its linked workbook is not evaluated.",
                &source,
            );
            continue;
        }
        let indices: std::collections::BTreeSet<_> =
            cats.keys().chain(values.keys()).copied().collect();
        if indices.len() > ctx.options.limits.max_table_cells / 2 {
            return Err(Error::Limit("max_table_cells"));
        }
        let mut grid = BTreeMap::new();
        for (row, i) in indices.into_iter().enumerate() {
            grid.insert(
                (row as u32 + 1, 1),
                cats.get(&i).cloned().unwrap_or_default(),
            );
            grid.insert(
                (row as u32 + 1, 2),
                values.get(&i).cloned().unwrap_or_default(),
            );
        }
        if let Some(table) = table(grid, vec![], &source, ctx)? {
            out.push(table)
        }
    }
    ctx.diagnostic("office_chart_cache","Chart text and cached series extracted; caches may be stale and visual encoding is not reconstructed.",source,true);
    Ok(out)
}
