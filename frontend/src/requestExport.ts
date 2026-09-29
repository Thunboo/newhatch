import { decodeDisplayEscapes } from "./payloadDisplay";

export type ReplayTarget = { host: string; port: number };
export type ReplayProblem = "http" | "incomplete" | "multiple" | "encoding" | "headers";
export type ReplayRequest = {
  method: string;
  url: string;
  headers: [string, string][];
  params: [string, string][];
  body: string;
  hasBody: boolean;
};
export type ReplayResult = { ok: true; request: ReplayRequest } | { ok: false; problem: ReplayProblem };

const token = /^[!#$%&'*+.^_\x60|~0-9A-Za-z-]+$/;
const control = /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f]/;
const encoder = new TextEncoder();

function splitMessage(text: string) {
  const match = /\r?\n\r?\n/.exec(text);
  return match ? { head: text.slice(0, match.index), body: text.slice(match.index + match[0].length) } : null;
}

/**
 * Framing is checked against captured bytes; the exported body is deliberately
 * taken from the user's text view. This is a readable HTTP export, not raw replay.
 */
export function parseReplayRequest(bytes: Uint8Array, displayed: string, target: ReplayTarget): ReplayResult {
  const fail = (problem: ReplayProblem): ReplayResult => ({ ok: false, problem });
  let raw: string;
  try {
    raw = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  } catch {
    return fail("encoding");
  }
  const original = splitMessage(raw);
  const visible = splitMessage(displayed);
  if (!original || !visible) return fail("incomplete");
  const [line, ...headerLines] = original.head.split(/\r?\n/);
  const match = /^(\S+) (\/\S*) HTTP\/1\.[01]$/.exec(line);
  if (!match || !token.test(match[1])) return fail("http");

  const headers: [string, string][] = [];
  for (const line of headerLines) {
    const colon = line.indexOf(":");
    if (colon < 1 || !token.test(line.slice(0, colon))) return fail("headers");
    headers.push([line.slice(0, colon), line.slice(colon + 1).trim()]);
  }
  const values = (name: string) => headers.filter(([key]) => key.toLowerCase() === name).map(([, value]) => value);
  // Compressed/chunked bodies and Upgrade streams need a different representation.
  if (values("transfer-encoding").length || values("upgrade").length
    || values("content-encoding").some((value) => value.toLowerCase() !== "identity")) return fail("encoding");
  const lengths = values("content-length");
  if (lengths.length > 1 || (lengths.length && !/^\d+$/.test(lengths[0]))) return fail("headers");
  const expected = lengths.length ? Number(lengths[0]) : 0;
  if (!Number.isSafeInteger(expected)) return fail("headers");
  const actual = encoder.encode(original.body).length;
  if (actual < expected) return fail("incomplete");
  if (actual > expected) return fail("multiple");
  if (control.test(visible.body) || control.test(match[2])) return fail("encoding");
  if (match[1] === "HEAD" && expected > 0) return fail("http");

  // Display transforms can change byte lengths. Let the HTTP client frame the
  // new body, and remove connection-specific fields (including named fields).
  const omitted = new Set([
    "content-length", "transfer-encoding", "connection", "proxy-connection",
    "keep-alive", "te", "trailer", "upgrade", "expect", "accept-encoding",
  ]);
  for (const value of values("connection")) {
    value.split(",").forEach((name) => omitted.add(name.trim().toLowerCase()));
  }
  const seen = new Set<string>();
  const exportedHeaders: [string, string][] = [];
  for (const [name, value] of headers) {
    const key = name.toLowerCase();
    if (omitted.has(key)) continue;
    if (seen.has(key)) return fail("headers"); // requests uses a case-insensitive dictionary.
    seen.add(key);
    const decoded = decodeDisplayEscapes(value);
    if (/[\r\n]/.test(decoded) || control.test(decoded)) return fail("headers");
    exportedHeaders.push([name, decoded]);
  }

  const queryAt = match[2].indexOf("?");
  const path = queryAt < 0 ? match[2] : match[2].slice(0, queryAt);
  if (path.includes("#")) return fail("http");
  const params: [string, string][] = [];
  if (queryAt >= 0) {
    // Split before decoding so an escaped "&" stays inside its value.
    for (const part of match[2].slice(queryAt + 1).split("&")) {
      if (!part) continue;
      const equal = part.indexOf("=");
      const name = equal < 0 ? part : part.slice(0, equal);
      const value = equal < 0 ? "" : part.slice(equal + 1);
      const decode = (text: string) => decodeDisplayEscapes(text.replace(/\+/g, " "));
      params.push([decode(name), decode(value)]);
    }
  }
  if (params.some((pair) => pair.some((value) => control.test(value) || /[\r\n]/.test(value)))) return fail("encoding");
  const host = target.host.includes(":") ? "[" + target.host.replace(/^\[|\]$/g, "") + "]" : target.host;
  return {
    ok: true,
    request: {
      method: match[1],
      // Keep the captured path's URL escaping; bodies and query values are readable.
      url: "http://" + host + ":" + target.port + path,
      headers: exportedHeaders,
      params,
      body: visible.body,
      hasBody: lengths.length > 0 && match[1] !== "HEAD",
    },
  };
}

