// Run with Node from the repository root after npm ci in frontend/.
const fs = require('node:fs');
const assert = require('node:assert/strict');
const ts = require('../../frontend/node_modules/typescript');
require.extensions['.ts'] = (module, filename) => module._compile(ts.transpileModule(
  fs.readFileSync(filename, 'utf8'), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } },
).outputText, filename);
const { displayPayload } = require('../../frontend/src/payloadDisplay.ts');
const { payloadHighlights } = require('../../frontend/src/payloadHighlights.ts');
for (const example of require('./cases.json')) {
  for (const [pretty, key] of [[false, 'compact'], [true, 'pretty']]) {
    const raw = Buffer.from(example.bytes ?? example.raw);
    assert.equal(displayPayload(raw, pretty), example[key], example.name);
    assert.equal(payloadHighlights(raw, [], [], pretty).text, example[key], `${example.name} highlights`);
  }
}
const raw = Buffer.from('HTTP/1.1 200 OK\r\n\r\n{"text":"%D0%90 %46%4C%41%47%7Btest%7D","nested":"{\\"x\\":\\"\\\\u0410\\"}"}');
for (const pretty of [false, true]) {
  const result = payloadHighlights(raw, ['А', 'test'], [{ start: 0, end: 1, value: 'FLAG{test}' }], pretty);
  assert.equal(result.text, displayPayload(raw, pretty));
  assert.equal(result.ranges.filter((r) => r.kind === 'both').map((r) => result.text.slice(r.start, r.end)).join(''), 'test');
  assert.equal(result.ranges.filter((r) => r.kind === 'payload').length, 2);
}
const percent = Buffer.from('{"x":"%2541"}');
assert.equal(payloadHighlights(percent, ['%41'], [], true).text, displayPayload(percent, true), 'percent decoding is one pass');
const { parseReplayRequest, parseReplayRequests, exportCurl, exportPython } = require('../../frontend/src/requestExport.ts');
const body = JSON.stringify({ text: 'Привет', nested: String.raw`{"name":"\u0410"}`, encoded: '%D0%90' });
const wire = Buffer.from(`POST /api?q=%D0%90 HTTP/1.1\r\nHost: test\r\nContent-Type: application/json\r\nContent-Length: ${Buffer.byteLength(body)}\r\n\r\n${body}`);
for (const pretty of [false, true]) {
  const visible = displayPayload(wire, pretty);
  const parsed = parseReplayRequest(wire, visible, { host: '127.0.0.1', port: 8080 });
  assert.equal(parsed.ok, true);
  assert.equal(parsed.request.body, visible.split('\r\n\r\n')[1]);
  assert.equal(parsed.request.headers.some(([name]) => name.toLowerCase() === 'content-length'), false);
  assert.ok(Buffer.byteLength(parsed.request.body) > parsed.request.body.length);
  assert.ok(exportCurl(parsed.request).includes('Привет'));
  assert.ok(exportPython(parsed.request).includes('Привет'));
  const chain = parseReplayRequests(Buffer.concat([wire, wire]), pretty, { host: '127.0.0.1', port: 8080 });
  assert.equal(chain.ok, true); assert.equal(chain.requests.length, 2);
  assert.equal(chain.requests[1].body, parsed.request.body);
}
console.log('Shared canonical text, highlighting and individual/chain export fixtures passed.');
