// Node can instantiate the web-target WASM using explicit bytes, without fetch.
import {readFile} from 'node:fs/promises';
import assert from 'node:assert/strict';
import {init, parse, capabilities, inspectPdf} from '../index.js';
await init({module_or_path: await readFile(new URL('../pkg/bulkparse_wasm_bg.wasm', import.meta.url))});
const result = parse(new TextEncoder().encode('# Hello BulkParse'), {filename: 'x.md'});
assert.equal(result.document.blocks[0].type, 'heading');
assert.match(result.markdown, /# Hello BulkParse/);
assert.equal(result.status, 'complete');
assert.equal(capabilities().length, 19);
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

const {parseWithOcr} = await import('../index.js');
const ocrEvidence = {page_count: 2, pages: [1, 2].map(page => ({page, output: {type: 'text', value: `Frame ${page}`}}))};
const imageBytes = new Uint8Array([73, 73, 42, 0]);
const ocrResult = parseWithOcr(imageBytes, ocrEvidence);
assert.equal(ocrResult.document.format, 'image');
assert.equal(ocrResult.coverage.length, 2);
assert.match(ocrResult.markdown, /Frame 1[\s\S]*Frame 2/);
assert.equal(parseWithOcr(imageBytes, ocrEvidence, {}, {max_pages: 1}).coverage[1].status, 'partial');
assert.throws(() => parseWithOcr(imageBytes, ocrEvidence, {}, {max_output_bytes: 1}), {code: 'resource_limit'});
assert.throws(() => parseWithOcr(imageBytes, {...ocrEvidence, pages: [ocrEvidence.pages[0], ocrEvidence.pages[0]]}), {code: 'malformed'});

const mergedEvidence = JSON.parse(await readFile(new URL('../../../fixtures/ocr/evidence/merged-table.json', import.meta.url), 'utf8'));
const mergedResult = parseWithOcr(imageBytes, mergedEvidence);
assert.equal(mergedResult.document.blocks[0].rows[0][0].col_span, 2);
assert.match(mergedResult.markdown, /125/);
mergedEvidence.pages[0].output.value.regions[0].content.rows[0][0].row_span = 9;
assert.throws(() => parseWithOcr(imageBytes, mergedEvidence), {code: 'malformed'});
