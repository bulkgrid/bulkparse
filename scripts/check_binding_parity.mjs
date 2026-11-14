import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {spawnSync} from 'node:child_process';
import {dirname, resolve} from 'node:path';
import {fileURLToPath, pathToFileURL} from 'node:url';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const {parse: native, inspectPdf: inspectNative} = await import(pathToFileURL(root + '/bindings/node/index.cjs'));
const {init, parse: wasm, inspectPdf: inspectWasm} = await import(pathToFileURL(root + '/bindings/wasm/index.js'));
await init({module_or_path: readFileSync(root + '/bindings/wasm/pkg/bulkparse_wasm_bg.wasm')});
const cases = ['hello.docx','hello.md','table.csv','notebook.ipynb','native-and-empty.pdf'].map(name => [name, readFileSync(root+'/fixtures/'+name), {}]);
cases.push(['x.txt',Buffer.from('Plain text'),{}], ['x.tsv',Buffer.from('a\tb\nx\ty'),{}], ['x.csv',Buffer.from('a;b\n名称;😀','utf16le'),{csv_encoding:'utf16_le',csv_delimiter:';'}]);
if (process.argv.includes('--pdf-corpus')) {
 for (const name of ['newspaper-00.pdf', 'elsevier-00.pdf', 'right_to_left_01.pdf']) {
  cases.push([name, readFileSync(root + '/legacy/docling-main/tests/data/pdf/sources/' + name), {}]);
 }
 cases.push(['ruled-table.pdf',readFileSync(root+'/fixtures/holdout/ruled-table.pdf'),{}]);
 cases.push(['arabic-mixed.pdf',readFileSync(root+'/fixtures/holdout/arabic-mixed.pdf'),{}]);
 cases.push(['arabic-mixed.pdf',readFileSync(root+'/fixtures/holdout/arabic-mixed.pdf'),{pdf_normalize_unicode:false}]);
 cases.push(['spanning-title.pdf',readFileSync(root+'/fixtures/holdout/spanning-title.pdf'),{}]);
}
if (process.argv.includes('--office-corpus')) {
 for (const row of JSON.parse(readFileSync(root+'/fixtures/office-authored/manifest.json','utf8'))) {
  cases.push([row.path,readFileSync(root+'/'+row.path),{}]);
 }
}
if (process.argv.includes('--table-layout-corpus')) {
 for (const name of [
  'legacy/anydoc-main/tests/fixtures/rtf/handmade-bin.rtf',
  'legacy/anydoc-main/tests/fixtures/rtf/handmade-merge.rtf',
  'legacy/anydoc-main/tests/fixtures/rtf/text.rtf',
  'legacy/anydoc-main/tests/fixtures/xlsb/handmade-sheet.xlsb',
  'legacy/docling-main/tests/data/odf/sources/odf_presentation_01.odp',
  'legacy/docling-main/tests/data/odf/sources/text_document_03.odt',
  'legacy/docling-main/tests/data/pptx/sources/powerpoint_sample.pptx',
  'legacy/docling-main/tests/data/pptx/sources/powerpoint_with_image.pptx',
 ]) cases.push([name,readFileSync(root+'/'+name),{}]);
}
if (process.argv.includes('--legacy-layout-corpus')) {
 for (const name of [
  'legacy/anydoc-main/tests/fixtures/doc/text.doc',
  'legacy/docling-main/tests/data/doc/sources/greek_heading_styles.doc',
  'legacy/docling-main/tests/data/doc/sources/legacy_sample.doc',
  'legacy/anydoc-main/tests/fixtures/ppt/handmade-multimaster.ppt',
  'legacy/anydoc-main/tests/fixtures/ppt/pres.ppt',
  'legacy/docling-main/tests/data/ppt/sources/legacy_sample.ppt',
  'legacy/anydoc-main/tests/fixtures/rtf/handmade-cocoa.rtf',
  'legacy/anydoc-main/tests/fixtures/rtf/text.rtf',
 ]) {
  if (!cases.some(([filename]) => filename === name)) cases.push([name,readFileSync(root+'/'+name),{}]);
 }
}
for(const [filename,data,options] of cases){
 const n=await native(data,{filename,...options});
 const w=wasm(data,{filename,...options}); assert.deepEqual(w,n, 'WASM parity '+filename);
 const py=spawnSync('python3',['-c',"import sys,json,base64; from bulkparse import parse; x=json.load(sys.stdin); print(json.dumps(parse(base64.b64decode(x['data']),**x['options'])))"],{cwd:root,env:{...process.env,PYTHONPATH:root+'/bindings/python/python'},input:JSON.stringify({data:data.toString('base64'),options:{filename,...options}}),encoding:'utf8'});
 assert.equal(py.status,0,py.stderr); assert.deepEqual(JSON.parse(py.stdout),n,'Python parity '+filename);
 const format = filename.split('.').at(-1); const args=['parse','-','--format',format,'--to','json'];
 if(options.csv_delimiter)args.push('--csv-delimiter',options.csv_delimiter);
 if(options.csv_encoding)args.push('--csv-encoding','utf16-le');
 if(options.pdf_normalize_unicode !== undefined)args.push('--pdf-normalize-unicode='+options.pdf_normalize_unicode);
 const cli=spawnSync(process.env.BULKPARSE_CLI || root+'/target/release/bulkparse',args,{input:data,encoding:'utf8'}); assert.ok([0,2].includes(cli.status),cli.stderr);
 const {markdown,render_diagnostics,...parsed}=n; assert.deepEqual(JSON.parse(cli.stdout),parsed,'CLI parity '+filename);
}
console.log(`${cases.length} cases match across Node, Python, WASM and CLI, including UTF-16/delimiter overrides.`);

for (const name of ['native-and-empty.pdf', 'holdout/native-and-scan.pdf', 'inspection/header-over-scan.pdf']) {
 const data=readFileSync(root+'/fixtures/'+name);
 const n=await inspectNative(data);
 assert.deepEqual(inspectWasm(data),n,'inspection WASM parity '+name);
 const py=spawnSync('python3',['-c',"import sys,json; from bulkparse import inspect_pdf; print(json.dumps(inspect_pdf(sys.stdin.buffer.read())))"],{cwd:root,env:{...process.env,PYTHONPATH:root+'/bindings/python/python'},input:data,encoding:'utf8'});
 assert.equal(py.status,0,py.stderr); assert.deepEqual(JSON.parse(py.stdout),n,'inspection Python parity '+name);
 const cli=spawnSync(process.env.BULKPARSE_CLI || root+'/target/release/bulkparse',['inspect','-'],{input:data,encoding:'utf8'});
 assert.equal(cli.status,0,cli.stderr); assert.deepEqual(JSON.parse(cli.stdout),n,'inspection CLI parity '+name);
 assert.deepEqual((await native(data,{filename:name})).pdf_inspection,n,'inspection/conversion parity '+name);
}
console.log('3 PDF inspection cases match across Node, Python, WASM, CLI and conversion.');
