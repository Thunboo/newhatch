use std::{
    collections::{BTreeMap, HashMap},
    net::IpAddr,
    time::{Duration, Instant},
};

use regex::bytes::Regex;
use tokio::sync::mpsc;

use crate::{
    domain::{CompletedSession, Direction, FlagDirection, FlowKey, SessionProtocol},
    http_sessions::{Exchange, HttpSessions},
    packet::ClassifiedPacket,
    protocol,
    storage::StorageHandle,
};

pub struct FlowRuntime {
    pub senders: Vec<mpsc::Sender<IngressPacket>>,
}

#[derive(Clone, Debug)]
pub struct IngressPacket {
    pub collector_id: String,
    pub packet: ClassifiedPacket,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct CollectorFlowKey {
    collector_id: String,
    flow_key: FlowKey,
}

pub struct FlowOptions {
    pub workers: usize,
    pub queue_capacity: usize,
    pub idle_timeout: Duration,
    pub max_active_flows_per_worker: usize,
    pub max_stream_bytes: usize,
    pub flag_regex: Regex,
}

pub fn start_workers(options: FlowOptions, storage: StorageHandle) -> FlowRuntime {
    let mut senders = Vec::with_capacity(options.workers);
    for worker_id in 0..options.workers {
        let (sender, receiver) = mpsc::channel(options.queue_capacity);
        let worker = FlowWorker {
            worker_id,
            flows: HashMap::new(),
            receiver,
            storage: storage.clone(),
            flag_regex: options.flag_regex.clone(),
            idle_timeout: options.idle_timeout,
            max_active_flows: options.max_active_flows_per_worker,
            max_stream_bytes: options.max_stream_bytes,
            dropped_flows: 0,
            dropped_sessions: 0,
            discarded_http: 0,
        };
        tokio::spawn(worker.run());
        senders.push(sender);
    }
    FlowRuntime { senders }
}

pub fn worker_index(collector_id: &str, key: &FlowKey, workers: usize) -> usize {
    let mut hash = 0xcbf29ce484222325u64;
    mix_bytes(&mut hash, collector_id.as_bytes());
    mix_endpoint(&mut hash, &key.client);
    mix_endpoint(&mut hash, &key.server);
    (hash as usize) % workers
}

fn mix_endpoint(hash: &mut u64, endpoint: &crate::domain::Endpoint) {
    match endpoint.ip {
        IpAddr::V4(value) => mix_bytes(hash, &value.octets()),
        IpAddr::V6(value) => mix_bytes(hash, &value.octets()),
    }
    mix_bytes(hash, &endpoint.port.to_be_bytes());
}

fn mix_bytes(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(0x100000001b3);
    }
}

struct FlowWorker {
    worker_id: usize,
    flows: HashMap<CollectorFlowKey, FlowState>,
    receiver: mpsc::Receiver<IngressPacket>,
    storage: StorageHandle,
    flag_regex: Regex,
    idle_timeout: Duration,
    max_active_flows: usize,
    max_stream_bytes: usize,
    dropped_flows: u64,
    dropped_sessions: u64,
    discarded_http: u64,
}

impl FlowWorker {
    async fn run(mut self) {
        let mut expiration = tokio::time::interval(Duration::from_secs(1));
        expiration.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                packet = self.receiver.recv() => match packet {
                    Some(packet) => self.handle_packet(packet),
                    None => break,
                },
                _ = expiration.tick() => self.expire_idle(),
            }
        }
        let remaining = self.flows.drain().map(|(_, flow)| flow).collect::<Vec<_>>();
        for flow in remaining {
            self.store_all(flow.finish(true, &self.flag_regex))
        }
    }

    fn handle_packet(&mut self, ingress: IngressPacket) {
        let classified = ingress.packet;
        let key = CollectorFlowKey {
            collector_id: ingress.collector_id,
            flow_key: classified.flow_key.clone(),
        };
        // A reused endpoint tuple is not the same TCP connection. Retransmitted
        // SYNs keep the same ISN; a new client ISN starts a new assembly state.
        if self
            .flows
            .get(&key)
            .is_some_and(|flow| flow.is_new_connection(&classified))
        {
            if let Some(flow) = self.flows.remove(&key) {
                self.store_all(flow.finish(true, &self.flag_regex));
            }
        }
        if !self.flows.contains_key(&key) && self.flows.len() >= self.max_active_flows {
            self.dropped_flows = self.dropped_flows.saturating_add(1);
            if self.dropped_flows.is_power_of_two() {
                tracing::warn!(
                    worker = self.worker_id,
                    dropped = self.dropped_flows,
                    "active flow limit reached"
                );
            }
            return;
        }
        let (ready, should_finish, discarded_http) = {
            let flow = self.flows.entry(key.clone()).or_insert_with(|| {
                FlowState::new(
                    key.collector_id.clone(),
                    classified.source_id,
                    key.flow_key.clone(),
                    classified.packet.timestamp_micros,
                    self.max_stream_bytes,
                )
            });
            let previous = flow.discarded_http();
            let ready = flow.ingest(&classified, &self.flag_regex);
            (
                ready,
                classified.packet.rst || flow.closed(),
                flow.discarded_http().saturating_sub(previous),
            )
        };
        if discarded_http != 0 {
            let previous = self.discarded_http;
            self.discarded_http = self.discarded_http.saturating_add(discarded_http);
            if previous == 0 || self.discarded_http.is_power_of_two() {
                tracing::warn!(
                    worker = self.worker_id,
                    discarded = self.discarded_http,
                    "HTTP retention or association limit reached; discarding affected exchanges"
                );
            }
        }
        self.store_all(ready);
        if should_finish {
            if let Some(flow) = self.flows.remove(&key) {
                self.store_all(flow.finish(false, &self.flag_regex));
            }
        }
    }

    fn expire_idle(&mut self) {
        let now = Instant::now();
        let admission_pressure = self.flows.len() >= self.max_active_flows;
        let expired = self
            .flows
            .iter()
            .filter_map(|(key, flow)| {
                (flow.expired(now, self.idle_timeout)
                    || (admission_pressure
                        && flow.idle_http_context()
                        && now.saturating_duration_since(flow.last_payload) >= self.idle_timeout))
                    .then_some(key.clone())
            })
            .collect::<Vec<_>>();
        for key in expired {
            if let Some(flow) = self.flows.remove(&key) {
                self.store_all(flow.finish(true, &self.flag_regex));
            }
        }
    }

    fn store_all(&mut self, sessions: Vec<CompletedSession>) {
        for session in sessions {
            if !self.storage.try_store(session) {
                self.dropped_sessions = self.dropped_sessions.saturating_add(1);
                if self.dropped_sessions.is_power_of_two() {
                    tracing::error!(
                        worker = self.worker_id,
                        dropped = self.dropped_sessions,
                        "storage queue is saturated; dropping finalized session"
                    );
                }
            }
        }
    }
}

struct Span {
    end: usize,
    timestamp: i64,
    last_timestamp: i64,
}

#[derive(Default)]
struct Probe {
    scanned: usize,
    line_end: Option<usize>,
    header_end: Option<usize>,
}

