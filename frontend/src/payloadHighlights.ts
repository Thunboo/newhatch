import { decodePayload, findHttpBodyBoundary, looksLikeJson, MAX_EMBEDDED_JSON_DEPTH } from "./payloadDisplay";
import type { ByteRange } from "./types";

export type HighlightRange = ByteRange & { kind: "flag" | "payload" | "both" };
type MarkedText = { text: string; marks: Uint8Array };
type JsonString = { kind: "string"; value: MarkedText; quotes: [number, number] };
type JsonEntry = { key: JsonString; value: JsonNode; colon: number; comma: number };
type JsonNode = JsonString
  | { kind: "literal"; value: MarkedText }
  | { kind: "object"; entries: JsonEntry[]; edges: [number, number] }
  | { kind: "array"; entries: { value: JsonNode; comma: number }[]; edges: [number, number] };
const FLAG = 1;
const PAYLOAD = 2;
const encoder = new TextEncoder();

function byteString(bytes: Uint8Array): string {
  const chunks: string[] = [];
  for (let offset = 0; offset < bytes.length; offset += 8_192) {
    chunks.push(String.fromCharCode(...bytes.subarray(offset, offset + 8_192)));
  }
  return chunks.join("");
}

function markBytes(marks: Uint8Array, start: number, end: number, kind: number) {
  for (let index = start; index < end; index++) marks[index] |= kind;
}

function union(marks: Uint8Array, start = 0, end = marks.length): number {
  let value = 0;
  for (let index = start; index < end; index++) value |= marks[index];
  return value;
}

function literal(text: string, mark = 0): MarkedText {
  return { text, marks: new Uint8Array(text.length).fill(mark) };
}

function slice(value: MarkedText, start: number, end = value.text.length): MarkedText {
  return { text: value.text.slice(start, end), marks: value.marks.subarray(start, end) };
}

function join(values: MarkedText[]): MarkedText {
  const text = values.map((value) => value.text).join("");
  const marks = new Uint8Array(text.length);
  let offset = 0;
  for (const value of values) { marks.set(value.marks, offset); offset += value.text.length; }
  return { text, marks };
}

// Follow UTF-8 decoder boundaries, including replacement characters. A byte
// match inside an escape/code point marks its displayed character as a whole.
function decodeMarked(bytes: Uint8Array, marks: Uint8Array, text = decodePayload(bytes), stripBom = true): MarkedText {
  const decoded = new Uint8Array(text.length);
  let character = 0;
  for (let offset = 0; offset < bytes.length;) {
    if (stripBom && offset === 0 && bytes[0] === 0xef && bytes[1] === 0xbb && bytes[2] === 0xbf) { offset += 3; continue; }
    const first = bytes[offset];
    const size = first < 0x80 ? 1 : first >= 0xc2 && first <= 0xdf ? 2 : first >= 0xe0 && first <= 0xef ? 3 : first >= 0xf0 && first <= 0xf4 ? 4 : 1;
    let consumed = 1;
    while (consumed < size) {
      const byte = bytes[offset + consumed];
      let lower = 0x80;
      let upper = 0xbf;
      if (consumed === 1) {
        if (first === 0xe0) lower = 0xa0;
        if (first === 0xed) upper = 0x9f;
        if (first === 0xf0) lower = 0x90;
        if (first === 0xf4) upper = 0x8f;
      }
      if (byte === undefined || byte < lower || byte > upper) break;
      consumed++;
    }
    const mark = union(marks, offset, offset + consumed);
    decoded[character++] = mark;
    if (size === 4 && consumed === size) decoded[character++] = mark;
    offset += consumed;
  }
  return { text, marks: decoded };
}

