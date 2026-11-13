// Functional host-bridge check, not a timing benchmark. GC between documents
// bounds retained previous results; the complete current result remains live.
import {readFileSync,writeFileSync,appendFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {isDeepStrictEqual} from 'node:util';
import {parseFile} from '../bindings/node/index.cjs';
const root = new URL('../', import.meta.url);
const report = new URL((process.argv[2] ?? 'reports/full-document-coverage-2026-11-13/').replace(/\/?$/, '/'), root);
const corpus=JSON.parse(readFileSync(new URL('reports/realworld-2026-11-12/corpus-manifest.json',root)));
const expected=new Map(JSON.parse(readFileSync(new URL(process.argv[3] ?? 'comparison/after.json',report))).documents.map(r=>[r.id,r]));
const options=process.argv.includes('--default-options') ? {} : {limits:{max_nodes:16777216,max_table_cells:4194304,max_expanded_bytes:268435456,max_markdown_bytes:134217728}};
const rows=[];
for (const doc of corpus) {
  global.gc?.();
  let row;
  try {
    const result=await parseFile(new URL(doc.path,root).pathname,options);
    const hash=createHash('sha256').update(result.markdown).digest('hex');
    const reference=expected.get(doc.id);
    row={id:doc.id,ok:true,status:result.status,top_level_blocks:result.document.blocks.length,output_sha256:hash,diagnostics:result.diagnostics,
      matches:hash===reference.output_sha256 && result.status===reference.status && isDeepStrictEqual(result.diagnostics,reference.diagnostics)};
  } catch (e) { row={id:doc.id,ok:false,error:String(e),matches:false}; }
  rows.push(row);appendFileSync(new URL('node-large-documents.jsonl',report),JSON.stringify(row)+'\n');
  if (rows.length%10===0 || !row.ok) console.log(rows.length,doc.id,row.ok,row.error??'');
}
const summary={documents:rows.length,returned:rows.filter(r=>r.ok).length,matched:rows.filter(r=>r.matches).length,
  options,node:process.version,scope:'Native Node parseFile returns the complete JS document object and Markdown; GC requested between documents. Not a timing benchmark.',rows};
writeFileSync(new URL('node-large-documents.json',report),JSON.stringify(summary,null,2)+'\n');
if (summary.matched!==112) process.exitCode=1;
