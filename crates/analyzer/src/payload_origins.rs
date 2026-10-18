//! Read-only provenance for rule explanations. The ingest/search fast path stays
//! in payload_text; this mapper runs only for an opened retained payload.
use std::ops::Range;

use anyhow::{ensure, Result};
use serde::Serialize;

type Origin = Range<usize>;

#[derive(Clone)]
pub(crate) struct MappedText {
    pub text: String,
    origins: Vec<Origin>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl MappedText {
    pub fn from_bytes(bytes: &[u8], offset: usize) -> Self {
        let mut output = Self {
            text: String::with_capacity(bytes.len()),
            origins: Vec::with_capacity(bytes.len()),
        };
        let mut at = 0;
        while at < bytes.len() {
            let (valid, invalid) = match std::str::from_utf8(&bytes[at..]) {
                Ok(text) => (text.len(), 0),
                Err(error) => (
                    error.valid_up_to(),
                    error
                        .error_len()
                        .unwrap_or(bytes.len() - at - error.valid_up_to()),
                ),
            };
            let text = std::str::from_utf8(&bytes[at..at + valid]).unwrap();
            for (index, character) in text.char_indices() {
                output.text.push(character);
                output.origins.extend(
                    std::iter::repeat(
                        offset + at + index..offset + at + index + character.len_utf8(),
                    )
                    .take(character.len_utf8()),
                );
            }
            at += valid;
            if invalid > 0 {
                output.text.push('\u{fffd}');
                output
                    .origins
                    .extend(std::iter::repeat(offset + at..offset + at + invalid).take(3));
                at += invalid;
            }
        }
        output
    }

    fn literal(text: &str, origin: Origin) -> Self {
        Self {
            text: text.to_owned(),
            origins: vec![origin; text.len()],
        }
    }

    /// Join HTTP body bytes before UTF-8 decoding: a scalar or escape may cross
    /// chunk boundaries. Origins remain in the original retained message.
    pub fn from_parts(bytes: &[u8], parts: &[Range<usize>], offset: usize) -> Self {
        let mut joined = Vec::new();
        let mut starts = Vec::new();
        for part in parts {
            starts.push(joined.len());
            joined.extend_from_slice(&bytes[part.clone()]);
        }
        let mut mapped = Self::from_bytes(&joined, 0);
        let source = |index: usize| {
            let part = starts.partition_point(|start| *start <= index) - 1;
            offset + parts[part].start + index - starts[part]
        };
        for origin in &mut mapped.origins {
            *origin = source(origin.start)..source(origin.end - 1) + 1;
        }
        mapped
    }

    fn synthetic(text: &str) -> Self {
        Self::literal(text, usize::MAX..usize::MAX)
    }

    pub fn join(parts: Vec<Self>) -> Self {
        let mut text = String::new();
        let mut origins = Vec::new();
        for part in parts {
            text.push_str(&part.text);
            origins.extend(part.origins);
        }
        Self { text, origins }
    }

    pub fn slice(&self, range: Range<usize>) -> Self {
        Self {
            text: self.text[range.clone()].to_owned(),
            origins: self.origins[range].to_vec(),
        }
    }

    fn origin(&self, range: Range<usize>) -> Origin {
        let origins = &self.origins[range];
        let start = origins.iter().map(|o| o.start).min().unwrap_or(usize::MAX);
        let end = origins
            .iter()
            .filter(|o| o.start != usize::MAX)
            .map(|o| o.end)
            .max()
            .unwrap_or(usize::MAX);
        start..end
    }

    fn escapes(self) -> Self {
        if !self.text.as_bytes().contains(&b'\\') {
            return self;
        }
        let bytes = self.text.as_bytes();
        let mut parts = Vec::new();
        let mut at = 0;
        let mut untouched = 0;
        while at < bytes.len() {
            if bytes[at] == b'\\' && at + 1 < bytes.len() {
                let replacement = match bytes[at + 1] {
                    b'"' => Some('"'),
                    b'\\' => Some('\\'),
                    b'/' => Some('/'),
                    b'b' => Some('\u{8}'),
                    b'f' => Some('\u{c}'),
                    b'n' => Some('\n'),
                    b'r' => Some('\r'),
                    b't' => Some('\t'),
                    _ => None,
                };
                if let Some(character) = replacement {
                    parts.push(self.slice(untouched..at));
                    parts.push(Self::literal(
                        &character.to_string(),
                        self.origin(at..at + 2),
                    ));
                    at += 2;
                    untouched = at;
                    continue;
                }
                if bytes[at + 1] == b'u'
                    && at + 6 <= bytes.len()
                    && bytes[at + 2..at + 6].iter().all(u8::is_ascii_hexdigit)
                {
                    let first = u32::from_str_radix(&self.text[at + 2..at + 6], 16).unwrap();
                    let mut code = first;
                    let mut length = 6;
                    if (0xd800..=0xdbff).contains(&first)
                        && bytes.get(at + 6..at + 8) == Some(b"\\u")
                        && at + 12 <= bytes.len()
                        && bytes[at + 8..at + 12].iter().all(u8::is_ascii_hexdigit)
                    {
                        let low = u32::from_str_radix(&self.text[at + 8..at + 12], 16).unwrap();
                        if (0xdc00..=0xdfff).contains(&low) {
                            code = 0x10000 + ((first - 0xd800) << 10) + low - 0xdc00;
                            length = 12;
                        }
                    }
                    parts.push(self.slice(untouched..at));
                    parts.push(Self::literal(
                        &char::from_u32(code).unwrap_or('\u{fffd}').to_string(),
                        self.origin(at..at + length),
                    ));
                    at += length;
                    untouched = at;
                    continue;
                }
            }
            at += 1;
        }
        parts.push(self.slice(untouched..self.text.len()));
        Self::join(parts)
    }

    fn percent(self) -> Self {
        if !self.text.as_bytes().contains(&b'%') {
            return self;
        }
        let bytes = self.text.as_bytes();
        let mut parts = Vec::new();
        let mut at = 0;
        let mut untouched = 0;
        while at < bytes.len() {
            let begin = at;
            let mut decoded = Vec::new();
            while at + 2 < bytes.len()
                && bytes[at] == b'%'
                && bytes[at + 1].is_ascii_hexdigit()
                && bytes[at + 2].is_ascii_hexdigit()
            {
                decoded.push(u8::from_str_radix(&self.text[at + 1..at + 3], 16).unwrap());
                at += 3;
            }
            if at == begin {
                at += 1;
            } else if let Ok(text) = std::str::from_utf8(&decoded) {
                parts.push(self.slice(untouched..begin));
                for (index, character) in text.char_indices() {
                    parts.push(Self::literal(
                        &character.to_string(),
                        self.origin(begin + index * 3..begin + (index + character.len_utf8()) * 3),
                    ));
                }
                untouched = at;
            }
        }
        parts.push(self.slice(untouched..self.text.len()));
        Self::join(parts)
    }

    pub fn decode(self) -> Self {
        self.escapes().percent()
    }

    pub fn display(self, pretty: bool) -> Self {
        let raw = if self.text.starts_with('\u{feff}') {
            self.slice(3..self.text.len())
        } else {
            self
        };
        let boundary = raw
            .text
            .find("\r\n\r\n")
            .map(|n| (n, 4))
            .into_iter()
            .chain(raw.text.find("\n\n").map(|n| (n, 2)))
            .min_by_key(|p| p.0);
        if let Some((end, separator)) = boundary {
            if let Some(body) = raw
                .slice(end + separator..raw.text.len())
                .json(pretty, 0, 0)
            {
                return Self::join(vec![
                    raw.slice(0..end).decode(),
                    raw.slice(end..end + separator),
                    body,
                ]);
            }
        } else if let Some(json) = raw.json(pretty, 0, 0) {
            return json;
        }
        raw.decode()
    }

    fn json(&self, pretty: bool, embedded: usize, level: usize) -> Option<Self> {
        let validation: serde_json::Value =
            serde_json::from_str(crate::payload_text::trim(&self.text)).ok()?;
        fn depth(value: &serde_json::Value, level: usize) -> bool {
            level <= 64
                && match value {
                    serde_json::Value::Array(items) => items.iter().all(|v| depth(v, level + 1)),
                    serde_json::Value::Object(items) => items.values().all(|v| depth(v, level + 1)),
                    _ => true,
                }
        }
        if !depth(&validation, 0) {
            return None;
        }
        drop(validation);
        let trimmed = crate::payload_text::trim(&self.text);
        let start = trimmed.as_ptr() as usize - self.text.as_ptr() as usize;
        let trimmed_source = (start != 0 || trimmed.len() != self.text.len())
            .then(|| self.slice(start..start + trimmed.len()));
        let source = trimmed_source.as_ref().unwrap_or(self);
        let mut parser = Parser { source, at: 0 };
        Some(parser.node().render(pretty, level, embedded))
    }

    pub fn sources(&self, range: Range<usize>) -> Vec<Origin> {
        merge(
            self.origins[range]
                .iter()
                .filter(|r| r.start != usize::MAX)
                .cloned()
                .collect(),
        )
    }

    pub fn project(&self, sources: Vec<Origin>) -> Vec<Span> {
        let sources = merge(sources);
        let mut spans: Vec<Span> = Vec::new();
        for (at, origin) in self.origins.iter().enumerate() {
            if origin.start == usize::MAX {
                continue;
            }
            let index = sources.partition_point(|range| range.end <= origin.start);
            if sources
                .get(index)
                .is_some_and(|range| range.start < origin.end)
            {
                if let Some(last) = spans.last_mut().filter(|span| span.end == at) {
                    last.end += 1;
                } else {
                    spans.push(Span {
                        start: at,
                        end: at + 1,
                    });
                }
            }
        }
        spans
    }
}

pub(crate) fn merge(mut ranges: Vec<Origin>) -> Vec<Origin> {
    ranges.sort_by_key(|range| (range.start, range.end));
    let mut result: Vec<Origin> = Vec::new();
    for range in ranges {
        if let Some(last) = result.last_mut().filter(|last| last.end >= range.start) {
            last.end = last.end.max(range.end);
        } else {
            result.push(range);
        }
    }
    result
}

enum Node {
    String(MappedText, Origin, Origin),
    Literal(MappedText),
    Object(Vec<(Node, Node, Origin)>, Vec<Origin>, Origin, Origin),
    Array(Vec<Node>, Vec<Origin>, Origin, Origin),
}

struct Parser<'a> {
    source: &'a MappedText,
    at: usize,
}
impl Parser<'_> {
    fn skip(&mut self) {
        while self
            .source
            .text
            .as_bytes()
            .get(self.at)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.at += 1;
        }
    }
    fn token(&mut self) -> Origin {
        let origin = self.source.origin(self.at..self.at + 1);
        self.at += 1;
        origin
    }
    fn node(&mut self) -> Node {
        self.skip();
        match self.source.text.as_bytes()[self.at] {
            b'"' => {
                let open = self.token();
                let start = self.at;
                while self.source.text.as_bytes()[self.at] != b'"' {
                    self.at += if self.source.text.as_bytes()[self.at] == b'\\' {
                        2
                    } else {
                        1
                    };
                }
                let value = self.source.slice(start..self.at).escapes();
                let close = self.token();
                Node::String(value, open, close)
            }
            b'{' => {
                let open = self.token();
                let mut entries: Vec<(Node, Node, Origin)> = Vec::new();
                let mut keys = std::collections::HashMap::new();
                let mut commas = Vec::new();
                self.skip();
                while self.source.text.as_bytes()[self.at] != b'}' {
                    let key = self.node();
                    self.skip();
                    let colon = self.token();
                    let value = self.node();
                    self.skip();
                    if let Some(index) = keys.get(key.key()).copied() {
                        let entry: &mut (Node, Node, Origin) = &mut entries[index];
                        entry.1 = value;
                        entry.2 = colon;
                    } else {
                        keys.insert(key.key().to_owned(), entries.len());
                        entries.push((key, value, colon));
                    }
                    if self.source.text.as_bytes()[self.at] == b',' {
                        commas.push(self.token());
                        self.skip();
                    }
                }
                Node::Object(entries, commas, open, self.token())
            }
            b'[' => {
                let open = self.token();
                let mut entries = Vec::new();
                let mut commas = Vec::new();
                self.skip();
                while self.source.text.as_bytes()[self.at] != b']' {
                    entries.push(self.node());
                    self.skip();
                    if self.source.text.as_bytes()[self.at] == b',' {
                        commas.push(self.token());
                        self.skip();
                    }
                }
                Node::Array(entries, commas, open, self.token())
            }
            _ => {
                let start = self.at;
                while self
                    .source
                    .text
                    .as_bytes()
                    .get(self.at)
                    .is_some_and(|b| !b.is_ascii_whitespace() && !b",]}".contains(b))
                {
                    self.at += 1;
                }
                let original = self.source.slice(start..self.at);
                let value: serde_json::Value = serde_json::from_str(&original.text).unwrap();
                let canonical = crate::payload_text::stringify(&value, false, 0);
                Node::Literal(if canonical == original.text {
                    original
                } else {
                    MappedText::literal(&canonical, self.source.origin(start..self.at))
                })
            }
        }
    }
}
impl Node {
    fn key(&self) -> &str {
        match self {
            Self::String(value, _, _) => &value.text,
            _ => unreachable!(),
        }
    }
    fn quoted(value: &MappedText, open: Origin, close: Origin) -> MappedText {
        let mut parts = vec![MappedText::literal("\"", open)];
        let mut untouched = 0;
        for (index, character) in value.text.char_indices() {
            if character != '"' && character != '\\' && character >= '\u{20}' {
                continue;
            }
            parts.push(value.slice(untouched..index));
            let encoded = serde_json::to_string(&character.to_string()).unwrap();
            parts.push(MappedText::literal(
                &encoded[1..encoded.len() - 1],
                value.origin(index..index + character.len_utf8()),
            ));
            untouched = index + character.len_utf8();
        }
        parts.push(value.slice(untouched..value.text.len()));
        parts.push(MappedText::literal("\"", close));
        MappedText::join(parts)
    }
    fn render(self, pretty: bool, level: usize, embedded: usize) -> MappedText {
        match self {
            Self::Literal(value) => value,
            Self::String(value, open, close) => {
                let value = value.percent();
                let trimmed = crate::payload_text::trim(&value.text);
                if embedded < 4
                    && ((trimmed.starts_with('{') && trimmed.ends_with('}'))
                        || (trimmed.starts_with('[') && trimmed.ends_with(']')))
                {
                    if let Some(nested) = value.json(pretty, embedded + 1, level) {
                        return if pretty {
                            nested
                        } else {
                            Self::quoted(&nested, open, close)
                        };
                    }
                }
                Self::quoted(&value, open, close)
            }
            Self::Object(mut entries, commas, open, close) => {
                entries.sort_by_key(|(key, _, _)| {
                    key.key()
                        .parse::<u32>()
                        .ok()
                        .filter(|n| *n != u32::MAX && n.to_string() == key.key())
                        .map_or((1, 0), |n| (0, n))
                });
                let values = entries
                    .into_iter()
                    .map(|(key, value, colon)| {
                        let Self::String(text, open, close) = key else {
                            unreachable!()
                        };
                        MappedText::join(vec![
                            Self::quoted(&text, open, close),
                            MappedText::literal(if pretty { ": " } else { ":" }, colon),
                            value.render(pretty, level + 1, embedded),
                        ])
                    })
                    .collect();
                Self::container(values, commas, (open, close), ('{', '}'), pretty, level)
            }
            Self::Array(entries, commas, open, close) => Self::container(
                entries
                    .into_iter()
                    .map(|value| value.render(pretty, level + 1, embedded))
                    .collect(),
                commas,
                (open, close),
                ('[', ']'),
                pretty,
                level,
            ),
        }
    }
    fn container(
        values: Vec<MappedText>,
        commas: Vec<Origin>,
        (open, close): (Origin, Origin),
        (left, right): (char, char),
        pretty: bool,
        level: usize,
    ) -> MappedText {
        let mut parts = vec![MappedText::literal(&left.to_string(), open)];
        let length = values.len();
        for (index, value) in values.into_iter().enumerate() {
            if index > 0 {
                parts.push(MappedText::literal(",", commas[index - 1].clone()));
            }
            if pretty {
                parts.push(MappedText::synthetic(&format!(
                    "\n{}",
                    "  ".repeat(level + 1)
                )));
            }
            parts.push(value);
        }
        if pretty && length > 0 {
            parts.push(MappedText::synthetic(&format!("\n{}", "  ".repeat(level))));
        }
        parts.push(MappedText::literal(&right.to_string(), close));
        MappedText::join(parts)
    }
}

pub(crate) fn display(bytes: &[u8], pretty: bool) -> Result<MappedText> {
    let mapped = MappedText::from_bytes(bytes, 0).display(pretty);
    ensure!(
        mapped.text == crate::payload_text::display(bytes, pretty),
        "rule explanation normalization mismatch"
    );
    Ok(mapped)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shared_display_fixtures_preserve_provenance() {
        let examples: serde_json::Value =
            serde_json::from_str(include_str!("../../../test/payload_text/cases.json")).unwrap();
        for example in examples.as_array().unwrap() {
            let bytes: Vec<u8> = if let Some(bytes) = example["bytes"].as_array() {
                bytes.iter().map(|b| b.as_u64().unwrap() as u8).collect()
            } else {
                example["raw"].as_str().unwrap().as_bytes().to_vec()
            };
            for (pretty, key) in [(false, "compact"), (true, "pretty")] {
                assert_eq!(
                    MappedText::from_bytes(&bytes, 0).display(pretty).text,
                    example[key].as_str().unwrap(),
                    "{}",
                    example["name"]
                );
            }
        }
    }
}
