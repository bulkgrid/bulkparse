import init, { parseJson, inspectPdfJson, capabilitiesJson } from './pkg/bulkparse_wasm.js';
export { init };

export function parse(bytes, options = {}) { return run(bytes, options, false); }
export function inspectPdf(bytes, options = {}) { return run(bytes, options, true); }
function run(bytes, options, inspect) {
  if (!(bytes instanceof Uint8Array)) throw new TypeError('bytes must be a Uint8Array');
  const { filename, ...config } = options;
  const cap = config.limits?.max_input_bytes ?? 33554432;
  if (!Number.isSafeInteger(cap) || cap < 0) throw new TypeError('max_input_bytes must be a nonnegative safe integer');
  if (bytes.byteLength > cap) {
    const error = new Error('resource limit exceeded: max_input_bytes');
    error.code = 'resource_limit';
    throw error;
  }
  try { return JSON.parse(inspect ? inspectPdfJson(bytes, JSON.stringify(config)) : parseJson(bytes, filename, JSON.stringify(config))); }
  catch (failure) {
    let detail;
    try { detail = JSON.parse(String(failure)); } catch { throw failure; }
    const error = new Error(detail.message);
    error.name = 'BulkParseError';
    error.code = detail.code;
    throw error;
  }
}
export const capabilities = () => JSON.parse(capabilitiesJson());
