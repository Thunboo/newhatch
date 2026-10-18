//! Source- and owner-scoped patterns over ordered HTTP transactions.
use anyhow::{ensure, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};

mod highlights;
pub use highlights::{
    explain, validate_selection, window_labels, RuleHighlights, RuleSelection, StaleRuleMatch,
};

use crate::{
    domain::{Direction, SessionPayload},
    http_sessions::HttpSessions,
    payload_text,
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RuleInput {
    pub source_id: i64,
    pub label: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default = "yes")]
    pub adjacent: bool,
    #[serde(default)]
    pub max_gap: usize,
    pub steps: Vec<Vec<Condition>>,
}
fn yes() -> bool {
    true
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AttackRule {
    pub id: i64,
    pub owner: String,
    #[serde(flatten)]
    pub input: RuleInput,
}

impl AttackRule {
    fn version(&self) -> String {
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        serde_json::to_string(&self.input).unwrap().hash(&mut hash);
        format!("{:016x}", hash.finish())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Condition {
    pub field: Field,
    pub op: Operator,
    pub value: String,
    /// Select a decoded query parameter. Omit to match the entire decoded query.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Field {
    Method,
    Path,
    Target,
    Query,
    RequestBody,
    Status,
    ResponseBody,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Operator {
    Equals,
    Contains,
    Regex,
}

pub struct CompiledRule {
    pub rule: AttackRule,
    steps: Vec<Vec<(Condition, Option<Regex>)>>,
    version: String,
}

pub struct MatchState<'a> {
    rule: &'a CompiledRule,
    active: Vec<Option<(usize, Vec<EventRef>)>>,
    index: usize,
    pub matched: Option<RuleMatch>,
}
impl<'a> MatchState<'a> {
    pub fn new(rule: &'a CompiledRule) -> Self {
        Self {
            rule,
            active: vec![None; rule.steps.len()],
            index: 0,
            matched: None,
        }
    }
    pub fn feed(&mut self, event: Option<&HttpEvent>) {
        if self.matched.is_some() {
            return;
        }
        let index = self.index;
        self.index += 1;
        let gap = if self.rule.rule.input.adjacent {
            0
        } else {
            self.rule.rule.input.max_gap
        };
        for candidates in &mut self.active {
            if candidates
                .as_ref()
                .is_some_and(|(previous, _)| index - previous > gap + 1)
            {
                *candidates = None;
            }
        }
        for step in (0..self.rule.steps.len()).rev() {
            let Some(event) = event else { continue };
            if !self.rule.step_matches(step, event) {
                continue;
            }
            let mut path = if step == 0 {
                Vec::new()
            } else if let Some((_, path)) = self.active[step - 1].as_ref() {
                path.clone()
            } else {
                continue;
            };
            path.push(event.reference.clone());
            if step + 1 == self.rule.steps.len() {
                self.matched = Some(RuleMatch {
                    version: self.rule.version(),
                    rule_id: self.rule.rule.id,
                    label: self.rule.rule.input.label.clone(),
                    events: path,
                });
                return;
            }
            // A later valid prefix dominates older prefixes for every future
            // event: only order and a maximum per-transition gap constrain it.
            self.active[step] = Some((index, path));
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct RuleMatch {
    pub version: String,
    pub rule_id: i64,
    pub label: String,
    pub events: Vec<EventRef>,
}
#[derive(Clone, Debug, Serialize)]
pub struct EventRef {
    pub session_id: i64,
    pub transaction: usize,
}

#[derive(Debug, Serialize)]
pub struct HttpEvent {
    #[serde(flatten)]
    pub reference: EventRef,
    pub method: String,
    pub path: String,
    pub target: String,
    pub query: String,
    pub query_params: Vec<(String, String)>,
    pub request_body: Option<String>,
    pub status: String,
    pub response_body: Option<String>,
}

impl CompiledRule {
    pub fn version(&self) -> String {
        self.version.clone()
    }
    pub fn new(rule: AttackRule) -> Result<Self> {
        let input = &rule.input;
        ensure!(input.source_id > 0, "source_id must be positive");
        ensure!(
            !input.label.trim().is_empty() && input.label.len() <= 160,
            "label must contain 1..160 bytes"
        );
        ensure!(
            (1..=8).contains(&input.steps.len()),
            "a rule must contain 1..8 steps"
        );
        ensure!(input.max_gap <= 100, "max_gap must not exceed 100 events");
        ensure!(
            !input.adjacent || input.max_gap == 0,
            "adjacent rules require max_gap=0"
        );
        let mut steps = Vec::new();
        for step in &input.steps {
            ensure!(
                (1..=12).contains(&step.len()),
                "each step must contain 1..12 conditions"
            );
            let mut conditions = Vec::new();
            for condition in step {
                ensure!(
                    condition.value.len() <= 2048 && (!condition.value.is_empty() || matches!(condition.op, Operator::Equals)),
                    "condition values must not exceed 2048 bytes; only equals accepts an empty value"
                );
                ensure!(
                    condition
                        .name
                        .as_ref()
                        .is_none_or(|n| matches!(condition.field, Field::Query)
                            && !n.is_empty()
                            && n.len() <= 128),
                    "name is only supported for query parameters (1..128 bytes)"
                );
                let regex = if matches!(condition.op, Operator::Regex) {
                    Some(
                        regex::RegexBuilder::new(&condition.value)
                            .size_limit(256 * 1024)
                            .dfa_size_limit(256 * 1024)
                            .build()
                            .map_err(|error| anyhow::anyhow!("invalid regex: {error}"))?,
                    )
                } else {
                    None
                };
                conditions.push((condition.clone(), regex));
            }
            steps.push(conditions);
        }
        let version = rule.version();
        Ok(Self {
            rule,
            steps,
            version,
        })
    }

    fn step_matches(&self, step: usize, event: &HttpEvent) -> bool {
        self.steps[step].iter().all(|(condition, regex)| {
            let test = |text: &str| match condition.op {
                Operator::Equals => text == condition.value,
                Operator::Contains => text.contains(&condition.value),
                Operator::Regex => regex.as_ref().unwrap().is_match(text),
            };
            match condition.field {
                Field::Method => test(&event.method),
                Field::Path => test(&event.path),
                Field::Target => test(&event.target),
                Field::Status => test(&event.status),
                Field::RequestBody => event.request_body.as_deref().is_some_and(test),
                Field::ResponseBody => event.response_body.as_deref().is_some_and(test),
                Field::Query => match &condition.name {
                    Some(name) => event
                        .query_params
                        .iter()
                        .any(|(key, value)| key == name && test(value)),
                    None => test(&event.query),
                },
            }
        })
    }

    /// Dynamic programming retains all viable starts; a greedy first B may hide
    /// the only B that can reach C within max_gap. State is bounded by rule size.
    pub fn find(&self, events: &[Option<HttpEvent>]) -> Option<RuleMatch> {
        let mut state = MatchState::new(self);
        for event in events {
            state.feed(event.as_ref());
        }
        state.matched
    }
}

/// Reuse the ingest framer for retained multi-request streams. Never split on
/// decoded content, guess response pairing or turn payload data into messages.
pub fn extract_events(id: i64, payload: &SessionPayload) -> Result<Vec<HttpEvent>> {
    let exchanges = retained_exchanges(payload);
    let mut events = Vec::new();
    for (transaction, exchange) in exchanges.into_iter().enumerate() {
        let Some(event) = extract_event(id, transaction, &exchange) else {
            return Ok(Vec::new());
        };
        events.push(event);
    }
    Ok(events)
}

fn retained_exchanges(payload: &SessionPayload) -> Vec<crate::http_sessions::Exchange> {
    if payload.c2s.is_empty() || payload.s2c.is_empty() {
        return Vec::new();
    }
    let mut parser = HttpSessions::new(payload.c2s.len().max(payload.s2c.len()));
    parser.feed(Direction::C2s, &payload.c2s, 0);
    let mut exchanges = parser.feed(Direction::S2c, &payload.s2c, 0);
    exchanges.extend(parser.close_response(true));
    // Unsupported/ambiguous retained streams remain opaque barriers. Do not let
    // one legacy record make every list request for its source fail.
    if parser.failed || parser.discarded_requests > 0 {
        return Vec::new();
    }
    exchanges
}

fn extract_event(
    id: i64,
    transaction: usize,
    exchange: &crate::http_sessions::Exchange,
) -> Option<HttpEvent> {
    if exchange.protocol != crate::domain::SessionProtocol::Http {
        return None;
    }
    let metadata = crate::protocol::classify(&exchange.c2s, &exchange.s2c).1;
    let (Some(method), Some(target), Some(status)) =
        (metadata.method, metadata.path, metadata.status)
    else {
        return None;
    };
    // Only origin-form paths are eligible; host:port is never a rule field.
    if !target.starts_with('/') {
        return None;
    }
    let (path, query) = target.split_once('?').unwrap_or((&target, ""));
    let query_params = query
        .split('&')
        .filter(|s| !s.is_empty())
        .map(|param| {
            let (name, value) = param.split_once('=').unwrap_or((param, ""));
            (payload_text::decode(name), payload_text::decode(value))
        })
        .collect();
    let request_body = body(&exchange.c2s);
    Some(HttpEvent {
        reference: EventRef {
            session_id: id,
            transaction,
        },
        method,
        path: payload_text::decode(path),
        target: payload_text::decode(&target),
        query: payload_text::decode(query),
        query_params,
        request_body,
        status: status.to_string(),
        response_body: body(&exchange.s2c),
    })
}

fn body(message: &[u8]) -> Option<String> {
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
    let bytes = &message[start..];
    if !chunked {
        return Some(payload_text::display(bytes, false));
    }
    let mut decoded = Vec::new();
    let mut rest = bytes;
    loop {
        let end = rest.windows(2).position(|s| s == b"\r\n")?;
        let size = std::str::from_utf8(&rest[..end]).ok()?.split(';').next()?;
        let size = usize::from_str_radix(size.trim(), 16).ok()?;
        rest = &rest[end + 2..];
        if size == 0 {
            return Some(payload_text::display(&decoded, false));
        }
        if size > rest.len() || rest.get(size..size.checked_add(2)?)? != b"\r\n" {
            return None;
        }
        decoded.extend_from_slice(&rest[..size]);
        rest = &rest[size + 2..];
    }
}

pub fn compile(rules: Vec<AttackRule>) -> Result<Vec<CompiledRule>> {
    rules
        .into_iter()
        .filter(|r| r.input.enabled)
        .map(CompiledRule::new)
        .collect()
}

pub fn validate(input: RuleInput) -> Result<RuleInput> {
    let rule = CompiledRule::new(AttackRule {
        id: 0,
        owner: String::new(),
        input,
    })
    .map_err(|error| RuleValidationError(error.to_string()))?;
    Ok(rule.rule.input)
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct RuleValidationError(String);

#[cfg(test)]
mod tests {
    use super::*;
    fn rule(steps: &[&str], adjacent: bool, max_gap: usize) -> CompiledRule {
        CompiledRule::new(AttackRule {
            id: 1,
            owner: "team".into(),
            input: RuleInput {
                source_id: 1,
                label: "test".into(),
                enabled: true,
                adjacent,
                max_gap,
                steps: steps
                    .iter()
                    .map(|path| {
                        vec![Condition {
                            field: Field::Path,
                            op: Operator::Equals,
                            value: path.to_string(),
                            name: None,
                        }]
                    })
                    .collect(),
            },
        })
        .unwrap()
    }
    fn event(path: &str, id: i64) -> Option<HttpEvent> {
        Some(HttpEvent {
            reference: EventRef {
                session_id: id,
                transaction: 0,
            },
            method: "GET".into(),
            path: path.into(),
            target: path.into(),
            query: String::new(),
            query_params: Vec::new(),
            request_body: Some(String::new()),
            status: "200".into(),
            response_body: Some(String::new()),
        })
    }
    #[test]
    fn matches_subsequence_with_noise_before_and_after() {
        let events = vec![
            event("/register", 1),
            event("/users", 2),
            event("/profile", 3),
            event("/admin", 4),
        ];
        let matched = rule(&["/users", "/profile"], true, 0)
            .find(&events)
            .unwrap();
        assert_eq!(
            matched
                .events
                .iter()
                .map(|e| e.session_id)
                .collect::<Vec<_>>(),
            vec![2, 3]
        );
        assert!(rule(&["/profile", "/users"], true, 0)
            .find(&events)
            .is_none());
    }
    #[test]
    fn distinguishes_adjacency_and_bounded_order_including_opaque_events() {
        let events = vec![event("/A", 1), None, event("/B", 3)];
        assert!(rule(&["/A", "/B"], true, 0).find(&events).is_none());
        assert!(rule(&["/A", "/B"], false, 1).find(&events).is_some());
        assert!(rule(&["/A", "/B"], false, 0).find(&events).is_none());
        assert!(rule(&["/A", "/A"], true, 0)
            .find(&[event("/A", 1)])
            .is_none());
    }
    #[test]
    fn retains_later_viable_middle_steps_and_expires_stale_prefixes() {
        let events = vec![
            event("/A", 1),
            event("/B", 2),
            event("/B", 3),
            event("/noise", 4),
            event("/C", 5),
        ];
        assert!(rule(&["/A", "/B", "/C"], false, 1).find(&events).is_some());
        assert!(rule(&["/A", "/C"], false, 1).find(&events).is_none());
    }
    #[test]
    fn extracts_pipelined_pairs_with_decoded_query_json_and_chunked_body() {
        let payload = SessionPayload {
            c2s: b"GET /users?id=%2E%2E%2F&id=safe HTTP/1.1\r\n\r\nGET /profile HTTP/1.1\r\n\r\n".to_vec(),
            s2c: b"HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\nHTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nroot:\r\n0\r\n\r\n".to_vec(),
        };
        let events = extract_events(7, &payload).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].path, "/users");
        assert_eq!(
            events[0].query_params,
            vec![("id".into(), "../".into()), ("id".into(), "safe".into())]
        );
        assert_eq!(events[1].response_body.as_deref(), Some("root:"));
        assert_eq!(events[1].reference.transaction, 1);
    }
    #[test]
    fn conditions_use_and_and_named_query_any_value() {
        let mut r = rule(&["/users"], true, 0).rule;
        r.input.steps[0].push(Condition {
            field: Field::Query,
            op: Operator::Contains,
            value: "../".into(),
            name: Some("id".into()),
        });
        r.input.steps[0].push(Condition {
            field: Field::Status,
            op: Operator::Regex,
            value: "^20[01]$".into(),
            name: None,
        });
        let r = CompiledRule::new(r).unwrap();
        let mut e = event("/users", 1).unwrap();
        e.query_params = vec![("other".into(), "../".into()), ("id".into(), "safe".into())];
        assert!(!r.step_matches(0, &e));
        e.query_params.push(("id".into(), "../../".into()));
        assert!(r.step_matches(0, &e));
        e.status = "500".into();
        assert!(!r.step_matches(0, &e));
    }
    #[test]
    fn rejects_bad_or_unbounded_rules() {
        let mut r = rule(&["/"], true, 0).rule;
        r.input.steps[0][0].op = Operator::Regex;
        r.input.steps[0][0].value = "[".into();
        assert!(CompiledRule::new(r).is_err());
        let mut r = rule(&["/"], true, 0).rule;
        r.input.steps = Vec::new();
        assert!(CompiledRule::new(r).is_err());
        let mut r = rule(&["/"], true, 0).rule;
        r.input.max_gap = 1;
        assert!(CompiledRule::new(r).is_err());
    }

    #[test]
    fn supports_large_header_counts_and_empty_equals_without_decompressing() {
        let request = format!("GET / HTTP/1.1\r\n{}\r\n", "X-Test: value\r\n".repeat(80));
        let payload = SessionPayload {
            c2s: request.into_bytes(),
            s2c: b"HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: 4\r\n\r\nDATA"
                .to_vec(),
        };
        let events = extract_events(1, &payload).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].request_body.as_deref(), Some(""));
        assert!(events[0].response_body.is_none());
        let mut r = rule(&["/"], true, 0).rule;
        r.input.steps[0].push(Condition {
            field: Field::RequestBody,
            op: Operator::Equals,
            value: String::new(),
            name: None,
        });
        assert!(CompiledRule::new(r).unwrap().step_matches(0, &events[0]));
    }
}
