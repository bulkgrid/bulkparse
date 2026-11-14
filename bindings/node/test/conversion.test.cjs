const test = require('node:test');
const assert = require('node:assert/strict');
const path = require('node:path');
const { parse, parseFile, capabilities } = require('..');

test('native conversion and file API share the contract', async () => {
  const result = await parse(Buffer.from('# Hello BulkParse'), {filename: 'x.md'});
  assert.equal(result.document.blocks[0].type, 'heading');
  assert.equal(result.status, 'complete');
  assert.match(result.markdown, /# Hello BulkParse/);
  const file = await parseFile(path.resolve(__dirname, '../../../fixtures/hello.docx'));
  assert.equal(file.document.format, 'docx');
  assert.match(file.markdown, /# Hello BulkParse/);
  assert.equal(capabilities().length, 19);
});

test('errors and resource limits cross the binding', async () => {
  await assert.rejects(parse(Buffer.from('abc'), {filename: 'x.txt', limits: {max_input_bytes: 2}}), {code: 'resource_limit'});
  await assert.rejects(parse(Buffer.from('abc')), {code: 'unsupported'});
  await assert.rejects(parse(Buffer.from('abc'), {filename: 'x.txt', strcit: true}), {code: 'malformed'});
});


 test('CSV encoding and dialect options cross the binding', async () => {
  const bytes = Buffer.from('name;value\n名称;😀\n', 'utf16le');
  const result = await parse(bytes, {filename: 'x.csv', csv_encoding: 'utf16_le', csv_delimiter: ';'});
  assert.equal(result.document.blocks[0].rows[0].length, 2);
  assert.match(result.markdown, /😀/);
  await assert.rejects(parse(Buffer.from('a,b;c\nx,y;z'), {filename: 'x.csv'}), {code: 'malformed'});
});

test('PDF inspection exposes routing without OCR and obeys limits', async () => {
  const {inspectPdf} = require('..');
  const bytes = require('node:fs').readFileSync(path.resolve(__dirname, '../../../fixtures/inspection/header-over-scan.pdf'));
  const report = await inspectPdf(bytes);
  assert.deepEqual(report.pages_needing_ocr, [1]);
  assert.equal(report.pages[0].ocr_status, 'not_requested');
  assert.deepEqual((await parse(bytes, {filename:'x.pdf'})).pdf_inspection, report);
  await assert.rejects(inspectPdf(bytes, {limits:{max_input_bytes:1}}), {code:'resource_limit'});
  await assert.rejects(inspectPdf(bytes, {limits:{max_nodes:1}}), {code:'resource_limit'});
});

test('bounded partial recovery is explicit and strict mode still rejects it', async () => {
  const bytes = Buffer.from('name,value\nalpha,one\nbeta,two\n');
  const options = {filename: 'large.csv', limits: {max_table_cells: 5}};
  await assert.rejects(parse(bytes, options), {code: 'resource_limit'});
  const result = await parse(bytes, {...options, partial_on_limit: true});
  assert.equal(result.status, 'partial');
  assert.equal(result.document.blocks[0].rows.length, 2);
  assert.ok(result.diagnostics.some(d => d.code === 'resource_limit_partial' && d.content_loss));
  assert.match(result.markdown, /alpha/);
  assert.doesNotMatch(result.markdown, /beta/);
  await assert.rejects(parse(bytes, {...options, partial_on_limit: true, strict: true}), {code: 'resource_limit'});
});

test('streaming file output matches conversion and preserves destinations on failure', async () => {
  const fs = require('node:fs/promises');
  const os = require('node:os');
  const {parseFileToMarkdown} = require('..');
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'bulkparse-stream-'));
  try {
    const input = path.join(dir, 'input.csv'), output = path.join(dir, 'output.md');
    await fs.writeFile(input, 'name,value\nalpha,one\nbeta,two\n');
    const options = {partial_on_limit: true, limits: {max_table_cells: 5}};
    const expected = await parseFile(input, options);
    const summary = await parseFileToMarkdown(input, output, options);
    assert.equal(await fs.readFile(output, 'utf8'), expected.markdown);
    assert.equal(summary.status, expected.status);
    assert.deepEqual(summary.diagnostics, expected.diagnostics);
    assert.deepEqual(summary.render_diagnostics, expected.render_diagnostics);
    assert.equal(summary.bytes_written, Buffer.byteLength(expected.markdown));
    assert.equal('document' in summary, false);
    await assert.rejects(parseFileToMarkdown(input, output, options), {code: 'io'});
    assert.equal(await fs.readFile(output, 'utf8'), expected.markdown);
    const failed = path.join(dir, 'failed.md');
    await assert.rejects(parseFileToMarkdown(input, failed, {...options, strict: true}), {code: 'resource_limit'});
    await assert.rejects(fs.stat(failed), {code: 'ENOENT'});
    await assert.rejects(parseFileToMarkdown(input, failed, {strcit: true}), {code: 'malformed'});
    await assert.rejects(parseFileToMarkdown(input, failed, {limits: {max_input_bytes: 1}}), {code: 'resource_limit'});
  } finally { await fs.rm(dir, {recursive: true, force: true}); }
});

test('embedded images are returned as base64 and saved without changing Markdown', async () => {
  const fs = require('node:fs/promises');
  const os = require('node:os');
  const {parseFileToMarkdown} = require('..');
  const bytes = Buffer.from('{\\rtf1 Text {\\pict\\pngblip 89504e470d0a1a0a}}');
  const result = await parse(bytes, {filename: 'image.rtf'});
  assert.equal(result.document.assets.length, 1);
  const original = Buffer.from('89504e470d0a1a0a', 'hex');
  assert.deepEqual(Buffer.from(result.document.assets[0].data, 'base64'), original);
  assert.equal((await parse(bytes, {filename: 'image.rtf', extract_images: false})).document.assets.length, 0);
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'bulkparse-images-test-'));
  try {
    const input = path.join(dir, 'input.rtf'), output = path.join(dir, 'output.md');
    await fs.writeFile(input, bytes);
    const summary = await parseFileToMarkdown(input, output, {export_images: true});
    assert.equal(summary.images.length, 1);
    assert.deepEqual(await fs.readFile(path.join(dir, summary.images[0].path)), original);
    assert.equal(await fs.readFile(output, 'utf8'), (await parseFile(input)).markdown);
    assert.ok(!(await fs.readFile(output, 'utf8')).includes(summary.images[0].path));
  } finally { await fs.rm(dir, {recursive: true, force: true}); }
});
