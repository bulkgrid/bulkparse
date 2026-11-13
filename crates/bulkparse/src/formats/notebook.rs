use crate::{Block, BlockContent, Context, Coverage, Error, Source, Status};
use serde_json::Value;

fn string(value: &Value) -> Result<String, Error> {
    if let Some(s) = value.as_str() {
        return Ok(s.to_owned());
    }
    if let Some(parts) = value.as_array() {
        let mut out = String::new();
        for part in parts {
            out.push_str(part.as_str().ok_or_else(|| {
                Error::Malformed("notebook text array must contain strings".into())
            })?);
        }
        return Ok(out);
    }
    Err(Error::Malformed(
        "notebook text must be a string or string array".into(),
    ))
}

pub fn parse(bytes: &[u8], ctx: &mut Context<'_>) -> Result<Vec<Block>, Error> {
    let notebook: Value =
        serde_json::from_slice(bytes).map_err(|e| Error::Malformed(e.to_string()))?;
    if notebook.get("nbformat").and_then(Value::as_u64) != Some(4) {
        return Err(Error::Unsupported("only nbformat 4 is supported".into()));
    }
    let cells = notebook
        .get("cells")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Malformed("notebook missing cells".into()))?;
    let language = notebook
        .pointer("/metadata/language_info/name")
        .and_then(Value::as_str)
        .unwrap_or("");
    let mut blocks = Vec::new();
    for (index, cell) in cells.iter().enumerate() {
        ctx.node()?;
        let source = Source {
            cell: Some(index as u32 + 1),
            ..Default::default()
        };
        let start_diagnostics = ctx.diagnostics.len();
        let text = string(
            cell.get("source")
                .ok_or_else(|| Error::Malformed("notebook cell missing source".into()))?,
        )?;
        match cell.get("cell_type").and_then(Value::as_str) {
            Some("markdown") => blocks.extend(super::markdown::parse(&text, ctx, source.clone())?),
            Some("code") => {
                ctx.text(&text)?;
                blocks.push(ctx.block(
                    BlockContent::Code {
                        language: language.to_string(),
                        text,
                    },
                    source.clone(),
                )?);
                if let Some(outputs) = cell.get("outputs").and_then(Value::as_array) {
                    for output in outputs {
                        ctx.node()?;
                        let value = output
                            .get("text")
                            .or_else(|| output.pointer("/data/text/plain"));
                        if let Some(value) = value {
                            let text = string(value)?;
                            ctx.text(&text)?;
                            blocks.push(ctx.block(
                                BlockContent::Code {
                                    language: "text".into(),
                                    text,
                                },
                                source.clone(),
                            )?);
                        } else if output.get("output_type").and_then(Value::as_str) == Some("error")
                        {
                            let text = format!(
                                "{}: {}",
                                output
                                    .get("ename")
                                    .and_then(Value::as_str)
                                    .unwrap_or("Error"),
                                output.get("evalue").and_then(Value::as_str).unwrap_or("")
                            );
                            ctx.text(&text)?;
                            blocks.push(ctx.block(
                                BlockContent::Code {
                                    language: "text".into(),
                                    text,
                                },
                                source.clone(),
                            )?);
                        } else {
                            ctx.diagnostic("notebook_rich_output", "An output has no text representation; rich output extraction is not implemented.", source.clone(), true);
                        }
                        if output
                            .get("data")
                            .and_then(Value::as_object)
                            .is_some_and(|d| d.keys().any(|k| k != "text/plain"))
                        {
                            ctx.diagnostic("notebook_rich_output", "Rich MIME outputs are not extracted, even when a text alternative is present.", source.clone(), true);
                        }
                    }
                }
            }
            Some("raw") => {
                ctx.text(&text)?;
                blocks.push(ctx.block(
                    BlockContent::Raw {
                        media_type: "text/plain".into(),
                        text,
                    },
                    source.clone(),
                )?);
            }
            _ => return Err(Error::Malformed("unknown notebook cell type".into())),
        }
        if cell
            .get("attachments")
            .and_then(Value::as_object)
            .is_some_and(|a| !a.is_empty())
        {
            ctx.diagnostic(
                "notebook_attachments",
                "Notebook attachments are not extracted in this version.",
                source.clone(),
                true,
            );
        }
        let status = if ctx.diagnostics[start_diagnostics..]
            .iter()
            .any(|d| d.content_loss)
        {
            Status::Partial
        } else {
            Status::Complete
        };
        ctx.coverage.push(Coverage { source, status });
    }
    Ok(blocks)
}