function bashString(value: string): string {
  return "'" + value.replace(/'/g, "'\"'\"'") + "'";
}

function pythonString(value: string): string {
  // JSON string quoting is also valid for these Python strings, and keeps Unicode.
  return JSON.stringify(value);
}

export function exportCurl(request: ReplayRequest): string {
  const args = [
    "curl --silent --show-error --max-time 10 --globoff --path-as-is",
    "--request " + bashString(request.method),
    "--url " + bashString(request.url),
  ];
  if (request.method === "HEAD") args.push("--head");
  for (const [name, value] of request.params) {
    // curl expects the parameter name to be URL-escaped. Always include "=" so
    // a captured @file cannot turn into curl's file-reading syntax.
    const argument = name
      ? encodeURIComponent(name) + "=" + value
      : "+=" + encodeURIComponent(value);
    args.push("--url-query " + bashString(argument));
  }
  for (const [name, value] of request.headers) {
    args.push("--header " + bashString(value ? name + ": " + value : name + ";"));
  }
  if (request.hasBody && !request.headers.some(([name]) => name.toLowerCase() === "content-type")) {
    args.push("--header 'Content-Type:'");
  }
  if (request.hasBody) args.push("--data-binary @-");
  const command = args.join(" \\\n  ");
  // Bash's builtin printf preserves trailing newlines, literal @ and shell
  // metacharacters, and avoids the OS argument-size limit for a large body.
  return (request.hasBody ? "printf '%s' " + bashString(request.body) + " | \\\n" : "") + command + "\n";
}

export function exportPython(request: ReplayRequest): string {
  const lines = [
    "import requests",
    "",
    "url = " + pythonString(request.url),
    "headers = {",
    ...request.headers.map(([name, value]) => "    " + pythonString(name) + ": " + pythonString(value)
      + (/[^\x00-\x7f]/.test(value) ? '.encode("utf-8")' : "") + ","),
    "}",
    "params = [",
    ...request.params.map(([name, value]) => "    (" + pythonString(name) + ", " + pythonString(value) + "),"),
    "]",
  ];
  if (request.hasBody) {
    // Literal Unicode and multiline text are editable; only Python delimiters,
    // backslashes and carriage returns need escaping.
    const body = request.body.replace(/\\/g, "\\\\").replace(/'/g, "\\'").replace(/\r/g, "\\r");
    lines.push("data = '''" + body + "'''.encode(\"utf-8\")");
  }
  lines.push(
    "",
    "response = requests.request(",
    "    method=" + pythonString(request.method) + ",",
    "    url=url,",
    "    headers=headers,",
    "    params=params,",
    ...(request.hasBody ? ["    data=data,"] : []),
    "    timeout=10,",
    "    allow_redirects=False,",
    ")",
    "print(response.text)",
    "",
  );
  return lines.join("\n");
}
