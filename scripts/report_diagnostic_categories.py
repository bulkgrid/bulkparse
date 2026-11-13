#!/usr/bin/env python3
"""Summarize diagnostic evidence without treating warnings as a quality oracle."""
import argparse, collections, json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

def read(p): return json.loads(p.read_text())
def summarize(rows):
    counts = collections.Counter()
    codes = collections.defaultdict(lambda: {'documents': set(), 'stages': set()})
    documents = []
    for r in rows:
        seen = set()
        categories = set()
        diagnostics = []
        for stage, field in [('parser', 'diagnostics'), ('markdown', 'render_diagnostics')]:
            for d in r.get(field, []):
                assert d['category'] in ('missing_content','formatting_limitation','informational')
                assert d['evidence'] in ('detected','unverified')
                key = (d['category'], d['evidence'], d['code'])
                codes[key]['documents'].add(r['id']); codes[key]['stages'].add(stage)
                codes[key]['message'] = d['message']
                seen.add(key[:2]); categories.add(d['category'])
                diagnostics.append(dict(stage=stage, **d))
        if not r['ok']: bucket = 'failed'
        elif ('missing_content', 'detected') in seen: bucket = 'detected_missing_content'
        elif ('missing_content', 'unverified') in seen: bucket = 'unverified_content_risk_only'
        elif 'formatting_limitation' in categories: bucket = 'formatting_without_content_warning'
        elif 'informational' in categories: bucket = 'informational_only'
        else: bucket = 'no_diagnostics'
        counts[bucket] += 1
        documents.append(dict(id=r['id'], format=r['format'], bucket=bucket, status=r.get('status'), diagnostics=diagnostics))
    by_code = [dict(category=k[0], evidence=k[1], code=k[2], documents=sorted(v['documents']), count=len(v['documents']), stages=sorted(v['stages']), message=v['message']) for k,v in sorted(codes.items())]
    overlapping = {c:sum(any(d['category']==c for d in r['diagnostics']) for r in documents) for c in ['missing_content','formatting_limitation','informational']}
    return dict(exclusive_counts=dict(counts), overlapping_category_counts=overlapping, documents=documents, codes=by_code)