impl Probe {
    fn inspect(&mut self, bytes: &[u8]) {
        let start = self.scanned.saturating_sub(3);
        if self.line_end.is_none() {
            self.line_end = bytes[start..]
                .iter()
                .position(|byte| *byte == b'\n')
                .map(|index| start + index);
        }
        if self.header_end.is_none() {
            self.header_end = bytes[start..]
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .map(|index| start + index + 4)
                .or_else(|| {
                    bytes[start..]
                        .windows(2)
                        .position(|window| window == b"\n\n")
                        .map(|index| start + index + 2)
                });
        }
        self.scanned = bytes.len();
    }
}

struct FlowState {
    collector_id: String,
    first_payload_c2s_at: Option<i64>,
    first_payload_s2c_at: Option<i64>,
    source_id: i64,
    key: FlowKey,
    started_at: i64,
    ended_at: i64,
    last_seen: Instant,
    last_payload: Instant,
    ordered: [TcpAssembler; 2],
    syn: [Option<u32>; 2],
    c2s: StreamAssembler,
    s2c: StreamAssembler,
    spans: [Vec<Span>; 2],
    probe: [Probe; 2],
    http: Option<HttpSessions>,
    raw_decided: bool,
    suppressed: bool,
    response_eof: bool,
    max_stream_bytes: usize,
    c2s_flag_count: usize,
    s2c_flag_count: usize,
    c2s_scanned_to: usize,
    s2c_scanned_to: usize,
}

impl FlowState {
    fn new(
        collector_id: String,
        source_id: i64,
        key: FlowKey,
        timestamp: i64,
        max_stream_bytes: usize,
    ) -> Self {
        Self {
            collector_id,
            source_id,
            key,
            first_payload_c2s_at: None,
            first_payload_s2c_at: None,
            started_at: timestamp,
            ended_at: timestamp,
            last_seen: Instant::now(),
            last_payload: Instant::now(),
            ordered: [
                TcpAssembler::new(max_stream_bytes),
                TcpAssembler::new(max_stream_bytes),
            ],
            syn: [None, None],
            c2s: StreamAssembler::new(max_stream_bytes),
            s2c: StreamAssembler::new(max_stream_bytes),
            spans: [Vec::new(), Vec::new()],
            probe: [Probe::default(), Probe::default()],
            http: None,
            raw_decided: false,
            suppressed: false,
            response_eof: false,
            max_stream_bytes,
            c2s_flag_count: 0,
            s2c_flag_count: 0,
            c2s_scanned_to: 0,
            s2c_scanned_to: 0,
        }
    }

    fn is_new_connection(&self, packet: &ClassifiedPacket) -> bool {
        if !packet.packet.syn {
            return false;
        }
        match packet.direction {
            Direction::C2s => match self.syn[0] {
                Some(sequence) => sequence != packet.packet.sequence,
                None => self.first_payload_c2s_at.is_some() || self.first_payload_s2c_at.is_some(),
            },
            Direction::S2c => {
                self.syn[1].is_some_and(|sequence| sequence != packet.packet.sequence)
            }
        }
    }

    fn expired(&self, now: Instant, timeout: Duration) -> bool {
        if self.idle_http_context() {
            return false;
        }
        let probing_http = self.http.is_none()
            && !self.raw_decided
            && !self.suppressed
            && (self.probe[0]
                .line_end
                .is_some_and(|end| http_version_line(&self.c2s.bytes()[..end]))
                || self.s2c.bytes().starts_with(b"HTTP/1."));
        let last = if self.http.as_ref().is_some_and(|http| !http.is_websocket())
            || self.suppressed
            || probing_http
        {
            self.last_payload
        } else {
            self.last_seen
        };
        now.saturating_duration_since(last) >= timeout
    }

    fn idle_http_context(&self) -> bool {
        self.http
            .as_ref()
            .is_some_and(|http| !http.is_websocket() && !http.has_pending())
            && self
                .ordered
                .iter()
                .all(|stream| !stream.has_gap() && stream.fin_at.is_none())
    }

    fn closed(&self) -> bool {
        self.ordered.iter().all(TcpAssembler::closed)
    }

    fn discarded_http(&self) -> u64 {
        u64::from(self.suppressed)
            + self.http.as_ref().map_or(0, |http| {
                http.discarded_requests
                    .saturating_add(u64::from(http.failed))
            })
    }

    fn ingest(
        &mut self,
        classified: &ClassifiedPacket,
        flag_regex: &Regex,
    ) -> Vec<CompletedSession> {
        let packet = &classified.packet;
        self.last_seen = Instant::now();
        self.started_at = self.started_at.min(packet.timestamp_micros);
        self.ended_at = self.ended_at.max(packet.timestamp_micros);
        let index = direction_index(classified.direction);
        if !packet.payload.is_empty() {
            let first = if index == 0 {
                &mut self.first_payload_c2s_at
            } else {
                &mut self.first_payload_s2c_at
            };
            *first = Some(first.map_or(packet.timestamp_micros, |time| {
                time.min(packet.timestamp_micros)
            }));
        }
        if packet.syn && self.syn[index].is_none() {
            self.syn[index] = Some(packet.sequence);
            self.ordered[index].seed(packet.sequence.wrapping_add(1));
        }
        let sequence = packet.sequence.wrapping_add(u32::from(packet.syn));
        let mut ready = Vec::new();
        // Move the small transport state out temporarily to let its callback
        // feed ordered slices directly, without copying every captured packet.
        let mut assembler = std::mem::replace(
            &mut self.ordered[index],
            TcpAssembler::new(self.max_stream_bytes),
        );
        let accepted = assembler.push(
            sequence,
            &packet.payload,
            packet.timestamp_micros,
            |sequence, bytes, timestamp| {
                self.consume(
                    classified.direction,
                    sequence,
                    bytes,
                    timestamp,
                    flag_regex,
                    &mut ready,
                );
            },
        );
        if packet.fin {
            assembler.fin(sequence.wrapping_add(packet.payload.len() as u32))
        }
        self.ordered[index] = assembler;
        if accepted {
            self.last_payload = Instant::now()
        }
        if packet.syn
            && packet.payload.is_empty()
            && self.syn.iter().all(Option::is_some)
            && self.http.is_none()
            && !self.raw_decided
            && !self.suppressed
        {
            // TCP Fast Open can carry a request in the client SYN. Start its
            // preprocessor once the server sequence anchor is also observed.
            self.consume(
                classified.direction,
                sequence,
                b"",
                packet.timestamp_micros,
                flag_regex,
                &mut ready,
            );
        }
        if self.ordered[1].closed() && !self.response_eof {
            self.response_eof = true;
            if let Some(http) = &mut self.http {
                let exchanges = http.close_response(true);
                for exchange in exchanges {
                    ready.push(self.make_session(exchange, flag_regex))
                }
            }
        }
        ready
    }

