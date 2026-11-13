//! Referenced auxiliary stories, kept separate from the main reading sequence.
use super::*;
use std::collections::HashSet;

impl Walker<'_, '_> {
    pub(super) fn auxiliary(
        &mut self,
        pkg: &mut Package<'_>,
        body: Node<'_, '_>,
        output: &mut Vec<Block>,
    ) -> Result<(), Error> {
        let main = self.part.clone();
        let mut wanted = Vec::new();
        let mut seen = HashSet::new();
        for n in body
            .descendants()
            .filter(|n| w(*n, "headerReference") || w(*n, "footerReference"))
        {
            let kind = if w(n, "headerReference") {
                "header"
            } else {
                "footer"
            };
            let rel = n
                .attribute((R, "id"))
                .or_else(|| n.attribute((R_STRICT, "id")))
                .and_then(|id| self.relationships.get(id));
            if let Some(rel) = rel.filter(|r| !r.external && r.kind.ends_with(&format!("/{kind}")))
            {
                let path = resolve(&main, &rel.target)?;
                if seen.insert(path.clone()) {
                    wanted.push((kind.to_string(), path));
                }
            } else {
                self.warn(
                    "docx_auxiliary_missing",
                    "Referenced header/footer relationship is unavailable.",
                );
            }
        }
        for kind in ["footnote", "endnote", "comment"] {
            if !self.note_refs.iter().any(|(k, _)| k == kind) {
                continue;
            }
            let suffix = format!("/{kind}s");
            let mut rels: Vec<_> = self
                .relationships
                .values()
                .filter(|r| r.kind.ends_with(&suffix) && !r.external)
                .collect();
            rels.sort_by(|a, b| a.target.cmp(&b.target));
            let path = rels
                .first()
                .map(|r| resolve(&main, &r.target))
                .transpose()?
                .unwrap_or(resolve(&main, &format!("{kind}s.xml"))?);
            if seen.insert(path.clone()) {
                wanted.push((kind.into(), path));
            }
        }
        let main_rels = std::mem::take(&mut self.relationships);
        let mut resolved = HashSet::new();
        for (kind, path) in wanted {
            self.part = path.clone();
            let Some(text) = pkg.read(&path)? else {
                self.warn(
                    "docx_auxiliary_missing",
                    "Referenced auxiliary part is missing.",
                );
                continue;
            };
            let doc = match xml(&text, self.ctx) {
                Ok(d) => d,
                Err(Error::Malformed(_)) => {
                    self.warn(
                        "docx_auxiliary_unreadable",
                        "Auxiliary part is malformed; main document content retained.",
                    );
                    continue;
                }
                Err(e) => return Err(e),
            };
            let rel_text = pkg.read(&rels_name(&path))?;
            self.relationships = relationships(rel_text.as_deref(), self.ctx)?;
            if kind == "header" || kind == "footer" {
                let mut blocks = self.blocks(doc.root_element(), 0)?;
                blocks.retain(|b| !empty_block(b));
                if !blocks.is_empty() {
                    let label = if kind == "header" {
                        "Document header"
                    } else {
                        "Document footer"
                    };
                    self.ctx.text(label)?;
                    output.push(self.ctx.block(
                        BlockContent::Heading {
                            level: 2,
                            inlines: vec![Inline::text(label)],
                        },
                        Source {
                            part: Some(path.into()),
                            ..Default::default()
                        },
                    )?);
                    output.extend(blocks);
                    self.warn("docx_auxiliary_layout", "Referenced headers/footers appended once; pagination and section-specific placement are not reconstructed.");
                }
            } else {
                // Resolve in reference order; separator records and unreferenced
                // editorial notes are not document body text.
                let references: Vec<_> = self
                    .note_refs
                    .iter()
                    .filter(|(k, _)| k == &kind)
                    .cloned()
                    .collect();
                for key in references {
                    let Some(n) = doc.root_element().children().find(|n| {
                        w(*n, &kind)
                            && attr(*n, "id") == Some(key.1.as_str())
                            && attr(*n, "type").is_none_or(|t| t == "normal")
                    }) else {
                        continue;
                    };
                    resolved.insert(key.clone());
                    let label = format!("{} {}", kind, key.1);
                    self.ctx.text(&label)?;
                    output.push(self.ctx.block(
                        BlockContent::Heading {
                            level: 2,
                            inlines: vec![
                                Inline::Anchor {
                                    id: format!("docx-{}-{}", kind, key.1),
                                },
                                Inline::text(label),
                            ],
                        },
                        self.source(n),
                    )?);
                    output.extend(self.blocks(n, 0)?);
                }
            }
        }
        self.part = main;
        self.relationships = main_rels;
        if self.note_refs.iter().any(|k| !resolved.contains(k)) {
            self.warn(
                "docx_note_missing",
                "One or more referenced notes/comments could not be resolved.",
            );
        }
        Ok(())
    }
}
