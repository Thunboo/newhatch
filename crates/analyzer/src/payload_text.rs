//! Version 1 readable payload contract, shared with frontend/src/payloadDisplay.ts.
//! Decoding is for inspection/matching only; never use it to delimit HTTP bytes.
use serde_json::Value;

pub const VERSION: &str = "readable-v1";

pub fn display(bytes: &[u8], pretty: bool) -> String {
    let raw = String::from_utf8_lossy(bytes);
    let raw = raw.strip_prefix('\u{feff}').unwrap_or(&raw);
    let boundary = raw
        .find("\r\n\r\n")
        .map(|n| (n, 4))
        .into_iter()
        .chain(raw.find("\n\n").map(|n| (n, 2)))
        .min_by_key(|x| x.0);
    if let Some((end, separator)) = boundary {
        if let Some(body) = json_text(&raw[end + separator..], pretty) {
            return format!(
                "{}{}{}",
                decode(&raw[..end]),
                &raw[end..end + separator],
                body
            );
        }
    } else if let Some(json) = json_text(raw, pretty) {
        return json;
    }
    decode(raw)
}

fn json_text(text: &str, pretty: bool) -> Option<String> {
    let value: Value = serde_json::from_str(trim(text)).ok()?;
    if !valid_depth(&value, 0) {
        return None;
    }
    let value = normalize(value, 0, pretty);
    Some(stringify(&value, pretty, 0))
}

// JSON.stringify uses IEEE-754 numbers and enumerates array-index keys before
// other object keys. serde_json's default serializer has different rules.
pub(crate) fn stringify(value: &Value, pretty: bool, depth: usize) -> String {
    match value {
        Value::Number(number) => {
            let number = number.as_f64().unwrap_or(f64::NAN);
            if !number.is_finite() {
                return "null".into();
            }
            ryu_js::Buffer::new().format_finite(number).to_owned()
        }
        Value::Array(values) => {
            let items = values
                .iter()
                .map(|v| stringify(v, pretty, depth + 1))
                .collect::<Vec<_>>();
            join_json('[', ']', items, pretty, depth)
        }
        Value::Object(values) => {
            let mut entries: Vec<_> = values.iter().collect();
            // None keys must come after index keys while preserving insertion order.
            entries.sort_by_key(|(key, _)| {
                key.parse::<u32>()
                    .ok()
                    .filter(|n| *n != u32::MAX && n.to_string() == **key)
                    .map_or((1, 0), |n| (0, n))
            });
            let items = entries
                .into_iter()
                .map(|(key, v)| {
                    format!(
                        "{}:{}{}",
                        serde_json::to_string(key).unwrap(),
                        if pretty { " " } else { "" },
                        stringify(v, pretty, depth + 1)
                    )
                })
                .collect();
            join_json('{', '}', items, pretty, depth)
        }
        _ => serde_json::to_string(value).unwrap(),
    }
}
fn join_json(open: char, close: char, items: Vec<String>, pretty: bool, depth: usize) -> String {
    if items.is_empty() {
        return format!("{open}{close}");
    }
    if !pretty {
        return format!("{open}{}{close}", items.join(","));
    }
    let indent = "  ".repeat(depth + 1);
    format!(
        "{open}\n{indent}{}\n{}{close}",
        items.join(&format!(",\n{indent}")),
        "  ".repeat(depth)
    )
}

fn normalize(value: Value, depth: usize, expand: bool) -> Value {
    match value {
        Value::String(value) => {
            let decoded = percent(&value);
            let trimmed = trim(&decoded);
            if depth < 4
                && ((trimmed.starts_with('{') && trimmed.ends_with('}'))
                    || (trimmed.starts_with('[') && trimmed.ends_with(']')))
            {
                if let Ok(nested) = serde_json::from_str(trimmed) {
                    if !valid_depth(&nested, 0) {
                        return Value::String(decoded);
                    }
                    let nested = normalize(nested, depth + 1, expand);
                    return if expand {
                        nested
                    } else {
                        Value::String(stringify(&nested, false, 0))
                    };
                }
            }
            Value::String(decoded)
        }
        Value::Array(values) => Value::Array(
            values
                .into_iter()
                .map(|v| normalize(v, depth, expand))
                .collect(),
        ),
        Value::Object(values) => Value::Object(
            values
                .into_iter()
                .map(|(k, v)| (k, normalize(v, depth, expand)))
                .collect(),
        ),
        other => other,
    }
}

