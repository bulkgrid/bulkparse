# Historical real-world parsing failures

This permanent regression suite contains **all 27 documents BulkParse failed to
parse in any recorded round of the original eight-converter benchmark**. Selection
is mechanical, using the preserved original results; it is not based on which
documents later improved. The manifest preserves download URLs, source notices,
file hashes, original failure reasons and result-file hashes.

These documents already belong to the 112-document real-world corpus. They are
not duplicated or given additional weight in full-corpus results. Report this
27-document subset separately to make regressions visible. It contains nine CSV,
five ODS, five XLS, three XLSM, three RTF and two XLSX inputs.

Default structured and Markdown-writer parsing now return all 27 historical
failures without resource-limit recovery. The earlier conservative limits remain
covered separately, with recovery off and on: 15 still reject at those deliberately
smaller budgets. Rejection is a tested limit, not successful conversion. Output
equality is a regression check, not evidence of complete visual fidelity.

```sh
python3 scripts/restore_realworld_corpus.py --manifest benchmarks/realworld-failures/manifest.json
cargo build --release --locked -p bulkparse --example office_bench
python3 scripts/check_realworld_failures.py --out /tmp/bulkparse-realworld-failures
```

Use a fresh output directory. Missing or changed inputs fail validation; the
runner does not silently skip them or refresh expected results. Review deliberate
parser changes before updating expectations. The corpus files remain in ignored
`target/realworld/corpus`; the manifest restores them without committing third-party
documents. Preserve their individual license and attribution notices.

The original [112-document study](../realworld/README.md) documents selection and
limitations. These files are development/regression inputs, not an unseen holdout.

## Default complete output

The checker makes 162 outcome/content/diagnostic comparisons across six modes:
default structured/writer output, and the four original behaviors under explicit
conservative limits. No large-document flag is required. Expectations preserve
all original corpus members and retain old rejection/recovery evidence.
See automatic handling measurements (historical artifact; not distributed).

The fidelity follow-up (historical artifact; not distributed) updates
three RTF expectations after source review: two add footer sections and one
clarifies its object-omission diagnostic. Their original body text remains intact;
all other expectations and all 27 selected documents are preserved.
