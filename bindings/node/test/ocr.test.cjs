const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const os = require('node:os');
const path = require('node:path');
const {parseWithOcr, OcrParser} = require('..');
const bytes = Buffer.from('49492a00', 'hex');
const evidence = {page_count: 2, pages: [1, 2].map(page => ({page, output: {type: 'text', value: `Frame ${page}`}}))};

test('portable OCR evidence retains frame provenance and enforces budgets', async () => {
  const result = await parseWithOcr(bytes, evidence);
  assert.equal(result.document.format, 'image');
  assert.equal(result.coverage.length, 2);
  assert.match(result.markdown, /Frame 1[\s\S]*Frame 2/);
  const limited = await parseWithOcr(bytes, evidence, {}, {max_pages: 1});
  assert.equal(limited.status, 'partial');
  assert.equal(limited.coverage[1].status, 'partial');
  await assert.rejects(parseWithOcr(bytes, evidence, {}, {max_output_bytes: 1}), {code: 'resource_limit'});
  await assert.rejects(parseWithOcr(bytes, {...evidence, pages: [evidence.pages[0], evidence.pages[0]]}), {code: 'malformed'});
  await assert.rejects(parseWithOcr(bytes, evidence, {strict: true}, {max_pages: 1}), {code: 'incomplete'});
});

test('Rust OCR parser has no executable configuration and closes explicitly', async () => {
  assert.throws(() => new OcrParser({executable: 'python3'}), {code: 'malformed'});
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'bulkparse-ocr-'));
  const models = path.join(dir, 'models.json');
  await fs.writeFile(models, '{}');
  // Native documents do not initialize inference or require a loaded runtime.
  const parser = new OcrParser({models, onnxruntime_library: models});
  try {
    const first = await parser.parse(Buffer.from('Reusable Rust parser'), {filename: 'input.txt'});
    assert.match(first.markdown, /Reusable Rust parser/);
    const input = path.join(dir, 'input.txt');
    await fs.writeFile(input, 'Reusable Rust parser');
    assert.equal((await parser.parseFile(input)).markdown, first.markdown);
    parser.close();
    await assert.rejects(parser.parse(Buffer.from('closed')), {code: 'malformed', message: /closed/});
  } finally { parser.close(); await fs.rm(dir, {recursive: true, force: true}); }
});

test('merged semantic table evidence survives the Node transport', async () => {
  const evidence = JSON.parse(await fs.readFile(path.resolve(__dirname, '../../../fixtures/ocr/evidence/merged-table.json'), 'utf8'));
  const result = await parseWithOcr(bytes, evidence);
  assert.equal(result.document.blocks[0].rows[0][0].col_span, 2);
  assert.match(result.markdown, /Sales report/);
  assert.match(result.markdown, /125/);
  evidence.pages[0].output.value.regions[0].content.rows[0][0].row_span = 9;
  await assert.rejects(parseWithOcr(bytes, evidence), {code: 'malformed'});
});