pub fn trim(text: &str) -> &str {
    text.trim_matches(|c| matches!(c, '\u{0009}'..='\u{000d}' | '\u{0020}' | '\u{00a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}'))
}

fn valid_depth(value: &Value, depth: usize) -> bool {
    if depth > 64 {
        return false;
    }
    match value {
        Value::Array(values) => values.iter().all(|v| valid_depth(v, depth + 1)),
        Value::Object(values) => values.values().all(|v| valid_depth(v, depth + 1)),
        _ => true,
    }
}

pub fn decode(text: &str) -> String {
    if !text.as_bytes().contains(&b'\\') {
        return percent(text);
    }
    let mut output = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            output.push(c);
            continue;
        }
        let Some(&escape) = chars.peek() else {
            output.push(c);
            break;
        };
        let replacement = match escape {
            '"' => Some('"'),
            '\\' => Some('\\'),
            '/' => Some('/'),
            'b' => Some('\u{0008}'),
            'f' => Some('\u{000c}'),
            'n' => Some('\n'),
            'r' => Some('\r'),
            't' => Some('\t'),
            _ => None,
        };
        if let Some(replacement) = replacement {
            chars.next();
            output.push(replacement);
            continue;
        }
        if escape == 'u' {
            let mut probe = chars.clone();
            probe.next();
            let hex: String = probe.by_ref().take(4).collect();
            if hex.len() == 4 && hex.bytes().all(|c| c.is_ascii_hexdigit()) {
                let first = u32::from_str_radix(&hex, 16).unwrap();
                let mut code = first;
                if (0xd800..=0xdbff).contains(&first) {
                    let mut second = probe.clone();
                    if second.next() == Some('\\') && second.next() == Some('u') {
                        let hex: String = second.by_ref().take(4).collect();
                        if hex.len() == 4 && hex.bytes().all(|c| c.is_ascii_hexdigit()) {
                            let low = u32::from_str_radix(&hex, 16).unwrap();
                            if (0xdc00..=0xdfff).contains(&low) {
                                code = 0x10000 + ((first - 0xd800) << 10) + low - 0xdc00;
                                probe = second;
                            }
                        }
                    }
                }
                output.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                chars = probe;
                continue;
            }
        }
        output.push(c);
    }
    percent(&output)
}

pub fn percent(text: &str) -> String {
    let bytes = text.as_bytes();
    if !bytes.contains(&b'%') {
        return text.to_owned();
    }
    let mut output = String::with_capacity(text.len());
    let mut start = 0;
    let mut index = 0;
    while index < bytes.len() {
        let begin = index;
        let mut decoded = Vec::new();
        while index + 2 < bytes.len()
            && bytes[index] == b'%'
            && bytes[index + 1].is_ascii_hexdigit()
            && bytes[index + 2].is_ascii_hexdigit()
        {
            decoded.push(u8::from_str_radix(&text[index + 1..index + 3], 16).unwrap());
            index += 3;
        }
        if index == begin {
            index += 1;
            continue;
        }
        output.push_str(&text[start..begin]);
        output.push_str(std::str::from_utf8(&decoded).unwrap_or(&text[begin..index]));
        start = index;
    }
    output.push_str(&text[start..]);
    output
}

pub fn flag_count(regex: &regex::bytes::Regex, bytes: &[u8]) -> usize {
    regex.find_iter(display(bytes, false).as_bytes()).count()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shared_examples() {
        let cases: Value =
            serde_json::from_str(include_str!("../../../test/payload_text/cases.json")).unwrap();
        for case in cases.as_array().unwrap() {
            let raw: Vec<u8> = if let Some(bytes) = case["bytes"].as_array() {
                bytes.iter().map(|b| b.as_u64().unwrap() as u8).collect()
            } else {
                case["raw"].as_str().unwrap().as_bytes().to_vec()
            };
            assert_eq!(
                display(&raw, false),
                case["compact"].as_str().unwrap(),
                "{}",
                case["name"]
            );
            assert_eq!(
                display(&raw, true),
                case["pretty"].as_str().unwrap(),
                "{}",
                case["name"]
            );
        }
    }
}
