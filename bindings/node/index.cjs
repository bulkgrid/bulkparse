'use strict';
const fs = require('node:fs');
const path = require('node:path');
const localAddon = path.join(__dirname, 'bulkparse.node');
const native = require(fs.existsSync(localAddon) ? localAddon : require('./platform.cjs').nativePath('bulkparse.node'));

function normalizeError(error) {
  try {
    const detail = JSON.parse(error.message);
    const result = new Error(detail.message);
    result.name = 'BulkParseError';
    result.code = detail.code;
    return result;
  } catch { return error; }
}

async function parse(bytes, options = {}) { return run(bytes, options, false); }
async function inspectPdf(bytes, options = {}) { return run(bytes, options, true); }
async function run(bytes, options, inspect) {
  if (!(bytes instanceof Uint8Array)) throw new TypeError('bytes must be a Buffer or Uint8Array');
  const { filename, ...config } = options;
  const cap = config.limits?.max_input_bytes ?? 33554432;
  if (!Number.isSafeInteger(cap) || cap < 0) throw new TypeError('max_input_bytes must be a nonnegative safe integer');
  if (bytes.byteLength > cap) {
    const error = new Error('resource limit exceeded: max_input_bytes');
    error.code = 'resource_limit';
    throw error;
  }
  try {
    const data = Buffer.from(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    return JSON.parse(await (inspect ? native.inspectPdfJson(data, JSON.stringify(config)) : native.parseJson(data, filename, JSON.stringify(config))));
  } catch (error) { throw normalizeError(error); }
}

async function parseFile(file, options = {}) {
  const cap = options.limits?.max_input_bytes ?? 33554432;
  if (!Number.isSafeInteger(cap) || cap < 0) throw new TypeError('max_input_bytes must be a nonnegative safe integer');
  const chunks = [];
  for await (const chunk of fs.createReadStream(file, {start: 0, end: cap})) chunks.push(chunk);
  return parse(Buffer.concat(chunks), { ...options, filename: options.filename ?? path.basename(file) });
}

async function parseFileToMarkdown(input, output, options = {}) {
  if (typeof input !== 'string' || typeof output !== 'string') throw new TypeError('input and output must be file paths');
  if (options.filename !== undefined) throw new TypeError('parseFileToMarkdown uses the input path; use format to override format detection');
  const cap = options.limits?.max_input_bytes ?? 33554432;
  if (!Number.isSafeInteger(cap) || cap < 0) throw new TypeError('max_input_bytes must be a nonnegative safe integer');
  try { return JSON.parse(await native.parseFileToMarkdownJson(input, output, JSON.stringify(options))); }
  catch (error) { throw normalizeError(error); }
}

module.exports = { parse, parseFile, parseFileToMarkdown, inspectPdf, capabilities: () => JSON.parse(native.capabilitiesJson()) };

function ocrInput(bytes, options) {
  if (!(bytes instanceof Uint8Array)) throw new TypeError('bytes must be a Buffer or Uint8Array');
  const cap = options.limits?.max_input_bytes ?? 33554432;
  if (!Number.isSafeInteger(cap) || cap < 0) throw new TypeError('max_input_bytes must be a nonnegative safe integer');
  if (bytes.byteLength > cap) {
    const error = new Error('resource limit exceeded: max_input_bytes');
    error.code = 'resource_limit';
    throw error;
  }
  return Buffer.from(bytes.buffer, bytes.byteOffset, bytes.byteLength);
}
async function parseWithOcr(bytes, evidence, options = {}, budget = {}) {
  const data = ocrInput(bytes, options);
  const {filename, ...config} = options;
  try { return JSON.parse(await native.parseWithOcrJson(data, JSON.stringify(evidence), filename, JSON.stringify(config), JSON.stringify(budget))); }
  catch (error) { throw normalizeError(error); }
}
class OcrParser {
  #native;
  constructor(config) {
    try { this.#native = new native.NativeOcrParser(JSON.stringify(config)); }
    catch (error) { throw normalizeError(error); }
  }
  async parse(bytes, options = {}, budget = {}) {
    const data = ocrInput(bytes, options);
    const {filename, ...config} = options;
    try { return JSON.parse(await this.#native.parseJson(data, filename, JSON.stringify(config), JSON.stringify(budget))); }
    catch (error) { throw normalizeError(error); }
  }
  async parseFile(file, options = {}, budget = {}) {
    const cap = options.limits?.max_input_bytes ?? 33554432;
    if (!Number.isSafeInteger(cap) || cap < 0) throw new TypeError('max_input_bytes must be a nonnegative safe integer');
    const chunks = [];
    for await (const chunk of fs.createReadStream(file, {start: 0, end: cap})) chunks.push(chunk);
    return this.parse(Buffer.concat(chunks), {...options, filename: options.filename ?? path.basename(file)}, budget);
  }
  close() { this.#native.close(); }
}
module.exports.parseWithOcr = parseWithOcr;
module.exports.OcrParser = OcrParser;