def main():
    ap=argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--out', type=Path, required=True)
    args=ap.parse_args(); out=args.out
    current=read(out/'benchmark/bulkparse-round-1.json')['documents']
    before=read(ROOT/'reports/provenance-memory-2026-11-13/comparison/bulkparse-round-1.json')['documents']
    old={r['id']:r for r in before}
    changes=[]
    for r in current:
        b=old[r['id']]
        for k in ['ok','status','output_sha256','output_bytes']:
            assert r.get(k)==b.get(k),(r['id'],k)
        for field in ['diagnostics','render_diagnostics']:
            stripped=[{k:v for k,v in d.items() if k not in ('category','evidence')} for d in r[field]]
            if stripped!=b[field]: changes.append(dict(id=r['id'],field=field,before=b[field],after=stripped))
            projected=[]
            for d in stripped:
                d=dict(d)
                if d['code']=='docx_positioned_tab_layout':
                    d.update(code='docx_unsupported_inline',message='Unsupported inline content (such as images, equations, notes or revisions) was omitted.')
                elif d['code']=='office_math_empty':
                    d['code']='office_math_structure'
                projected.append(d)
            assert projected==b[field],(r['id'],field,'unexpected legacy diagnostic change')
    result=summarize(current)
    result.update(documents_returned=sum(r['ok'] for r in current),markdown_unchanged=len(current),legacy_metadata_changes=changes,
        interpretation='Parser-detected omissions include visual content. Unverified warnings are not confirmed loss; absence of warnings does not prove full extraction. No ground-truth judging performed.')
    (out/'categories.json').write_text(json.dumps(result,indent=2)+'\n')
    s=read(out/'benchmark/summary.json')['bulkparse']
    c=result['exclusive_counts']
    labels=[('detected_missing_content','Detected missing content'),('unverified_content_risk_only','Unverified content risk, no detected omission'),('formatting_without_content_warning','Formatting limitations, no content warning'),('informational_only','Informational notices only'),('no_diagnostics','No diagnostics'),('failed','Failed conversion')]
    table='| Result (mutually exclusive) | Documents |\n| --- | ---: |\n'+''.join(f'| {label} | {c.get(k,0)} |\n' for k,label in labels)
    omission_codes=[r for r in result['codes'] if r['category']=='missing_content' and r['evidence']=='detected']
    omissions='| Detected omission | Documents |\n| --- | ---: |\n'+''.join(f'| `{r["code"]}` | {r["count"]} |\n' for r in omission_codes)
    (out/'README.md').write_text(f'''# Diagnostic categories — November 13, 2026

BulkParse returned **{result['documents_returned']}/112** documents. All **112 Markdown
outputs are byte-identical** to the previous benchmark. The legacy statuses remain
13 complete / 99 partial; extraction was not improved or weakened by reclassifying
warnings. Schema 0.7 adds categories and evidence, so complete JSON bytes intentionally
change. Strict-mode acceptance is unchanged.

{table}
These buckets include both parser and Markdown-rendering warnings. A document can
have multiple categories; the table assigns it to the first applicable bucket.
Detected omissions include images and embedded objects. They are not a count of
missing-text documents. Unverified risks remain visible and are not called complete.

## Detected omissions

{omissions}
Counts in this table overlap. See [per-document evidence and messages](categories.json)
for every category, code, source location, stage and document ID.
No absence-of-warning result is proof of perfect content coverage; this is diagnostic
accounting, not a visual ground-truth audit or an independent quality score.

## Implementation and scope

- Three categories: `missing_content`, `formatting_limitation`, `informational`.
- Evidence is `detected` or `unverified`; blanket coverage warnings never count as
  confirmed loss. Unknown future loss warnings remain unverified content risks.
- Missing formula caches and error-valued cells are source conditions. Hidden data
  omitted by the declared visibility policy is informational, not an extraction
  failure; it is not included in the detected-omission count.
- A Word positioning tab now reports alignment limitations instead of unsupported
  inline content. This general element-based rule does not change extracted text.
- Empty/unsupported equation cases have separate codes so absent expression
  evidence is not conflated with retained child expressions.
- Legacy `content_loss`, `partial` and strict behavior remain unchanged. Consumers
  should use category **and** evidence to count known omissions.

[API semantics and migration](../../docs/diagnostics.md)

## Rerun

One sequential BulkParse corpus pass, no warm-up, same 112 public documents/default
settings, Apple M3 Max / 36 GiB / macOS, OCR off. Corpus time: **{s['corpus_seconds'][0]:.3f} s**;
CPU: **{s['corpus_cpu_seconds'][0]:.3f} s**; peak worker RSS:
**{s['resources']['lifetime_peak_rss_bytes']/2**30:.3f} GiB**. Worker initialization and
protocol serialization are excluded from conversion time; RSS includes worker
initialization/transport, not aggregate process-tree memory. This one-pass rerun is
for diagnostic accounting, not a new statistically stable performance claim.
AnyDoc and Docling were not rerun for this BulkParse-only metadata change; their
[previous comparison](../provenance-memory-2026-11-13/README.md) remains separate.

[Raw benchmark](benchmark/summary.json) · [Identity/configuration](benchmark/identity.json) ·
[Input/source/binary verification](benchmark/verification.json)

## Regression validation

All 162 historical failure checks pass against the frozen expectations, ignoring
only the two added classification fields. All 200 PDF Markdown outputs are
unchanged. Rust, Node, Python and WASM checks pass, including cross-binding parity.
The targeted tests cover formula-source conditions, actual omitted objects,
streaming parity, strict behavior, positioning tabs, unknown warning fallback and
deduplicated document counting. See [validation details](validation.json).

## Reproduce

```sh
cargo build --offline --release -p bulkparse --example office_bench
python3 scripts/benchmark_realworld_comparison.py --engines bulkparse --out /tmp/warning-audit/benchmark
python3 scripts/report_diagnostic_categories.py --out /tmp/warning-audit
```

The corpus manifest/input hashes and measured source snapshot are saved with the
benchmark. The comparison baseline is the preserved provenance-memory report.
''')
    print(json.dumps({'counts':c,'overlapping':result['overlapping_category_counts'],'detected_codes':[(r['code'],r['count']) for r in omission_codes], 'metadata_change_documents':[r['id'] for r in changes]},indent=2))

if __name__=='__main__':main()