    fn consume(
        &mut self,
        direction: Direction,
        sequence: u32,
        bytes: &[u8],
        timestamp: i64,
        regex: &Regex,
        ready: &mut Vec<CompletedSession>,
    ) {
        if self.suppressed {
            return;
        }
        if let Some(http) = &mut self.http {
            let exchanges = http.feed(direction, bytes, timestamp);
            for exchange in exchanges {
                ready.push(self.make_session(exchange, regex))
            }
            return;
        }
        let index = direction_index(direction);
        let (stream, count, scanned) = if index == 0 {
            (
                &mut self.c2s,
                &mut self.c2s_flag_count,
                &mut self.c2s_scanned_to,
            )
        } else {
            (
                &mut self.s2c,
                &mut self.s2c_flag_count,
                &mut self.s2c_scanned_to,
            )
        };
        let previous_length = stream.bytes.len();
        if stream.push(sequence, bytes) {
            *count += scan_new_matches(regex, stream.bytes(), scanned);
            if !self.raw_decided {
                if self.spans[index].len() >= 256 {
                    // Consolidate only the oldest header spans; the newest span
                    // can contain the header boundary and later messages.
                    let next = self.spans[index].remove(1);
                    let first = &mut self.spans[index][0];
                    first.end = next.end;
                    first.timestamp = first.timestamp.min(next.timestamp);
                    first.last_timestamp = first.last_timestamp.max(next.last_timestamp);
                }
                self.spans[index].push(Span {
                    end: stream.bytes.len(),
                    timestamp,
                    last_timestamp: timestamp,
                })
            }
        }
        let retained_input = stream.bytes.len() - previous_length;
        if self.raw_decided {
            return;
        }
        self.probe[index].inspect(if index == 0 {
            self.c2s.bytes()
        } else {
            self.s2c.bytes()
        });
        let c2s = self.c2s.bytes();
        let s2c = self.s2c.bytes();
        let request_http = http_version_line(&c2s[..self.probe[0].line_end.unwrap_or(c2s.len())]);
        let response_http = s2c.starts_with(b"HTTP/1.");
        let headers_ready = (request_http && self.probe[0].header_end.is_some())
            || (response_http && self.probe[1].header_end.is_some());
        if headers_ready {
            if protocol::classify(c2s, s2c).0 == SessionProtocol::WebSocket {
                self.raw_decided = true;
                self.spans = [Vec::new(), Vec::new()];
                return;
            }
            // Without both initial sequence anchors, an apparently first response
            // could belong to an unseen request from before capture began.
            if self.syn.iter().any(Option::is_none) {
                if self.syn[0].is_some() && self.syn[1].is_none() {
                    return;
                }
                self.suppressed = true;
                self.c2s = StreamAssembler::new(self.max_stream_bytes);
                self.s2c = StreamAssembler::new(self.max_stream_bytes);
                self.spans = [Vec::new(), Vec::new()];
                return;
            }
            let mut http = HttpSessions::new(self.max_stream_bytes);
            for index in 0..2 {
                let stream = if index == 0 {
                    &mut self.c2s
                } else {
                    &mut self.s2c
                };
                let bytes = std::mem::take(&mut stream.bytes);
                let spans = std::mem::take(&mut self.spans[index]);
                let mut start = 0;
                for span in spans {
                    let exchanges = http.feed(
                        if index == 0 {
                            Direction::C2s
                        } else {
                            Direction::S2c
                        },
                        &bytes[start..span.end],
                        span.timestamp,
                    );
                    http.observe_probe_time(
                        if index == 0 {
                            Direction::C2s
                        } else {
                            Direction::S2c
                        },
                        span.last_timestamp,
                    );
                    start = span.end;
                    for exchange in exchanges {
                        ready.push(self.make_session(exchange, regex))
                    }
                }
            }
            if retained_input < bytes.len() {
                let exchanges = http.feed(direction, &bytes[retained_input..], timestamp);
                for exchange in exchanges {
                    ready.push(self.make_session(exchange, regex))
                }
            }
            self.http = Some(http);
        } else if (!request_http && self.probe[0].line_end.is_some())
            || (c2s.len() >= 64 * 1024 && !http_request_hint(c2s))
        {
            self.raw_decided = true;
            self.spans = [Vec::new(), Vec::new()];
        }
    }

    fn make_session(&self, exchange: Exchange, regex: &Regex) -> CompletedSession {
        let c2s_flags = regex.find_iter(&exchange.c2s).count();
        let s2c_flags = regex.find_iter(&exchange.s2c).count();
        let (_, http) = protocol::classify(&exchange.c2s, &exchange.s2c);
        CompletedSession {
            collector_id: self.collector_id.clone(),
            source_id: self.source_id,
            first_payload_c2s_at: exchange.request_times.first,
            first_payload_s2c_at: exchange.response_times.first,
            started_at: exchange.request_times.first.unwrap_or(self.started_at),
            ended_at: exchange
                .request_times
                .last
                .into_iter()
                .chain(exchange.response_times.last)
                .max()
                .unwrap_or(self.ended_at),
            client_ip: self.key.client.ip,
            client_port: self.key.client.port,
            server_ip: self.key.server.ip,
            server_port: self.key.server.port,
            protocol: exchange.protocol,
            c2s: exchange.c2s,
            s2c: exchange.s2c,
            flag_direction: FlagDirection::from_counts(c2s_flags, s2c_flags),
            flag_count: c2s_flags + s2c_flags,
            incomplete: exchange.incomplete,
            http,
        }
    }

    fn finish(mut self, timed_out: bool, regex: &Regex) -> Vec<CompletedSession> {
        if self.suppressed {
            return Vec::new();
        }
        if let Some(http) = self.http.take() {
            let exchanges = http.finish(
                self.ordered[1].closed() && !self.ordered[1].has_gap(),
                timed_out || !self.closed() || self.ordered.iter().any(TcpAssembler::has_gap),
            );
            return exchanges
                .into_iter()
                .map(|exchange| self.make_session(exchange, regex))
                .collect();
        }
        let c2s_truncated = self.c2s.truncated;
        let s2c_truncated = self.s2c.truncated;
        let c2s = self.c2s.into_bytes();
        let s2c = self.s2c.into_bytes();
        let (protocol, http) = protocol::classify(&c2s, &s2c);
        // Recognized HTTP must never escape as an unsplit or one-sided legacy row.
        if protocol != SessionProtocol::WebSocket
            && (protocol == SessionProtocol::Http
                || http_request_line(&c2s)
                || s2c.starts_with(b"HTTP/1.")
                || (c2s_truncated && !self.raw_decided && http_request_hint(&c2s)))
        {
            return Vec::new();
        }
        let c2s_flags = regex.find_iter(&c2s).count();
        let s2c_flags = regex.find_iter(&s2c).count();
        if c2s_flags != self.c2s_flag_count || s2c_flags != self.s2c_flag_count {
            tracing::debug!(
                streaming_c2s = self.c2s_flag_count,
                streaming_s2c = self.s2c_flag_count,
                exact_c2s = c2s_flags,
                exact_s2c = s2c_flags,
                "final flag scan corrected streaming counts"
            );
        }
        vec![CompletedSession {
            collector_id: self.collector_id,
            source_id: self.source_id,
            first_payload_c2s_at: self.first_payload_c2s_at,
            first_payload_s2c_at: self.first_payload_s2c_at,
            started_at: self.started_at,
            ended_at: self.ended_at,
            client_ip: self.key.client.ip,
            client_port: self.key.client.port,
            server_ip: self.key.server.ip,
            server_port: self.key.server.port,
            protocol,
            c2s,
            s2c,
            flag_direction: FlagDirection::from_counts(c2s_flags, s2c_flags),
            flag_count: c2s_flags + s2c_flags,
            incomplete: timed_out
                || c2s_truncated
                || s2c_truncated
                || !self.ordered.iter().all(TcpAssembler::closed)
                || self.ordered.iter().any(TcpAssembler::has_gap),
            http,
        }]
    }
}

fn direction_index(direction: Direction) -> usize {
    match direction {
        Direction::C2s => 0,
        Direction::S2c => 1,
    }
}

fn http_request_line(bytes: &[u8]) -> bool {
    let line = bytes
        .split(|byte| *byte == b'\n')
        .next()
        .unwrap_or_default();
    http_version_line(line)
}

