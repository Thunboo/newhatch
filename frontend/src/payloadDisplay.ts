const utf8Decoder = new TextDecoder("utf-8", { fatal: false });
const MAX_EMBEDDED_JSON_DEPTH = 4;

export function decodePayload(bytes: Uint8Array): string {
  return utf8Decoder.decode(bytes);
}

export function displayPayload(bytes: Uint8Array, formatJson: boolean): string {
  const raw = decodePayload(bytes);
  const boundary = findHttpBodyBoundary(raw);
  if (!boundary) {
    return serializeJsonText(raw, formatJson, formatJson) ?? decodeDisplayEscapes(raw);
  }

  const displayedBody = serializeJsonText(
    raw.slice(boundary.bodyStart),
    formatJson,
    formatJson,
  );
  if (displayedBody === null) return decodeDisplayEscapes(raw);

  return `${decodeDisplayEscapes(raw.slice(0, boundary.headerEnd))}${boundary.separator}${displayedBody}`;
}

export function decodeDisplayEscapes(text: string): string {
  return decodePercentEncoding(decodeJsonEscapes(text));
}

function findHttpBodyBoundary(text: string): { headerEnd: number; bodyStart: number; separator: string } | null {
  const crlfIndex = text.indexOf("\r\n\r\n");
  const lfIndex = text.indexOf("\n\n");
  if (crlfIndex < 0 && lfIndex < 0) return null;

  if (crlfIndex >= 0 && (lfIndex < 0 || crlfIndex <= lfIndex)) {
    return { headerEnd: crlfIndex, bodyStart: crlfIndex + 4, separator: "\r\n\r\n" };
  }
  return { headerEnd: lfIndex, bodyStart: lfIndex + 2, separator: "\n\n" };
}

function serializeJsonText(text: string, pretty: boolean, expandEmbeddedJson: boolean): string | null {
  try {
    const parsed: unknown = JSON.parse(text.trim());
    return JSON.stringify(normalizeJsonValue(parsed, 0, expandEmbeddedJson), null, pretty ? 2 : undefined);
  } catch {
    return null;
  }
}

function normalizeJsonValue(value: unknown, depth: number, expandEmbeddedJson: boolean): unknown {
  if (typeof value === "string") {
    const decoded = decodePercentEncoding(value);
    if (!expandEmbeddedJson || depth >= MAX_EMBEDDED_JSON_DEPTH || !looksLikeJson(decoded)) return decoded;
    try {
      return normalizeJsonValue(JSON.parse(decoded), depth + 1, expandEmbeddedJson);
    } catch {
      return decoded;
    }
  }
  if (Array.isArray(value)) return value.map((item) => normalizeJsonValue(item, depth, expandEmbeddedJson));
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value).map(([key, item]) => [key, normalizeJsonValue(item, depth, expandEmbeddedJson)]),
    );
  }
  return value;
}

function looksLikeJson(text: string): boolean {
  const trimmed = text.trim();
  return (trimmed.startsWith("{") && trimmed.endsWith("}"))
    || (trimmed.startsWith("[") && trimmed.endsWith("]"));
}

function decodePercentEncoding(text: string): string {
  return text.replace(/(?:%[0-9a-fA-F]{2})+/g, (encoded) => {
    try {
      return decodeURIComponent(encoded);
    } catch {
      return encoded;
    }
  });
}

function decodeJsonEscapes(text: string): string {
  let decoded = "";
  for (let index = 0; index < text.length; index += 1) {
    if (text[index] !== "\\" || index + 1 >= text.length) {
      decoded += text[index];
      continue;
    }

    const escape = text[index + 1];
    const simpleEscapes: Record<string, string> = {
      "\"": "\"",
      "\\": "\\",
      "/": "/",
      "b": "\b",
      "f": "\f",
      "n": "\n",
      "r": "\r",
      "t": "\t",
    };
    if (escape in simpleEscapes) {
      decoded += simpleEscapes[escape];
      index += 1;
      continue;
    }

    if (escape === "u") {
      const firstHex = text.slice(index + 2, index + 6);
      if (/^[0-9a-fA-F]{4}$/.test(firstHex)) {
        const firstCodeUnit = Number.parseInt(firstHex, 16);
        const secondEscape = text.slice(index + 6, index + 8);
        const secondHex = text.slice(index + 8, index + 12);
        if (
          firstCodeUnit >= 0xd800
          && firstCodeUnit <= 0xdbff
          && secondEscape === "\\u"
          && /^[0-9a-fA-F]{4}$/.test(secondHex)
        ) {
          const secondCodeUnit = Number.parseInt(secondHex, 16);
          if (secondCodeUnit >= 0xdc00 && secondCodeUnit <= 0xdfff) {
            decoded += String.fromCodePoint(
              0x10000 + ((firstCodeUnit - 0xd800) << 10) + secondCodeUnit - 0xdc00,
            );
            index += 11;
            continue;
          }
        }
        decoded += String.fromCharCode(firstCodeUnit);
        index += 5;
        continue;
      }
    }

    decoded += text[index];
  }
  return decoded;
}
