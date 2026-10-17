//! Explanations of the first occurrence selected by the list classifier. Only
//! the opened record is read; no chain scan or browser-side regex is needed.
use std::{
    collections::HashSet,
    ops::Range,
    time::{Duration, Instant},
};

use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};

use super::{CompiledRule, Condition, Field, Operator};
use crate::{
    domain::SessionPayload,
    http_sessions::Exchange,
    payload_origins::{self, MappedText, Span},
};

/// Compact URL representation: [rule_id, definition_version, [[step, transaction], ...]].
#[derive(Deserialize)]
pub struct RuleSelection(pub i64, pub String, pub Vec<(usize, usize)>);

#[derive(Debug, thiserror::Error)]
#[error("rule matches changed; refresh the list and reopen the session")]
pub struct StaleRuleMatch;

#[derive(Debug, Serialize)]
pub struct WindowRule {
    pub rule_id: i64,
    pub label: String,
    pub ranges: Vec<Span>,
    pub pretty_ranges: Vec<Span>,
}
#[derive(Debug, Default, Serialize)]
pub struct RuleHighlights {
    pub representation: &'static str,
    pub c2s: Vec<WindowRule>,
    pub s2c: Vec<WindowRule>,
}

pub fn validate_selection(selection: &[RuleSelection]) -> Result<()> {
    ensure!(selection.len() <= 100, "at most 100 rule selections");
    let mut rules = HashSet::new();
    for RuleSelection(id, version, steps) in selection {
        ensure!(
            *id > 0 && rules.insert(*id),
            "invalid or duplicate rule selection"
        );
        ensure!(
            version.len() == 16 && version.bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid rule version"
        );
        ensure!(
            !steps.is_empty() && steps.len() <= 8,
            "invalid selected steps"
        );
        let mut seen_steps = HashSet::new();
        let mut seen_events = HashSet::new();
        for (step, transaction) in steps {
            ensure!(
                *step < 8
                    && *transaction < 256
                    && seen_steps.insert(*step)
                    && seen_events.insert(*transaction),
                "invalid selected step or transaction"
            );
        }
    }
    Ok(())
}

pub fn explain(
    id: i64,
    source_id: i64,
    payload: &SessionPayload,
    rules: &[CompiledRule],
    selection: &[RuleSelection],
) -> Result<RuleHighlights> {
    let deadline = Instant::now() + Duration::from_secs(2);
    let exchanges = super::retained_exchanges(payload);
    let events = exchanges
        .iter()
        .enumerate()
        .map(|(transaction, exchange)| super::extract_event(id, transaction, exchange))
        .collect::<Option<Vec<_>>>()
        .ok_or(StaleRuleMatch)?;
    let mut requests = Vec::new();
    let mut responses = Vec::new();
    for RuleSelection(rule_id, version, steps) in selection {
        let rule = rules
            .iter()
            .find(|r| {
                r.rule.id == *rule_id
                    && r.rule.input.source_id == source_id
                    && r.version() == *version
            })
            .ok_or(StaleRuleMatch)?;
        let mut sources_c2s = Vec::new();
        let mut sources_s2c = Vec::new();
        let mut has_c2s = false;
        let mut has_s2c = false;
        for (step, transaction) in steps {
            let event = events.get(*transaction).ok_or(StaleRuleMatch)?;
            let conditions = rule.steps.get(*step).ok_or(StaleRuleMatch)?;
            ensure!(rule.step_matches(*step, event), StaleRuleMatch);
            for (condition, regex) in conditions {
                ensure!(
                    Instant::now() < deadline,
                    "classification deadline exceeded; retry rule highlights"
                );
                let fields = fields(&exchanges[*transaction], condition)?;
                let exchange = &exchanges[*transaction];
                let body = match condition.field {
                    Field::RequestBody => Some((&exchange.c2s, exchange.request_offset)),
                    Field::ResponseBody => Some((&exchange.s2c, exchange.response_offset)),
                    _ => None,
                };
                let allowed = body.and_then(|(bytes, offset)| {
                    body_parts(bytes).map(|parts| {
                        parts
                            .into_iter()
                            .map(|part| offset + part.start..offset + part.end)
                            .collect::<Vec<_>>()
                    })
                });
                let mut matched = false;
                let response_field = matches!(condition.field, Field::Status | Field::ResponseBody);
                for field in fields {
                    if let Some(ranges) = condition_ranges(&field.text, condition, regex.as_ref()) {
                        matched = true;
                        for range in ranges {
                            let mut sources = field.sources(range);
                            if let Some(parts) = &allowed {
                                sources = intersect(&sources, parts);
                            }
                            if response_field {
                                sources_s2c.extend(sources);
                            } else {
                                sources_c2s.extend(sources);
                            }
                        }
                    }
                }
                ensure!(matched, StaleRuleMatch);
                if response_field {
                    has_s2c = true;
                } else {
                    has_c2s = true;
                }
            }
        }
        if has_c2s {
            requests.push((
                *rule_id,
                rule.rule.input.label.clone(),
                payload_origins::merge(sources_c2s),
            ));
        }
        if has_s2c {
            responses.push((
                *rule_id,
                rule.rule.input.label.clone(),
                payload_origins::merge(sources_s2c),
            ));
        }
    }
    Ok(RuleHighlights {
        representation: crate::payload_text::VERSION,
        c2s: project(&payload.c2s, requests, deadline)?,
        s2c: project(&payload.s2c, responses, deadline)?,
    })
}