fn http_version_line(line: &[u8]) -> bool {
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    line.ends_with(b" HTTP/1.1") || line.ends_with(b" HTTP/1.0")
}

fn http_request_hint(bytes: &[u8]) -> bool {
    let Some(space) = bytes.iter().take(32).position(|byte| *byte == b' ') else {
        return false;
    };
    space != 0
        && bytes[..space]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(byte))
        && (bytes[space + 1..].starts_with(b"/")
            || bytes[space + 1..].starts_with(b"*")
            || bytes[space + 1..].starts_with(b"http")
            || bytes.starts_with(b"CONNECT "))
}

struct TcpChunk {
    bytes: Vec<u8>,
    timestamp: i64,
}

struct TcpAssembler {
    next: Option<u32>,
    pending: BTreeMap<u32, TcpChunk>,
    pending_bytes: usize,
    limit: usize,
    fin_at: Option<u32>,
    failed: bool,
}

impl TcpAssembler {
    fn new(limit: usize) -> Self {
        Self {
            next: None,
            pending: BTreeMap::new(),
            pending_bytes: 0,
            limit,
            fin_at: None,
            failed: false,
        }
    }

    fn seed(&mut self, sequence: u32) {
        self.next.get_or_insert(sequence);
    }

    fn push(
        &mut self,
        mut sequence: u32,
        mut bytes: &[u8],
        timestamp: i64,
        mut consume: impl FnMut(u32, &[u8], i64),
    ) -> bool {
        if self.failed || bytes.is_empty() {
            return false;
        }
        let next = *self.next.get_or_insert(sequence);
        if sequence_before(sequence, next) {
            let overlap = next.wrapping_sub(sequence) as usize;
            if overlap >= bytes.len() {
                return false;
            }
            sequence = next;
            bytes = &bytes[overlap..];
        }
        let start = sequence.wrapping_sub(next) as u64;
        let end = start + bytes.len() as u64;
        if self
            .fin_at
            .is_some_and(|fin| sequence_before(fin, sequence.wrapping_add(bytes.len() as u32)))
        {
            self.failed = true;
            return false;
        }
        for (other_sequence, chunk) in &self.pending {
            let other_start = other_sequence.wrapping_sub(next) as u64;
            let from = start.max(other_start);
            let to = end.min(other_start + chunk.bytes.len() as u64);
            if from < to
                && bytes[(from - start) as usize..(to - start) as usize]
                    != chunk.bytes[(from - other_start) as usize..(to - other_start) as usize]
            {
                self.failed = true;
                return false;
            }
        }
        if sequence != next {
            if let Some(old) = self.pending.get_mut(&sequence) {
                let overlap = old.bytes.len().min(bytes.len());
                if old.bytes[..overlap] != bytes[..overlap] {
                    self.failed = true;
                    return false;
                }
                if bytes.len() <= old.bytes.len() {
                    return false;
                }
                let extra = bytes.len() - old.bytes.len();
                if self.pending_bytes.saturating_add(extra) > self.limit {
                    self.failed = true;
                    return false;
                }
                old.bytes.extend_from_slice(&bytes[overlap..]);
                old.timestamp = old.timestamp.min(timestamp);
                self.pending_bytes += extra;
                return true;
            }
            if self.pending.len() >= 256
                || self.pending_bytes.saturating_add(bytes.len()) > self.limit
            {
                self.failed = true;
                return false;
            }
            self.pending.insert(
                sequence,
                TcpChunk {
                    bytes: bytes.to_vec(),
                    timestamp,
                },
            );
            self.pending_bytes += bytes.len();
            return true;
        }
        consume(sequence, bytes, timestamp);
        self.next = Some(next.wrapping_add(bytes.len() as u32));
        loop {
            let next = self.next.expect("seeded stream");
            // Compare relative sequence distances, not BTreeMap numeric order:
            // an otherwise valid stream can cross u32::MAX.
            let candidate = self
                .pending
                .keys()
                .filter(|sequence| !sequence_before(next, **sequence))
                .min_by_key(|sequence| next.wrapping_sub(**sequence))
                .copied();
            let Some(sequence) = candidate else { break };
            let chunk = self.pending.remove(&sequence).expect("pending candidate");
            self.pending_bytes -= chunk.bytes.len();
            let overlap = next.wrapping_sub(sequence) as usize;
            if overlap < chunk.bytes.len() {
                consume(next, &chunk.bytes[overlap..], chunk.timestamp);
                self.next = Some(next.wrapping_add((chunk.bytes.len() - overlap) as u32));
            }
        }
        true
    }

    fn fin(&mut self, sequence: u32) {
        if self.fin_at.is_some_and(|old| old != sequence) {
            self.failed = true
        }
        self.fin_at = Some(sequence);
        if self
            .next
            .is_some_and(|next| sequence_before(sequence, next))
            || self.pending.iter().any(|(start, chunk)| {
                sequence_before(sequence, start.wrapping_add(chunk.bytes.len() as u32))
            })
        {
            self.failed = true;
        }
    }

    fn closed(&self) -> bool {
        !self.failed && self.fin_at.is_some() && self.fin_at == self.next
    }
    fn has_gap(&self) -> bool {
        self.failed || !self.pending.is_empty() || (self.fin_at.is_some() && !self.closed())
    }
}

fn scan_new_matches(regex: &Regex, bytes: &[u8], scanned_to: &mut usize) -> usize {
    const OVERLAP_BYTES: usize = 4 * 1024;
    let previous_end = (*scanned_to).min(bytes.len());
    let window_start = previous_end.saturating_sub(OVERLAP_BYTES);
    let count = regex
        .find_iter(&bytes[window_start..])
        .filter(|found| window_start + found.end() > previous_end)
        .count();
    *scanned_to = bytes.len();
    count
}

struct StreamAssembler {
    next_sequence: Option<u32>,
    pending: BTreeMap<u32, Vec<u8>>,
    bytes: Vec<u8>,
    max_bytes: usize,
    truncated: bool,
}

impl StreamAssembler {
    fn new(max_bytes: usize) -> Self {
        Self {
            next_sequence: None,
            pending: BTreeMap::new(),
            bytes: Vec::new(),
            max_bytes,
            truncated: false,
        }
    }

    fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    fn push(&mut self, mut sequence: u32, mut payload: &[u8]) -> bool {
        if payload.is_empty() || self.truncated {
            self.next_sequence.get_or_insert(sequence);
            return false;
        }
        let next = *self.next_sequence.get_or_insert(sequence);
        if sequence_before(sequence, next) {
            let overlap = next.wrapping_sub(sequence) as usize;
            if overlap >= payload.len() {
                return false;
            }
            sequence = next;
            payload = &payload[overlap..];
        }
        if sequence != next {
            if self.pending.len() < 256 {
                self.pending
                    .entry(sequence)
                    .or_insert_with(|| payload.to_vec());
            } else {
                self.truncated = true;
            }
            return false;
        }

        let mut changed = self.append_contiguous(payload);
        loop {
            let next = self.next_sequence.expect("set after append");
            let candidate = self
                .pending
                .range(..=next)
                .next_back()
                .map(|(sequence, _)| *sequence)
                .or_else(|| self.pending.contains_key(&next).then_some(next));
            let Some(candidate) = candidate else { break };
            let mut queued = self.pending.remove(&candidate).expect("candidate exists");
            let overlap = next.wrapping_sub(candidate) as usize;
            if overlap >= queued.len() {
                continue;
            }
            if overlap > 0 {
                queued.drain(..overlap);
            }
            changed |= self.append_contiguous(&queued);
        }
        changed
    }