function decodeEscapes(value: MarkedText): MarkedText {
  const parts: MarkedText[] = [];
  const simple: Record<string, string> = { '"': '"', "\\": "\\", "/": "/", b: "\b", f: "\f", n: "\n", r: "\r", t: "\t" };
  let untouched = 0;
  for (let index = 0; index < value.text.length; index++) {
    if (value.text[index] !== "\\") continue;
    const escape = value.text[index + 1];
    let replacement = simple[escape];
    let length = 2;
    if (escape === "u" && /^[0-9a-fA-F]{4}$/.test(value.text.slice(index + 2, index + 6))) {
      const first = Number.parseInt(value.text.slice(index + 2, index + 6), 16);
      replacement = String.fromCharCode(first);
      length = 6;
      const second = value.text.slice(index + 8, index + 12);
      if (first >= 0xd800 && first <= 0xdbff && value.text.slice(index + 6, index + 8) === "\\u" && /^[0-9a-fA-F]{4}$/.test(second)) {
        const low = Number.parseInt(second, 16);
        if (low >= 0xdc00 && low <= 0xdfff) { replacement += String.fromCharCode(low); length = 12; }
      }
    }
    if (replacement === undefined) continue;
    parts.push(slice(value, untouched, index), literal(replacement, union(value.marks.subarray(index, index + length))));
    index += length - 1;
    untouched = index + 1;
  }
  parts.push(slice(value, untouched));
  return join(parts);
}

function decodePercent(value: MarkedText): MarkedText {
  const parts: MarkedText[] = [];
  let untouched = 0;
  for (const match of value.text.matchAll(/(?:%[0-9a-fA-F]{2})+/g)) {
    const start = match.index;
    const end = start + match[0].length;
    try {
      const text = decodeURIComponent(match[0]);
      const bytes = new Uint8Array(match[0].length / 3);
      const marks = new Uint8Array(bytes.length);
      for (let index = 0; index < bytes.length; index++) {
        bytes[index] = Number.parseInt(match[0].slice(index * 3 + 1, index * 3 + 3), 16);
        marks[index] = union(value.marks.subarray(start + index * 3, start + index * 3 + 3));
      }
      parts.push(slice(value, untouched, start), decodeMarked(bytes, marks, text, false));
      untouched = end;
    } catch { /* Invalid percent-encoded UTF-8 remains literal, as in displayPayload. */ }
  }
  parts.push(slice(value, untouched));
  return join(parts);
}

// JSON.parse validates the input; this small reader retains marks on its tokens
// so formatting/embedded JSON expansion cannot highlight unrelated equal text.
function parseJson(source: MarkedText): JsonNode {
  JSON.parse(source.text);
  let index = 0;
  const skip = () => { while (" \t\r\n".includes(source.text[index]) && index < source.text.length) index++; };
  const readString = (): JsonString => {
    const start = index++;
    while (source.text[index] !== '"') { index += source.text[index] === "\\" ? 2 : 1; }
    const end = index++;
    return { kind: "string", value: decodeEscapes(slice(source, start + 1, end)), quotes: [source.marks[start], source.marks[end]] };
  };
  const read = (): JsonNode => {
    skip();
    if (source.text[index] === '"') return readString();
    if (source.text[index] === "{") {
      const open = source.marks[index++];
      const entries: Record<string, JsonEntry> = Object.create(null);
      skip();
      while (source.text[index] !== "}") {
        const key = readString();
        skip();
        const colon = source.marks[index++];
        const value = read();
        skip();
        const comma = source.text[index] === "," ? source.marks[index++] : 0;
        entries[key.value.text] = { key, value, colon, comma };
        skip();
      }
      return { kind: "object", entries: Object.values(entries), edges: [open, source.marks[index++]] };
    }
    if (source.text[index] === "[") {
      const open = source.marks[index++];
      const entries: { value: JsonNode; comma: number }[] = [];
      skip();
      while (source.text[index] !== "]") {
        const value = read();
        skip();
        const comma = source.text[index] === "," ? source.marks[index++] : 0;
        entries.push({ value, comma });
      }
      return { kind: "array", entries, edges: [open, source.marks[index++]] };
    }
    const start = index;
    while (index < source.text.length && !" \t\r\n,]}".includes(source.text[index])) index++;
    const value = slice(source, start, index);
    return { kind: "literal", value: literal(JSON.stringify(JSON.parse(value.text)), union(value.marks)) };
  };
  return read();
}

function stringifyString(value: JsonString): MarkedText {
  const parts: MarkedText[] = [literal('"', value.quotes[0])];
  let untouched = 0;
  const text = value.value.text;
  for (let index = 0; index < text.length; index++) {
    const code = text.charCodeAt(index);
    const pairedHigh = code >= 0xd800 && code <= 0xdbff && text.charCodeAt(index + 1) >= 0xdc00 && text.charCodeAt(index + 1) <= 0xdfff;
    const pairedLow = code >= 0xdc00 && code <= 0xdfff && text.charCodeAt(index - 1) >= 0xd800 && text.charCodeAt(index - 1) <= 0xdbff;
    if (code >= 32 && text[index] !== '"' && text[index] !== "\\" && (code < 0xd800 || code > 0xdfff || pairedHigh || pairedLow)) continue;
    parts.push(slice(value.value, untouched, index), literal(JSON.stringify(text[index]).slice(1, -1), value.value.marks[index]));
    untouched = index + 1;
  }
  parts.push(slice(value.value, untouched), literal('"', value.quotes[1]));
  return join(parts);
}