type PendingWindow = (i64, String, Vec<Range<usize>>);

fn intersect(sources: &[Range<usize>], parts: &[Range<usize>]) -> Vec<Range<usize>> {
    let (mut source, mut part) = (0, 0);
    let mut result = Vec::new();
    while source < sources.len() && part < parts.len() {
        let start = sources[source].start.max(parts[part].start);
        let end = sources[source].end.min(parts[part].end);
        if start < end {
            result.push(start..end);
        }
        if sources[source].end <= parts[part].end {
            source += 1;
        } else {
            part += 1;
        }
    }
    result
}

fn project(
    bytes: &[u8],
    pending: Vec<PendingWindow>,
    deadline: Instant,
) -> Result<Vec<WindowRule>> {
    let mut windows: Vec<_> = pending
        .iter()
        .map(|(id, label, _)| WindowRule {
            rule_id: *id,
            label: label.clone(),
            ranges: Vec::new(),
            pretty_ranges: Vec::new(),
        })
        .collect();
    if pending.iter().all(|(_, _, sources)| sources.is_empty()) {
        return Ok(windows);
    }
    // Keep only one direction/representation's provenance map in memory.
    for pretty in [false, true] {
        let mapped = payload_origins::display(bytes, pretty)?;
        for (window, (_, _, sources)) in windows.iter_mut().zip(&pending) {
            ensure!(
                Instant::now() < deadline,
                "classification deadline exceeded; retry rule highlights"
            );
            let ranges = mapped.project(sources.clone());
            if pretty {
                window.pretty_ranges = ranges;
            } else {
                window.ranges = ranges;
            }
        }
    }
    Ok(windows)
}

fn condition_ranges(
    text: &str,
    condition: &Condition,
    regex: Option<&regex::Regex>,
) -> Option<Vec<Range<usize>>> {
    match condition.op {
        Operator::Equals => (text == condition.value).then(|| {
            if text.is_empty() {
                Vec::new()
            } else {
                std::iter::once(0..text.len()).collect()
            }
        }),
        Operator::Contains => {
            let mut ranges = Vec::new();
            let mut at = 0;
            while let Some(offset) = text[at..].find(&condition.value) {
                let start = at + offset;
                push_range(&mut ranges, start..start + condition.value.len());
                at = start + text[start..].chars().next()?.len_utf8();
            }
            (!ranges.is_empty()).then_some(ranges)
        }
        Operator::Regex => {
            let mut matched = false;
            let mut ranges = Vec::new();
            for found in regex?.find_iter(text) {
                matched = true;
                push_range(&mut ranges, found.range());
            }
            matched.then_some(ranges)
        }
    }
}

fn push_range(ranges: &mut Vec<Range<usize>>, range: Range<usize>) {
    if range.is_empty() {
        return;
    }
    if let Some(last) = ranges.last_mut().filter(|last| last.end >= range.start) {
        last.end = last.end.max(range.end);
    } else {
        ranges.push(range);
    }
}

