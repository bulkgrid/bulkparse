use super::*;
use std::borrow::Cow;

#[derive(Default)]
pub(super) struct SharedStrings {
    memory: Vec<Vec<Run>>,
    #[cfg(not(target_arch = "wasm32"))]
    disk: Option<super::super::xml_parts::Store>,
    #[cfg(not(target_arch = "wasm32"))]
    cache: HashMap<usize, Vec<Run>>,
    #[cfg(not(target_arch = "wasm32"))]
    cache_bytes: usize,
}
impl SharedStrings {
    pub fn load(pkg: &mut Package<'_>, path: &str, ctx: &mut Context<'_>) -> Result<Self, Error> {
        let mut result = Self::default();
        if !pkg.has(path) {
            return Ok(result);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(parts) = super::super::xml_parts::partition_mapped(
            pkg,
            path,
            super::super::xml_parts::Kind::Strings,
            ctx,
            Some(&mut |si| {
                let values = runs(si);
                check(&values, ctx)?;
                serde_json::to_vec(&values).map_err(bad)
            }),
        )? {
            parts.account(ctx)?;
            xml(&parts.projection, ctx)?;
            result.disk = Some(parts.into_store());
            return Ok(result);
        }
        let source = pkg.required_xml(path, ctx)?;
        let doc = xml(&source, ctx)?;
        for si in doc.root_element().children().filter(|n| named(*n, "si")) {
            let values = runs(si);
            check(&values, ctx)?;
            result.memory.push(values);
        }
        Ok(result)
    }
    pub fn get(&mut self, index: usize) -> Result<Cow<'_, [Run]>, Error> {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(store) = &mut self.disk {
            if self.cache.contains_key(&index) {
                return Ok(Cow::Borrowed(self.cache.get(&index).unwrap()));
            }
            let values: Vec<Run> = serde_json::from_slice(&store.get(index)?).map_err(bad)?;
            let size = 64
                + values.capacity() * std::mem::size_of::<Run>()
                + values.iter().map(|r| r.text.capacity()).sum::<usize>();
            // A bounded cache, independent of document size. Large individual
            // strings are served without retaining a second cached copy.
            if size <= 256 * 1024 {
                if self.cache_bytes.saturating_add(size) > 256 * 1024 {
                    self.cache.clear();
                    self.cache_bytes = 0;
                }
                self.cache.insert(index, values);
                self.cache_bytes += size;
                return Ok(Cow::Borrowed(self.cache.get(&index).unwrap()));
            }
            return Ok(Cow::Owned(values));
        }
        self.memory
            .get(index)
            .map(|values| Cow::Borrowed(values.as_slice()))
            .ok_or_else(|| bad("shared string outside table"))
    }
}
fn check(values: &[Run], ctx: &Context<'_>) -> Result<(), Error> {
    if values.iter().map(|r| r.text.len()).sum::<usize>() > ctx.options.limits.max_text_bytes {
        return Err(Error::Limit("max_text_bytes"));
    }
    Ok(())
}
