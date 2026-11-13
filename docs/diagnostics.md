# Diagnostic categories

Schema **0.7** adds `category` and `evidence` to every parser and Markdown-rendering
diagnostic in Rust, JSON, Node, Python and WASM. The CLI prints both alongside the
code. These fields classify warnings; they do not change extracted content.

| Category | Meaning |
| --- | --- |
| `missing_content` | Source content was omitted, or content coverage is uncertain. Includes images/objects, not just text. |
| `formatting_limitation` | Text/values survive, but styling, structure, numbering, links or placement are incomplete. |
| `informational` | Source conditions, normalization, cached values or declared visibility policies. |

Always check `evidence` alongside category:

- `detected`: the parser encountered a specific object/condition. For missing
  content this identifies a known omission, such as an embedded image it skipped.
- `unverified`: a broad capability notice or an uncertain condition. It does not
  establish actual loss. A drawing can contain only extracted text boxes; a broad
  legacy-format warning does not prove that a particular document lost text.

A document can have several categories. Count documents once within each category,
and use disjoint buckets for totals: detected missing content; otherwise unverified
missing-content risk; otherwise formatting limitations; otherwise informational
only; otherwise no diagnostics. Include both parser and render diagnostics, keeping
those stages separate in detailed reports.

## Compatibility and interpretation

`content_loss` retains its legacy meaning: a broad fidelity/uncertainty flag.
Likewise, `status: partial` and strict-mode rejection keep their existing behavior.
Neither is a count of confirmed omissions. The new category does not override the
old strict-mode flag. For example, a formula missing its source cache remains a
strict-mode warning even though its category is informational.

Formulas are not evaluated. If a source has no cached result, preserving its
expression is not a parser omission. A source error-valued cell is also a source
condition. Hidden content omitted by the documented visibility policy is disclosed
as informational, not counted as an extraction failure. This does not mean the
output contains all hidden data. Formatting limitations can still matter greatly:
numbering, links and table structure may carry meaning.

Unknown future loss warnings conservatively become `missing_content/unverified`;
they never silently become formatting-only or confirmed omissions. Categories are
based on parser behavior and diagnostic codes, never document names or benchmark
identities. Schema 0.7 requires the new fields when deserializing diagnostics;
Rust callers should use `Diagnostic::new(...)` to construct classified diagnostics.

Absence of a detected-loss warning is **not proof of complete extraction**. These
are parser-reported findings, not an independent visual or ground-truth quality
assessment. Detected visual omissions are not a count of missing-text documents.

Schema 0.8 additionally retains image assets. `image_assets_retained` is informational; `image_asset_unavailable` reports a detected unsupported or budget-omitted image. Existing visual-fidelity diagnostics are not cleared merely because some resource bytes were retained. See [images](images.md).