    fn append_contiguous(&mut self, payload: &[u8]) -> bool {
        let available = self.max_bytes.saturating_sub(self.bytes.len());
        let accepted = payload.len().min(available);
        if accepted == 0 {
            self.truncated = true;
            return false;
        }
        crate::http_sessions::append_bounded(&mut self.bytes, &payload[..accepted], self.max_bytes);
        let next = self.next_sequence.unwrap_or_default();
        self.next_sequence = Some(next.wrapping_add(payload.len() as u32));
        if accepted < payload.len() {
            self.truncated = true;
        }
        true
    }
}

fn sequence_before(left: u32, right: u32) -> bool {
    (left.wrapping_sub(right) as i32) < 0
}

#[cfg(test)]
mod tests {
    use crate::domain::{Endpoint, FlowKey};
    use crate::packet::{ClassifiedPacket, TcpPacket};
    use newhatch_protocol::{Direction, PacketMessage};
    use regex::bytes::Regex;

    use super::{scan_new_matches, worker_index, CollectorFlowKey, FlowState, StreamAssembler};

    #[test]
    fn worker_routing_includes_collector_identity() {
        let key = FlowKey {
            client: Endpoint {
                ip: "10.0.0.2".parse().unwrap(),
                port: 50000,
            },
            server: Endpoint {
                ip: "10.0.0.1".parse().unwrap(),
                port: 8080,
            },
        };
        assert_eq!(worker_index("one", &key, 32), worker_index("one", &key, 32));
        assert_ne!(
            CollectorFlowKey {
                collector_id: "one".into(),
                flow_key: key.clone()
            },
            CollectorFlowKey {
                collector_id: "two".into(),
                flow_key: key
            },
        );
    }

    #[test]
    fn reassembles_out_of_order_and_ignores_retransmit() {
        let mut stream = StreamAssembler::new(1024);
        assert!(stream.push(100, b"hello "));
        assert!(!stream.push(112, b"world"));
        assert!(stream.push(106, b"brave "));
        assert!(!stream.push(100, b"hello "));
        assert_eq!(stream.bytes(), b"hello brave world");
    }

    #[test]
    fn reassembles_out_of_order_after_wire_round_trip() {
        let key = FlowKey {
            client: Endpoint {
                ip: "10.0.0.2".parse().unwrap(),
                port: 50000,
            },
            server: Endpoint {
                ip: "10.0.0.1".parse().unwrap(),
                port: 8080,
            },
        };
        let regex = Regex::new("FLAG").unwrap();
        let mut flow = FlowState::new("test-box".into(), 1, key.clone(), 1, 1024);
        for (sequence, payload) in [
            (100, b"hello ".as_slice()),
            (112, b"world"),
            (106, b"brave "),
        ] {
            let packet = ClassifiedPacket {
                source_id: 1,
                direction: Direction::C2s,
                flow_key: key.clone(),
                packet: TcpPacket {
                    timestamp_micros: 1,
                    src: key.client.clone(),
                    dst: key.server.clone(),
                    sequence,
                    syn: false,
                    fin: false,
                    rst: false,
                    payload: payload.to_vec(),
                },
            };
            let decoded = ClassifiedPacket::try_from(PacketMessage::from(&packet)).unwrap();
            flow.ingest(&decoded, &regex);
        }
        assert_eq!(
            flow.finish(true, &regex).remove(0).c2s,
            b"hello brave world"
        );
    }

    #[test]
    fn session_metadata_uses_capture_time_and_preserves_collector() {
        let key = FlowKey {
            client: Endpoint {
                ip: "10.0.0.2".parse().unwrap(),
                port: 50000,
            },
            server: Endpoint {
                ip: "10.0.0.1".parse().unwrap(),
                port: 8080,
            },
        };
        let regex = Regex::new("FLAG").unwrap();
        let mut flow = FlowState::new("vulnbox-2".into(), 1, key.clone(), 100, 1024);
        for (direction, timestamp, sequence, payload) in [
            (Direction::C2s, 100, 10, b"".as_slice()),
            (Direction::C2s, 500, 10, b"request"),
            (Direction::S2c, 700, 20, b"FLAG"),
            (Direction::C2s, 400, 10, b"request"),
            (Direction::S2c, 50, 24, b""),
        ] {
            flow.ingest(
                &ClassifiedPacket {
                    source_id: 1,
                    direction,
                    flow_key: key.clone(),
                    packet: TcpPacket {
                        timestamp_micros: timestamp,
                        src: key.client.clone(),
                        dst: key.server.clone(),
                        sequence,
                        syn: false,
                        fin: false,
                        rst: false,
                        payload: payload.to_vec(),
                    },
                },
                &regex,
            );
        }
        let session = flow.finish(true, &regex).remove(0);
        assert_eq!(session.collector_id, "vulnbox-2");
        assert_eq!(session.started_at, 50);
        assert_eq!(session.first_payload_c2s_at, Some(400));
        assert_eq!(session.first_payload_s2c_at, Some(700));
        assert_eq!(session.c2s, b"request");
        let no_payload = FlowState::new("local".into(), 1, key, 1, 1024)
            .finish(true, &regex)
            .remove(0);
        assert_eq!(no_payload.first_payload_c2s_at, None);
        assert_eq!(no_payload.first_payload_s2c_at, None);
    }

    #[test]
    fn marks_stream_truncated_at_limit() {
        let mut stream = StreamAssembler::new(4);
        assert!(stream.push(10, b"abcdef"));
        assert_eq!(stream.bytes(), b"abcd");
        assert!(stream.truncated);
    }

    #[test]
    fn incremental_scan_detects_a_flag_split_across_chunks() {
        let regex = Regex::new(r"FLAG\{[^}]+\}").unwrap();
        let mut stream = StreamAssembler::new(1024);
        let mut scanned_to = 0;
        stream.push(10, b"prefix FLAG{ABC");
        assert_eq!(scan_new_matches(&regex, stream.bytes(), &mut scanned_to), 0);
        stream.push(25, b"DEF} suffix");
        assert_eq!(scan_new_matches(&regex, stream.bytes(), &mut scanned_to), 1);
    }
}

#[cfg(test)]
mod atomic_tests {
    use super::*;
    use crate::domain::{Endpoint, SourceInput};
    use crate::packet::TcpPacket;
    use crate::payload_search::PayloadSearch;
    use crate::storage::{self, chains::ChainReader, Catalog, SessionFilter};

    fn key() -> FlowKey {
        FlowKey {
            client: Endpoint {
                ip: "10.0.0.2".parse().unwrap(),
                port: 50000,
            },
            server: Endpoint {
                ip: "10.0.0.1".parse().unwrap(),
                port: 8080,
            },
        }
    }

    fn packet(
        direction: Direction,
        sequence: u32,
        bytes: &[u8],
        timestamp: i64,
    ) -> ClassifiedPacket {
        let key = key();
        let (src, dst) = if direction == Direction::C2s {
            (key.client.clone(), key.server.clone())
        } else {
            (key.server.clone(), key.client.clone())
        };
        ClassifiedPacket {
            source_id: 1,
            direction,
            flow_key: key,
            packet: TcpPacket {
                timestamp_micros: timestamp,
                src,
                dst,
                sequence,
                syn: false,
                fin: false,
                rst: false,
                payload: bytes.to_vec(),
            },
        }
    }