fn fields(exchange: &Exchange, condition: &Condition) -> Result<Vec<MappedText>> {
    let request = &exchange.c2s;
    let method_end = request
        .iter()
        .position(|b| *b == b' ')
        .ok_or(StaleRuleMatch)?;
    let target_start = request[method_end..]
        .iter()
        .position(|b| *b != b' ')
        .map(|n| n + method_end)
        .ok_or(StaleRuleMatch)?;
    let target_end = request[target_start..]
        .iter()
        .position(|b| *b == b' ')
        .map(|n| n + target_start)
        .ok_or(StaleRuleMatch)?;
    let path_end = request[target_start..target_end]
        .iter()
        .position(|b| *b == b'?')
        .map_or(target_end, |n| n + target_start);
    let query_start = (path_end + 1).min(target_end);
    let part = |range: Range<usize>| {
        MappedText::from_bytes(
            &request[range.clone()],
            exchange.request_offset + range.start,
        )
        .decode()
    };
    Ok(match condition.field {
        Field::Method => vec![part(0..method_end)],
        Field::Path => vec![part(target_start..path_end)],
        Field::Target => vec![part(target_start..target_end)],
        Field::Query => {
            if let Some(name) = &condition.name {
                let mut at = query_start;
                let mut values = Vec::new();
                for parameter in request[query_start..target_end].split(|b| *b == b'&') {
                    if !parameter.is_empty() {
                        let equals = parameter
                            .iter()
                            .position(|b| *b == b'=')
                            .unwrap_or(parameter.len());
                        if part(at..at + equals).text == *name {
                            values.push(part(
                                at + (equals + 1).min(parameter.len())..at + parameter.len(),
                            ));
                        }
                    }
                    at += parameter.len() + 1;
                }
                values
            } else {
                vec![part(query_start..target_end)]
            }
        }
        Field::RequestBody => body(request, exchange.request_offset).into_iter().collect(),
        Field::ResponseBody => body(&exchange.s2c, exchange.response_offset)
            .into_iter()
            .collect(),
        Field::Status => {
            let response = &exchange.s2c;
            let start = response
                .iter()
                .position(|b| *b == b' ')
                .ok_or(StaleRuleMatch)?;
            let start = response[start..]
                .iter()
                .position(|b| *b != b' ')
                .map(|n| n + start)
                .ok_or(StaleRuleMatch)?;
            let status = MappedText::from_bytes(
                response.get(start..start + 3).ok_or(StaleRuleMatch)?,
                exchange.response_offset + start,
            );
            let significant = status.text.trim_start_matches('0').len().max(1);
            vec![status.slice(status.text.len() - significant..status.text.len())]
        }
    })
}

fn body(message: &[u8], offset: usize) -> Option<MappedText> {
    let parts = body_parts(message)?;
    Some(MappedText::from_parts(message, &parts, offset).display(false))
}