function stringifyJson(value: JsonNode, pretty: boolean, depth = 0): MarkedText {
  if (value.kind === "string") return stringifyString(value);
  if (value.kind === "literal") return value.value;
  const object = value.kind === "object";
  const parts = [literal(object ? "{" : "[", value.edges[0])];
  for (let index = 0; index < value.entries.length; index++) {
    const entry = value.entries[index];
    const rendered = stringifyJson(entry.value, pretty, depth + 1);
    const fields = "key" in entry ? [stringifyString(entry.key), literal(":", entry.colon), literal(pretty ? " " : "", entry.colon & rendered.marks[0]), rendered] : [rendered];
    if (pretty) {
      const previous = parts[parts.length - 1].marks;
      parts.push(literal("\n" + "  ".repeat(depth + 1), previous[previous.length - 1] & fields[0].marks[0]));
    }
    parts.push(...fields);
    if (index < value.entries.length - 1) parts.push(literal(",", entry.comma));
  }
  if (pretty && value.entries.length) {
    const previous = parts[parts.length - 1].marks;
    parts.push(literal("\n" + "  ".repeat(depth), previous[previous.length - 1] & value.edges[1]));
  }
  parts.push(literal(object ? "}" : "]", value.edges[1]));
  return join(parts);
}

function normalizeJson(value: JsonNode, expand: boolean, depth = 0): JsonNode {
  if (value.kind === "string") {
    const decoded = decodePercent(value.value);
    if (depth < MAX_EMBEDDED_JSON_DEPTH && looksLikeJson(decoded.text)) {
      try {
        const nested = normalizeJson(parseJson(decoded), expand, depth + 1);
        return expand ? nested : { ...value, value: stringifyJson(nested, false) };
      } catch { /* Malformed embedded JSON remains a string. */ }
    }
    return { ...value, value: decoded };
  }
  if (value.kind === "literal") return value;
  return { ...value, entries: value.entries.map((entry) => ({ ...entry, value: normalizeJson(entry.value, expand, depth) })) } as JsonNode;
}

function formatMarked(value: MarkedText, pretty: boolean): MarkedText | null {
  try { return stringifyJson(normalizeJson(parseJson(value), pretty), pretty); }
  catch { return null; }
}

export function payloadHighlights(bytes: Uint8Array, terms: readonly string[], flags: ByteRange[], pretty: boolean): { text: string; ranges: HighlightRange[] } {
  const marks = new Uint8Array(bytes.length);
  for (const range of flags) markBytes(marks, range.start, range.end, FLAG);
  const rawBytes = byteString(bytes);
  for (const term of new Set(terms)) {
    if (!term) continue;
    const needle = byteString(encoder.encode(term));
    for (let start = rawBytes.indexOf(needle); start >= 0; start = rawBytes.indexOf(needle, start + 1)) {
      markBytes(marks, start, start + needle.length, PAYLOAD);
    }
  }
  const raw = decodeMarked(bytes, marks);
  const boundary = findHttpBodyBoundary(raw.text);
  let displayed: MarkedText;
  if (boundary) {
    const body = formatMarked(slice(raw, boundary.bodyStart), pretty);
    displayed = body ? join([decodePercent(decodeEscapes(slice(raw, 0, boundary.headerEnd))), slice(raw, boundary.headerEnd, boundary.bodyStart), body]) : decodePercent(decodeEscapes(raw));
  } else displayed = formatMarked(raw, pretty) ?? decodePercent(decodeEscapes(raw));
  const ranges: HighlightRange[] = [];
  for (let start = 0; start < displayed.text.length;) {
    const kind = displayed.marks[start];
    let end = start + 1;
    while (end < displayed.text.length && displayed.marks[end] === kind) end++;
    if (kind) ranges.push({ start, end, kind: kind === FLAG ? "flag" : kind === PAYLOAD ? "payload" : "both" });
    start = end;
  }
  return { text: displayed.text, ranges };
}