    fn flags() -> Regex {
        Regex::new(r"FLAG\{[^}]+\}").unwrap()
    }
    fn request(path: &str) -> Vec<u8> {
        format!("GET {path} HTTP/1.1\r\nHost: test\r\n\r\n").into_bytes()
    }
    fn response(body: &[u8]) -> Vec<u8> {
        let mut bytes =
            format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len()).into_bytes();
        bytes.extend_from_slice(body);
        bytes
    }

    fn handshake(flow: &mut FlowState) {
        for (direction, sequence) in [(Direction::C2s, 1000), (Direction::S2c, 5000)] {
            let mut syn = packet(direction, sequence, b"", 1);
            syn.packet.syn = true;
            assert!(flow.ingest(&syn, &flags()).is_empty());
        }
    }

    fn flow(limit: usize) -> FlowState {
        let mut flow = FlowState::new("local".into(), 1, key(), 1, limit);
        handshake(&mut flow);
        flow
    }

    #[test]
    fn keepalive_emits_atomic_sessions_before_fin_with_individual_times_and_flags() {
        let mut flow = flow(1024);
        let first = request("/one");
        let second = request("/two");
        let reply_one = response(b"one");
        let reply_two = response(b"FLAG{two}");
        assert!(flow
            .ingest(&packet(Direction::C2s, 1001, &first, 100), &flags())
            .is_empty());
        let one = flow.ingest(&packet(Direction::S2c, 5001, &reply_one, 200), &flags());
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].protocol, SessionProtocol::Http);
        assert_eq!(one[0].started_at, 100);
        assert_eq!(one[0].ended_at, 200);
        assert_eq!(one[0].first_payload_c2s_at, Some(100));
        assert_eq!(one[0].first_payload_s2c_at, Some(200));
        assert_eq!(one[0].flag_count, 0);
        assert!(!one[0].incomplete);
        let first_end = 1001 + first.len() as u32;
        let reply_end = 5001 + reply_one.len() as u32;
        flow.ingest(&packet(Direction::C2s, first_end, &second, 300), &flags());
        let two = flow.ingest(
            &packet(Direction::S2c, reply_end, &reply_two, 400),
            &flags(),
        );
        assert_eq!(two.len(), 1);
        assert_eq!(two[0].started_at, 300);
        assert_eq!(two[0].flag_count, 1);
        assert_eq!(two[0].flag_direction, FlagDirection::S2c);
        assert_eq!(two[0].c2s, second);
        assert_eq!(two[0].s2c, reply_two);
        assert!(flow.c2s.bytes.is_empty() && flow.s2c.bytes.is_empty());
        assert!(flow.finish(true, &flags()).is_empty());
    }

    #[test]
    fn out_of_order_headers_do_not_shift_response_association() {
        let mut flow = flow(1024);
        let req = request("/actual");
        assert!(flow
            .ingest(&packet(Direction::C2s, 1011, &req[10..], 110), &flags())
            .is_empty());
        let reply = response(b"FLAG{actual}");
        assert!(flow
            .ingest(&packet(Direction::S2c, 5001, &reply, 200), &flags())
            .is_empty());
        let pairs = flow.ingest(&packet(Direction::C2s, 1001, &req[..10], 100), &flags());
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].c2s, req);
        assert_eq!(pairs[0].s2c, reply);
        assert_eq!(pairs[0].started_at, 100);
        assert!(flow
            .ingest(&packet(Direction::C2s, 1001, &req, 300), &flags())
            .is_empty());
        assert!(flow
            .ingest(&packet(Direction::S2c, 5001, &reply, 400), &flags())
            .is_empty());
    }

    #[test]
    fn a_response_without_initial_tcp_anchors_is_not_guessed() {
        let mut flow = FlowState::new("local".into(), 1, key(), 1, 1024);
        assert!(flow
            .ingest(&packet(Direction::C2s, 1001, &request("/"), 100), &flags())
            .is_empty());
        assert!(flow
            .ingest(
                &packet(Direction::S2c, 5001, &response(b"reply"), 200),
                &flags()
            )
            .is_empty());
        assert!(flow.finish(true, &flags()).is_empty());
    }

    #[test]
    fn server_fin_before_missing_bytes_waits_for_gap_recovery() {
        let mut flow = flow(1024);
        let req = request("/");
        let reply = b"HTTP/1.0 200 OK\r\n\r\nanswer";
        flow.ingest(&packet(Direction::C2s, 1001, &req, 100), &flags());
        let mut fin = packet(Direction::S2c, 5001 + reply.len() as u32, b"", 300);
        fin.packet.fin = true;
        assert!(flow.ingest(&fin, &flags()).is_empty());
        assert!(!flow.response_eof);
        let pairs = flow.ingest(&packet(Direction::S2c, 5001, reply, 200), &flags());
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].s2c, reply);
        assert!(!pairs[0].incomplete);
        assert!(flow.finish(true, &flags()).is_empty());
    }

    #[test]
    fn http_timeout_uses_new_payload_not_acks_retransmissions_or_capture_clock() {
        let mut flow = flow(1024);
        let req = request("/");
        flow.ingest(
            &packet(Direction::C2s, 1001, &req, 9_999_999_999_999_999),
            &flags(),
        );
        flow.last_payload = Instant::now() - Duration::from_secs(16);
        flow.ingest(&packet(Direction::S2c, 5001, b"", 10), &flags());
        flow.ingest(&packet(Direction::C2s, 1001, &req, 10), &flags());
        assert!(flow.expired(Instant::now(), Duration::from_secs(15)));
        let reply = b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n";
        flow.ingest(
            &packet(Direction::S2c, 5001, reply, 9_999_999_999_999_999),
            &flags(),
        );
        assert!(!flow.expired(Instant::now(), Duration::from_secs(15)));
        flow.last_payload = Instant::now() - Duration::from_secs(14);
        flow.ingest(
            &packet(
                Direction::S2c,
                5001 + reply.len() as u32,
                b"x",
                9_999_999_999_999_999,
            ),
            &flags(),
        );
        assert!(!flow.expired(Instant::now(), Duration::from_secs(15)));
    }

    #[test]
    fn unresolved_client_gap_discards_unmatched_sessions_on_timeout() {
        let mut flow = flow(1024);
        let req = request("/missing");
        flow.ingest(&packet(Direction::C2s, 1005, &req[4..], 100), &flags());
        flow.ingest(
            &packet(Direction::S2c, 5001, &response(b"reply"), 200),
            &flags(),
        );
        assert!(flow.finish(true, &flags()).is_empty());
    }

    #[test]
    fn same_endpoints_new_client_syn_is_a_new_tcp_connection() {
        let mut flow = flow(1024);
        let mut same = packet(Direction::C2s, 1000, b"", 100);
        same.packet.syn = true;
        assert!(!flow.is_new_connection(&same));
        same.packet.sequence = 9000;
        assert!(flow.is_new_connection(&same));
        let mut server_syn = packet(Direction::S2c, 5000, b"", 100);
        server_syn.packet.syn = true;
        assert!(!flow.is_new_connection(&server_syn));
        server_syn.packet.sequence = 9000;
        assert!(flow.is_new_connection(&server_syn));
        flow.ingest(
            &packet(Direction::C2s, 1001, &request("/old"), 100),
            &flags(),
        );
        assert!(flow.finish(true, &flags()).is_empty());
    }

    #[test]
    fn tcp_sequence_wrap_and_overlapping_pending_ranges_are_ordered() {
        let mut assembler = TcpAssembler::new(1024);
        assembler.seed(u32::MAX - 2);
        let mut result = Vec::new();
        assembler.push(2, b"fg", 20, |_, bytes, _| result.extend_from_slice(bytes));
        assembler.push(u32::MAX - 2, b"abcde", 10, |_, bytes, _| {
            result.extend_from_slice(bytes)
        });
        assert_eq!(result, b"abcdefg");
        let mut assembler = TcpAssembler::new(1024);
        assembler.seed(100);
        let mut result = Vec::new();
        assembler.push(104, b"efgh", 20, |_, bytes, _| {
            result.extend_from_slice(bytes)
        });
        assembler.push(102, b"cdefghij", 20, |_, bytes, _| {
            result.extend_from_slice(bytes)
        });
        assembler.push(100, b"ab", 10, |_, bytes, _| {
            result.extend_from_slice(bytes)
        });
        assert_eq!(result, b"abcdefghij");
        assert!(!assembler.has_gap());
    }

    #[test]
    fn raw_tcp_and_websocket_still_use_connection_records() {
        for (request, reply, expected) in [
            (
                b"raw request".as_slice(),
                b"raw reply".as_slice(),
                SessionProtocol::RawTcp,
            ),
            (
                b"GET /ws HTTP/1.1\r\nUpgrade: websocket\r\n\r\nclient-frame".as_slice(),
                b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n\r\nserver-frame"
                    .as_slice(),
                SessionProtocol::WebSocket,
            ),
        ] {
            let mut flow = flow(1024);
            assert!(flow
                .ingest(&packet(Direction::C2s, 1001, request, 100), &flags())
                .is_empty());
            assert!(flow
                .ingest(&packet(Direction::S2c, 5001, reply, 200), &flags())
                .is_empty());
            let sessions = flow.finish(true, &flags());
            assert_eq!(sessions.len(), 1);
            assert_eq!(sessions[0].protocol, expected);
            assert_eq!(sessions[0].c2s, request);
            assert_eq!(sessions[0].s2c, reply);
        }
    }

    #[test]
    fn large_query_headers_remain_intact_above_64_kib() {
        let mut flow = flow(1024 * 1024);
        let mut req = b"GET /?q=".to_vec();
        req.extend_from_slice(&vec![b'x'; 300_000]);
        req.extend_from_slice(b" HTTP/1.1\r\nHost: test\r\n\r\n");
        for (index, chunk) in req.chunks(1024).enumerate() {
            assert!(flow
                .ingest(
                    &packet(
                        Direction::C2s,
                        1001 + (index * 1024) as u32,
                        chunk,
                        100 + index as i64
                    ),
                    &flags()
                )
                .is_empty());
        }
        let pairs = flow.ingest(
            &packet(Direction::S2c, 5001, &response(b"ok"), 500),
            &flags(),
        );
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].c2s, req);
    }

    #[test]
    fn actual_16_mib_cap_clips_a_20_mib_response_and_keeps_the_next_session() {
        let limit = 16 * 1024 * 1024;
        let mut flow = flow(limit);
        let reqs = [request("/large"), request("/next")].concat();
        flow.ingest(&packet(Direction::C2s, 1001, &reqs, 100), &flags());
        let header = b"HTTP/1.1 200 OK\r\nContent-Length: 20971520\r\n\r\n";
        let mut sequence = 5001;
        assert!(flow
            .ingest(&packet(Direction::S2c, sequence, header, 200), &flags())
            .is_empty());
        sequence += header.len() as u32;
        let body = vec![b'x'; 64 * 1024];
        let mut completed = Vec::new();
        for _ in 0..320 {
            completed.extend(flow.ingest(&packet(Direction::S2c, sequence, &body, 200), &flags()));
            sequence += body.len() as u32;
        }
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].s2c.len(), limit);
        assert!(completed[0].s2c.capacity() <= limit);
        assert!(completed[0].s2c.starts_with(header));
        assert!(!completed[0].incomplete);
        assert_eq!(completed[0].c2s, request("/large"));
        let next = flow.ingest(
            &packet(Direction::S2c, sequence, &response(b"FLAG{next}"), 300),
            &flags(),
        );
        assert_eq!(next.len(), 1);
        assert_eq!(next[0].c2s, request("/next"));
        assert_eq!(next[0].flag_count, 1);
    }

    #[tokio::test]
    async fn worker_writer_payload_search_and_chains_use_atomic_session_ids() {
        let directory = tempfile::tempdir().unwrap();
        let catalog = Catalog::open(directory.path().join("index.sqlite")).unwrap();
        catalog
            .create_source(&SourceInput {
                name: "test".into(),
                port: 8080,
                enabled: true,
            })
            .unwrap();
        let store = storage::start_writer(
            catalog.clone(),
            directory.path().to_path_buf(),
            Duration::from_secs(60),
            3,
            64,
        )
        .unwrap();
        let runtime = start_workers(
            FlowOptions {
                workers: 1,
                queue_capacity: 64,
                idle_timeout: Duration::from_secs(15),
                max_active_flows_per_worker: 64,
                max_stream_bytes: 16 * 1024 * 1024,
                flag_regex: flags(),
            },
            store.clone(),
        );
        let mut packets = Vec::new();
        for (direction, sequence) in [(Direction::C2s, 1000), (Direction::S2c, 5000)] {
            let mut syn = packet(direction, sequence, b"", 1);
            syn.packet.syn = true;
            packets.push(syn);
        }
        let req_a = request("/login");
        let req_b = request("/flag");
        let reply_a = response(b"token");
        let reply_b = response(b"FLAG{atomic}");
        packets.extend([
            packet(Direction::C2s, 1001, &req_a, 100_000),
            packet(Direction::S2c, 5001, &reply_a, 200_000),
            packet(Direction::C2s, 1001 + req_a.len() as u32, &req_b, 300_000),
            packet(
                Direction::S2c,
                5001 + reply_a.len() as u32,
                &reply_b,
                400_000,
            ),
        ]);
        for packet in packets {
            runtime.senders[0]
                .send(IngressPacket {
                    collector_id: "test-box".into(),
                    packet,
                })
                .await
                .unwrap()
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        let rows = loop {
            let rows = catalog
                .list_sessions(&SessionFilter {
                    limit: 10,
                    ..Default::default()
                })
                .unwrap();
            if rows.len() == 2 {
                break rows;
            }
            assert!(
                Instant::now() < deadline,
                "sessions were not persisted while TCP stayed open"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        assert_ne!(rows[0].id, rows[1].id);
        for row in &rows {
            let stored = catalog.get_session(row.id).unwrap().unwrap();
            let payload = storage::read_payload(directory.path().to_path_buf(), &stored).unwrap();
            if row.http.path.as_deref() == Some("/login") {
                assert_eq!(payload.c2s, req_a);
                assert_eq!(payload.s2c, reply_a);
                assert!(!row.contains_flag);
            } else {
                assert_eq!(payload.c2s, req_b);
                assert_eq!(payload.s2c, reply_b);
                assert!(row.contains_flag);
            }
        }
        let reader = ChainReader::open(&catalog).unwrap();
        let chains = reader
            .list(
                &SessionFilter {
                    limit: 10,
                    contains_flag: Some(true),
                    ..Default::default()
                },
                None,
                &PayloadSearch::new(Vec::new()).unwrap(),
                directory.path(),
            )
            .unwrap();
        assert_eq!(chains.items.len(), 1);
        assert_eq!(chains.items[0].session_count, 2);
        let members = reader
            .member_page(
                chains.items[0].id,
                None,
                Some(chains.items[0].snapshot_id),
                20,
            )
            .unwrap()
            .unwrap();
        assert_eq!(members.items[0].http.path.as_deref(), Some("/login"));
        assert_eq!(members.items[1].http.path.as_deref(), Some("/flag"));
        let impossible = reader
            .list(
                &SessionFilter {
                    limit: 10,
                    ..Default::default()
                },
                None,
                &PayloadSearch::new(vec!["token".into(), "FLAG{atomic}".into()]).unwrap(),
                directory.path(),
            )
            .unwrap();
        assert!(impossible.items.is_empty());
        // A representative larger response must survive the actual worker,
        // segment writer and CRC-checked payload retrieval without clipping.
        drop(reader);
        let large_request = request("/large");
        let mut body = vec![b'x'; 5 * 1024 * 1024];
        body.extend_from_slice(b"FLAG{large}");
        let large_response = response(&body);
        runtime.senders[0]
            .send(IngressPacket {
                collector_id: "test-box".into(),
                packet: packet(
                    Direction::C2s,
                    1001 + (req_a.len() + req_b.len()) as u32,
                    &large_request,
                    2_000_000,
                ),
            })
            .await
            .unwrap();
        let mut sequence = 5001 + (reply_a.len() + reply_b.len()) as u32;
        for (index, chunk) in large_response.chunks(64 * 1024).enumerate() {
            runtime.senders[0]
                .send(IngressPacket {
                    collector_id: "test-box".into(),
                    packet: packet(Direction::S2c, sequence, chunk, 2_100_000 + index as i64),
                })
                .await
                .unwrap();
            sequence += chunk.len() as u32;
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        let large = loop {
            let rows = catalog
                .list_sessions(&SessionFilter {
                    limit: 10,
                    ..Default::default()
                })
                .unwrap();
            if let Some(row) = rows
                .into_iter()
                .find(|row| row.http.path.as_deref() == Some("/large"))
            {
                break row;
            }
            assert!(
                Instant::now() < deadline,
                "larger session was not persisted"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        let stored = catalog.get_session(large.id).unwrap().unwrap();
        let payload = storage::read_payload(directory.path().to_path_buf(), &stored).unwrap();
        assert_eq!(payload.c2s, large_request);
        assert_eq!(payload.s2c, large_response);
        assert_eq!(large.flag_count, 1);
        assert!(!large.incomplete);
        let reader = ChainReader::open(&catalog).unwrap();
        let chains = reader
            .list(
                &SessionFilter {
                    limit: 10,
                    ..Default::default()
                },
                None,
                &PayloadSearch::new(Vec::new()).unwrap(),
                directory.path(),
            )
            .unwrap();
        assert_eq!(
            chains.items.len(),
            2,
            "request capture starts determine chain gaps even inside one TCP connection"
        );
        assert_eq!(chains.items[0].session_count, 1);
        drop(runtime);
        drop(store);
    }
    #[test]
    fn partial_http_headers_also_expire_without_new_payload() {
        let mut flow = flow(1024);
        flow.ingest(
            &packet(Direction::C2s, 1001, b"GET / HTTP/1.1\r\nHost: te", 100),
            &flags(),
        );
        flow.last_payload = Instant::now() - Duration::from_secs(16);
        flow.ingest(&packet(Direction::S2c, 5001, b"", 200), &flags());
        assert!(flow.expired(Instant::now(), Duration::from_secs(15)));
        assert!(flow.finish(true, &flags()).is_empty());
    }

    #[test]
    fn first_large_packet_does_not_lose_oversized_request_framing() {
        let mut flow = flow(128);
        let mut req = b"POST /large HTTP/1.1\r\nContent-Length: 512\r\n\r\n".to_vec();
        req.extend_from_slice(&vec![b'x'; 512]);
        req.extend_from_slice(&request("/next"));
        flow.ingest(&packet(Direction::C2s, 1001, &req, 100), &flags());
        let pairs = flow.ingest(
            &packet(
                Direction::S2c,
                5001,
                &[response(b"discard"), response(b"keep")].concat(),
                200,
            ),
            &flags(),
        );
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].c2s, request("/next"));
        assert_eq!(pairs[0].s2c, response(b"keep"));
    }

    #[test]
    fn conflicting_pending_tcp_overlaps_stop_assembly() {
        let mut assembler = TcpAssembler::new(1024);
        assembler.seed(100);
        let mut result = Vec::new();
        assembler.push(104, b"efgh", 20, |_, bytes, _| {
            result.extend_from_slice(bytes)
        });
        assembler.push(102, b"cdBAD", 20, |_, bytes, _| {
            result.extend_from_slice(bytes)
        });
        assembler.push(100, b"ab", 10, |_, bytes, _| {
            result.extend_from_slice(bytes)
        });
        assert!(assembler.failed);
        assert!(result.is_empty());
    }
    #[test]
    fn idle_completed_keepalive_connection_keeps_anchors_without_payload() {
        let mut flow = flow(1024);
        let first = request("/first");
        let first_response = response(b"first");
        flow.ingest(&packet(Direction::C2s, 1001, &first, 100), &flags());
        assert_eq!(
            flow.ingest(
                &packet(Direction::S2c, 5001, &first_response, 200),
                &flags()
            )
            .len(),
            1
        );
        flow.last_payload = Instant::now() - Duration::from_secs(31);
        assert!(flow.idle_http_context());
        assert!(!flow.expired(Instant::now(), Duration::from_secs(15)));
        flow.ingest(
            &packet(
                Direction::C2s,
                1001 + first.len() as u32,
                &request("/later"),
                32_000_000,
            ),
            &flags(),
        );
        assert!(flow.expired(
            Instant::now() + Duration::from_secs(16),
            Duration::from_secs(15)
        ));
        let next = flow.ingest(
            &packet(
                Direction::S2c,
                5001 + first_response.len() as u32,
                &response(b"later"),
                32_100_000,
            ),
            &flags(),
        );
        assert_eq!(next.len(), 1);
        assert_eq!(next[0].c2s, request("/later"));
        assert_eq!(next[0].started_at, 32_000_000);
    }

    #[test]
    fn tcp_fast_open_request_waits_for_the_server_anchor() {
        for inline_response in [false, true] {
            let mut flow = FlowState::new("local".into(), 1, key(), 1, 1024);
            let req = request("/fast-open");
            let reply = response(b"ok");
            let mut client_syn = packet(Direction::C2s, 1000, &req, 100);
            client_syn.packet.syn = true;
            assert!(flow.ingest(&client_syn, &flags()).is_empty());
            assert!(!flow.suppressed);
            let mut server_syn = packet(
                Direction::S2c,
                5000,
                if inline_response { &reply } else { b"" },
                200,
            );
            server_syn.packet.syn = true;
            let mut pairs = flow.ingest(&server_syn, &flags());
            if !inline_response {
                pairs.extend(flow.ingest(&packet(Direction::S2c, 5001, &reply, 300), &flags()));
            }
            assert_eq!(pairs.len(), 1);
            assert_eq!(pairs[0].c2s, req);
            assert_eq!(pairs[0].s2c, reply);
        }
    }
}
