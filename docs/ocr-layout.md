# OCR geometry and reading order — 2026-11-10

Follow-up: [explicit recognition profiles and scan stress tests](ocr-profiles.md)
compare model quality and resources while retaining these layout rules.

The optional OCR path now preserves each recognized line's quadrilateral,
rendered image dimensions and engine confidence. Rust applies bounded whitespace
cuts to recover horizontal left-to-right column order. On the existing authored
two-column scan, the text now matches the expected column-first order exactly.
There are no filename checks, phrase matches, spelling repairs or model changes.

## Evidence

The same eight PDFs, reference texts, model files and recognition settings from
the [initial OCR evaluation](ocr-evaluation.md) were reused. Expectations and
fixtures were not regenerated. This is a regression evaluation on a known corpus,
not independent evidence of real-world scan accuracy.

| Case / metric | Previous text-only path | Geometry path |
| --- | --- | --- |
| Two-column word error rate | 64% | 0% |
| Two-column exact line-pair order | 80% | 100% |
| English prose and invoice values | Exact | Exact |
| Two-page and existing scan-only cases | Exact | Exact |
| Native page followed by scan | Exact | Exact; native page unchanged |
| Blank scan | Empty with diagnostic | Empty with diagnostic |
| Swedish accent | `Åtta` → `Atta` | Same error, 1/67 character error rate |

Seven of eight cases match after NFC/whitespace normalization, including the
blank case. The two-page case reuses prose/invoice content and is not independent
recognition evidence. All results remain **partial**. The column gate now
requires exact text and 100% line-pair order; the Swedish error remains explicit.

The run metadata (historical artifact; not distributed) and
per-case measurements (historical artifact; not distributed) record
the binary/source/model/fixture hashes, settings and outputs. A separate raw
engine response (historical artifact; not distributed) allows
comparison with the converted JSON (historical artifact; not distributed).
The integration check verifies unchanged text, dimensions, quadrilaterals and
confidence; only line order changes. Precise float decoding avoids rounding
subpixel coordinates during transport.

## Design and bounds

Schema **0.2** adds optional `source.ocr` to OCR line blocks. Coordinates are in
rendered-page pixels, origin top left; they are not PDF point coordinates.
Confidence is an uncalibrated engine score, not a correctness guarantee. Each
line remains a separate paragraph block; paragraph grouping is not inferred.
The optional evidence uses a boxed allocation so ordinary sources only carry
an optional pointer. No OCR runtime or model was added to default dependencies.

Ordering uses axis-aligned bounds around the original quadrilaterals. A vertical
gutter must be at least 1.5 median line heights wide, with at least two lines on
each side and overlapping vertical extents. Qualifying columns are visited left
to right. Where no column cut qualifies, spacious horizontal bands can separate
spanning headings or footers. Ambiguous/overlapping regions retain engine order;
pages with tall or substantially skewed lines skip the horizontal layout model.

This is bounded to 4096 lines per page, 32 recursion levels and 32 vertical-gap
candidates per region. Cancellation is checked during recursion. Coordinates,
convexity, dimensions, confidence and line text are validated before layout.
Text budgets include intervening newlines, transport metadata is bounded, and
existing node/page/input/pixel/timeout controls still apply. No OS memory sandbox
or persistent inference process has been introduced.

The old text-only provider method remains supported. The new `recognize_page`
method supplies structured evidence; the parser invokes it through the same
provider boundary. Node/Python/WASM types and serialized results use schema 0.2;
OCR invocation remains limited to the Rust API and opt-in CLI. See
[provider and migration details](pdf-ocr.md) and [setup](../backends/rapidocr/README.md).

## Verification and limitations

Five layout tests cover translated/scaled columns, three columns with a spanning
heading/footer, ambiguous regions, skew and cancellation. Three core integration
tests cover JSON/Markdown/provenance, malformed evidence with native-page
preservation, and text/geometry/node budgets. Seven process-provider tests cover
both protocol versions, precise coordinates, failures, bounds and lifecycle.
Existing parser, pool and cross-language regressions are also checked; see the
verification record (historical artifact; not distributed).

Horizontal whitespace cuts are not general layout understanding. Borderless
tables can resemble columns, tightly spaced spanning titles can defeat cuts,
and RTL, vertical writing, complex mixed layouts and scanned table structure
remain unverified. Skewed/overlapping evidence is preserved without claiming
correct order. Same-page native/scanned mixtures and bad hidden text layers are
still routing gaps. The next quality work is a broader human-reviewed scan set,
multilingual recognition and table/paragraph reconstruction.
