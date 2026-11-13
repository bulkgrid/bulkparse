// Node can instantiate the web-target WASM using explicit bytes, without fetch.
import {readFile} from 'node:fs/promises';
import assert from 'node:assert/strict';
import {init, parse, capabilities, inspectPdf} from '../index.js';
await init({module_or_path: await readFile(new URL('../pkg/bulkparse_wasm_bg.wasm', import.meta.url))});
const result = parse(new TextEncoder().encode('# Hello BulkParse'), {filename: 'x.md'});
assert.equal(result.document.blocks[0].type, 'heading');
assert.match(result.markdown, /# Hello BulkParse/);
assert.equal(result.status, 'complete');
assert.equal(capabilities().length, 18);
assert.throws(() => parse(new Uint8Array([1, 2]), {filename: 'x.txt', limits: {max_input_bytes: 1}}), {code: 'resource_limit'});
for (const name of ['hello.docx', 'native-and-empty.pdf', 'notebook.ipynb']) {
  const converted = parse(await readFile(new URL(`../../../fixtures/${name}`, import.meta.url)), {filename: name});
  assert.match(converted.markdown, /Hello BulkParse/);
  if (name.endsWith('.pdf')) assert.equal(converted.status, 'partial');
}
console.log('WASM smoke tests passed');


const csv = parse(Buffer.from('name;value\n名称;😀\n', 'utf16le'), {filename: 'x.csv', csv_encoding: 'utf16_le', csv_delimiter: ';'});
assert.equal(csv.document.blocks[0].rows[0].length, 2);
assert.match(csv.markdown, /😀/);
assert.throws(() => parse(new TextEncoder().encode('a,b;c\nx,y;z'), {filename: 'x.csv'}), {code: 'malformed'});

const scan = await readFile(new URL('../../../fixtures/inspection/header-over-scan.pdf', import.meta.url));
const inspected = inspectPdf(scan);
assert.deepEqual(inspected.pages_needing_ocr, [1]);
assert.equal(inspected.pages[0].ocr_status, 'not_requested');
assert.deepEqual(parse(scan, {filename:'x.pdf'}).pdf_inspection, inspected);
assert.throws(() => inspectPdf(scan, {limits:{max_nodes:1}}), {code:'resource_limit'});
console.log('WASM PDF inspection tests passed');

const imageResult = parse(new TextEncoder().encode('{\\rtf1 Before {\\pict\\pngblip 89504e470d0a1a0a} After}'), {filename: 'image.rtf'});
assert.equal(imageResult.document.assets.length, 1);
assert.equal(Buffer.from(imageResult.document.assets[0].data, 'base64').toString('hex'), '89504e470d0a1a0a');
assert.ok(imageResult.document.blocks[0].inlines.some(i => i.type === 'image' && i.url === `asset:${imageResult.document.assets[0].id}`));
console.log('WASM embedded image tests passed');