fn body_parts(message: &[u8]) -> Option<Vec<Range<usize>>> {
    let mut encoded = false;
    let mut chunked = false;
    crate::protocol::for_each_header(message, |h| {
        encoded |= h.name.eq_ignore_ascii_case("content-encoding")
            && !h.value.eq_ignore_ascii_case(b"identity");
        chunked |= h.name.eq_ignore_ascii_case("transfer-encoding")
            && h.value.eq_ignore_ascii_case(b"chunked");
        Ok(())
    })
    .ok()?;
    if encoded {
        return None;
    }
    let mut start = 0;
    for line in message.split_inclusive(|byte| *byte == b'\n') {
        start += line.len();
        if line == b"\r\n" || line == b"\n" {
            break;
        }
    }
    if !chunked {
        return Some(std::iter::once(start..message.len()).collect());
    }
    let mut parts = Vec::new();
    let mut at = start;
    loop {
        let end = message[at..].windows(2).position(|s| s == b"\r\n")? + at;
        let size = std::str::from_utf8(&message[at..end])
            .ok()?
            .split(';')
            .next()?;
        let size = usize::from_str_radix(size.trim(), 16).ok()?;
        at = end + 2;
        if size == 0 {
            return Some(parts);
        }
        let data_end = at.checked_add(size)?;
        if message.get(data_end..data_end.checked_add(2)?)? != b"\r\n" {
            return None;
        }
        parts.push(at..data_end);
        at += size + 2;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attack_rules::{AttackRule, RuleInput};

    fn rule(steps: serde_json::Value) -> CompiledRule {
        CompiledRule::new(AttackRule {
            id: 7,
            owner: "team".into(),
            input: serde_json::from_value::<RuleInput>(
                serde_json::json!({"source_id":1,"label":"Traversal","steps":steps}),
            )
            .unwrap(),
        })
        .unwrap()
    }
    fn selected(rule: &CompiledRule, steps: Vec<(usize, usize)>) -> Vec<RuleSelection> {
        vec![RuleSelection(rule.rule.id, rule.version(), steps)]
    }
    fn marked(bytes: &[u8], rules: &[WindowRule], pretty: bool) -> String {
        let text = crate::payload_text::display(bytes, pretty);
        rules
            .iter()
            .flat_map(|rule| {
                if pretty {
                    &rule.pretty_ranges
                } else {
                    &rule.ranges
                }
            })
            .map(|range| &text[range.start..range.end])
            .collect()
    }
    fn response(body: &str) -> Vec<u8> {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .into_bytes()
    }

    #[test]
    fn named_query_does_not_mark_equal_text_in_other_fields() {
        let rule =
            rule(serde_json::json!([[{"field":"query","name":"id","op":"equals","value":"А"}]]));
        let payload = SessionPayload {
            c2s: "GET /А?id=%D0%90&other=%D0%90&id=safe HTTP/1.1\r\nHost: А\r\n\r\n"
                .as_bytes()
                .to_vec(),
            s2c: response("А"),
        };
        let selection = selected(&rule, vec![(0, 0)]);
        let found = explain(1, 1, &payload, &[rule], &selection).unwrap();
        assert_eq!(marked(&payload.c2s, &found.c2s, false), "А");
        assert!(found.s2c.is_empty());
        assert_eq!(found.c2s[0].ranges.len(), 1);
        let text = crate::payload_text::display(&payload.c2s, false);
        assert_eq!(found.c2s[0].ranges[0].start, text.find("?id=").unwrap() + 4);
    }

    #[test]
    fn selected_legacy_occurrence_preserves_informational_response_offsets() {
        let rule = rule(
            serde_json::json!([[{"field":"path","op":"equals","value":"/hit"},{"field":"response_body","op":"regex","value":"root:"}]]),
        );
        let mut replies = b"HTTP/1.1 100 Continue\r\nX-Test: root:\r\n\r\n".to_vec();
        replies.extend(response("root:"));
        replies.extend(response("root:"));
        let payload = SessionPayload {
            c2s: b"GET /hit HTTP/1.1\r\n\r\nGET /hit HTTP/1.1\r\n\r\n".to_vec(),
            s2c: replies,
        };
        let events = super::super::extract_events(1, &payload).unwrap();
        let matched = rule
            .find(&events.into_iter().map(Some).collect::<Vec<_>>())
            .unwrap();
        assert_eq!(matched.events[0].transaction, 0);
        let selection = selected(&rule, vec![(0, matched.events[0].transaction)]);
        let found = explain(1, 1, &payload, &[rule], &selection).unwrap();
        assert_eq!(marked(&payload.c2s, &found.c2s, false), "/hit");
        assert_eq!(marked(&payload.s2c, &found.s2c, false), "root:");
        let text = crate::payload_text::display(&payload.s2c, false);
        assert_eq!(
            found.s2c[0].ranges[0].start,
            text.find("\r\n\r\nroot:").unwrap() + 4
        );
    }

    #[test]
    fn json_expansion_and_decoding_preserve_exact_value_origin() {
        let rule =
            rule(serde_json::json!([[{"field":"response_body","op":"contains","value":"А"}]]));
        let payload = SessionPayload {
            c2s: b"GET / HTTP/1.1\r\n\r\n".to_vec(),
            s2c: response(r#"{ "nested": "{\"value\":\"%D0%90\"}", "keyА": "safe" }"#),
        };
        let selection = selected(&rule, vec![(0, 0)]);
        let found = explain(1, 1, &payload, &[rule], &selection).unwrap();
        for pretty in [false, true] {
            assert_eq!(marked(&payload.s2c, &found.s2c, pretty), "АА");
        }
        assert!(found.c2s.is_empty());
    }

    #[test]
    fn chunked_body_marks_data_across_chunk_boundaries_only() {
        let rule =
            rule(serde_json::json!([[{"field":"response_body","op":"contains","value":"root:"}]]));
        let payload = SessionPayload { c2s: b"GET / HTTP/1.1\r\n\r\n".to_vec(), s2c: b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nro\r\n3\r\not:\r\n0\r\n\r\n".to_vec() };
        let selection = selected(&rule, vec![(0, 0)]);
        let found = explain(1, 1, &payload, &[rule], &selection).unwrap();
        assert_eq!(marked(&payload.s2c, &found.s2c, false), "root:");
        assert_eq!(found.s2c[0].ranges.len(), 2);
    }

    #[test]
    fn chunked_unicode_and_percent_sequences_do_not_mark_framing() {
        for (chunks, expected) in [
            (b"3\r\n%D0\r\n3\r\n%90\r\n0\r\n\r\n".as_slice(), "%D0%90"),
            (
                b"1\r\n\xd0\r\n1\r\n\x90\r\n0\r\n\r\n".as_slice(),
                "\u{fffd}\u{fffd}",
            ),
        ] {
            let rule =
                rule(serde_json::json!([[{"field":"response_body","op":"equals","value":"А"}]]));
            let selection = selected(&rule, vec![(0, 0)]);
            let mut response = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec();
            response.extend_from_slice(chunks);
            let payload = SessionPayload {
                c2s: b"GET / HTTP/1.1\r\n\r\n".to_vec(),
                s2c: response,
            };
            let found = explain(1, 1, &payload, &[rule], &selection).unwrap();
            assert_eq!(marked(&payload.s2c, &found.s2c, false), expected);
            assert_eq!(found.s2c[0].ranges.len(), 2);
        }
    }

    #[test]
    fn empty_and_zero_width_matches_keep_direction_labels_without_marks() {
        for condition in [
            serde_json::json!({"field":"request_body","op":"equals","value":""}),
            serde_json::json!({"field":"query","op":"regex","value":"^$"}),
        ] {
            let rule = rule(serde_json::json!([[condition]]));
            let payload = SessionPayload {
                c2s: b"GET / HTTP/1.1\r\n\r\n".to_vec(),
                s2c: response(""),
            };
            let selection = selected(&rule, vec![(0, 0)]);
            let found = explain(1, 1, &payload, &[rule], &selection).unwrap();
            assert_eq!(found.c2s.len(), 1);
            assert!(found.c2s[0].ranges.is_empty());
            assert!(found.s2c.is_empty());
        }
    }

    #[test]
    fn changed_rule_or_nonmatching_step_cannot_produce_highlights() {
        let rule = rule(serde_json::json!([[{"field":"path","op":"equals","value":"/hit"}]]));
        let payload = SessionPayload {
            c2s: b"GET /noise HTTP/1.1\r\n\r\n".to_vec(),
            s2c: response(""),
        };
        let selection = selected(&rule, vec![(0, 0)]);
        assert!(explain(1, 1, &payload, &[rule], &selection)
            .unwrap_err()
            .downcast_ref::<StaleRuleMatch>()
            .is_some());
    }

    #[test]
    fn large_body_keeps_marks_local_and_equals_includes_json_punctuation() {
        for body in [
            format!("{}%72oot%3A", "x".repeat(256 * 1024)),
            r#"{ "2": "b", "1": "a" }"#.to_owned(),
        ] {
            let canonical = crate::payload_text::display(body.as_bytes(), false);
            let condition = if body.starts_with('{') {
                serde_json::json!({"field":"response_body","op":"equals","value":canonical})
            } else {
                serde_json::json!({"field":"response_body","op":"contains","value":"root:"})
            };
            let rule = rule(serde_json::json!([[condition]]));
            let selection = selected(&rule, vec![(0, 0)]);
            let payload = SessionPayload {
                c2s: b"GET / HTTP/1.1\r\n\r\n".to_vec(),
                s2c: response(&body),
            };
            let found = explain(1, 1, &payload, &[rule], &selection).unwrap();
            assert_eq!(
                marked(&payload.s2c, &found.s2c, false),
                if body.starts_with('{') {
                    canonical
                } else {
                    "root:".into()
                }
            );
        }
    }
}
