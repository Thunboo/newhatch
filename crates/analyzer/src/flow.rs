use std::{
    collections::{BTreeMap, HashMap},
    net::IpAddr,
    time::Duration,
};

use regex::bytes::Regex;
use tokio::sync::mpsc;

use crate::{
    domain::{CompletedSession, Direction, FlagDirection, FlowKey},
    packet::ClassifiedPacket,
    protocol,
    storage::StorageHandle,
};

pub struct FlowRuntime {
    pub senders: Vec<mpsc::Sender<ClassifiedPacket>>,
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
            idle_timeout_micros: options.idle_timeout.as_micros().min(i64::MAX as u128) as i64,
            max_active_flows: options.max_active_flows_per_worker,
            max_stream_bytes: options.max_stream_bytes,
            dropped_flows: 0,
            dropped_sessions: 0,
        };
        tokio::spawn(worker.run());
        senders.push(sender);
    }
    FlowRuntime { senders }
}

pub fn worker_index(key: &FlowKey, workers: usize) -> usize {
    let mut hash = 0xcbf29ce484222325u64;
    mix_endpoint(&mut hash, key.client.ip, key.client.port);
    mix_endpoint(&mut hash, key.server.ip, key.server.port);
    (hash as usize) % workers
}

fn mix_endpoint(hash: &mut u64, ip: IpAddr, port: u16) {
    match ip {
        IpAddr::V4(value) => mix_bytes(hash, &value.octets()),
        IpAddr::V6(value) => mix_bytes(hash, &value.octets()),
    }
    mix_bytes(hash, &port.to_be_bytes());
}

fn mix_bytes(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(0x100000001b3);
    }
}

struct FlowWorker {
    worker_id: usize,
    flows: HashMap<FlowKey, FlowState>,
    receiver: mpsc::Receiver<ClassifiedPacket>,
    storage: StorageHandle,
    flag_regex: Regex,
    idle_timeout_micros: i64,
    max_active_flows: usize,
    max_stream_bytes: usize,
    dropped_flows: u64,
    dropped_sessions: u64,
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
            self.store(flow.finish(true, &self.flag_regex));
        }
    }

    fn handle_packet(&mut self, classified: ClassifiedPacket) {
        let key = classified.flow_key.clone();
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

        let should_finish = {
            let flow = self.flows.entry(key.clone()).or_insert_with(|| {
                FlowState::new(
                    classified.source_id,
                    key.clone(),
                    classified.packet.timestamp_micros,
                    self.max_stream_bytes,
                )
            });
            flow.ingest(&classified, &self.flag_regex);
            classified.packet.rst || (flow.c2s_closed && flow.s2c_closed)
        };
        if should_finish {
            if let Some(flow) = self.flows.remove(&key) {
                self.store(flow.finish(false, &self.flag_regex));
            }
        }
    }

    fn expire_idle(&mut self) {
        let now = unix_micros();
        let expired = self
            .flows
            .iter()
            .filter_map(|(key, flow)| {
                (now.saturating_sub(flow.ended_at) >= self.idle_timeout_micros)
                    .then_some(key.clone())
            })
            .collect::<Vec<_>>();
        for key in expired {
            if let Some(flow) = self.flows.remove(&key) {
                self.store(flow.finish(true, &self.flag_regex));
            }
        }
    }

    fn store(&mut self, session: CompletedSession) {
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

struct FlowState {
    source_id: i64,
    key: FlowKey,
    started_at: i64,
    ended_at: i64,
    c2s: StreamAssembler,
    s2c: StreamAssembler,
    c2s_closed: bool,
    s2c_closed: bool,
    c2s_flag_count: usize,
    s2c_flag_count: usize,
    c2s_scanned_to: usize,
    s2c_scanned_to: usize,
}

impl FlowState {
    fn new(source_id: i64, key: FlowKey, timestamp: i64, max_stream_bytes: usize) -> Self {
        Self {
            source_id,
            key,
            started_at: timestamp,
            ended_at: timestamp,
            c2s: StreamAssembler::new(max_stream_bytes),
            s2c: StreamAssembler::new(max_stream_bytes),
            c2s_closed: false,
            s2c_closed: false,
            c2s_flag_count: 0,
            s2c_flag_count: 0,
            c2s_scanned_to: 0,
            s2c_scanned_to: 0,
        }
    }

    fn ingest(&mut self, classified: &ClassifiedPacket, flag_regex: &Regex) {
        self.started_at = self.started_at.min(classified.packet.timestamp_micros);
        self.ended_at = self.ended_at.max(classified.packet.timestamp_micros);
        let sequence = classified
            .packet
            .sequence
            .wrapping_add(u32::from(classified.packet.syn));
        match classified.direction {
            Direction::C2s => {
                let changed = self.c2s.push(sequence, &classified.packet.payload);
                if changed {
                    self.c2s_flag_count = self.c2s_flag_count.saturating_add(scan_new_matches(
                        flag_regex,
                        self.c2s.bytes(),
                        &mut self.c2s_scanned_to,
                    ));
                }
                self.c2s_closed |= classified.packet.fin || classified.packet.rst;
            }
            Direction::S2c => {
                let changed = self.s2c.push(sequence, &classified.packet.payload);
                if changed {
                    self.s2c_flag_count = self.s2c_flag_count.saturating_add(scan_new_matches(
                        flag_regex,
                        self.s2c.bytes(),
                        &mut self.s2c_scanned_to,
                    ));
                }
                self.s2c_closed |= classified.packet.fin || classified.packet.rst;
            }
        }
    }

    fn finish(self, timed_out: bool, flag_regex: &Regex) -> CompletedSession {
        let FlowState {
            source_id,
            key,
            started_at,
            ended_at,
            c2s,
            s2c,
            c2s_closed,
            s2c_closed,
            c2s_flag_count,
            s2c_flag_count,
            c2s_scanned_to: _,
            s2c_scanned_to: _,
        } = self;
        let c2s_truncated = c2s.truncated;
        let s2c_truncated = s2c.truncated;
        let c2s = c2s.into_bytes();
        let s2c = s2c.into_bytes();
        let exact_c2s_flags = flag_regex.find_iter(&c2s).count();
        let exact_s2c_flags = flag_regex.find_iter(&s2c).count();
        if c2s_flag_count != exact_c2s_flags || s2c_flag_count != exact_s2c_flags {
            tracing::debug!(
                provisional_c2s = c2s_flag_count,
                exact_c2s = exact_c2s_flags,
                provisional_s2c = s2c_flag_count,
                exact_s2c = exact_s2c_flags,
                "final flag scan corrected streaming counts"
            );
        }
        let (protocol, http) = protocol::classify(&c2s, &s2c);
        CompletedSession {
            source_id,
            started_at,
            ended_at,
            client_ip: key.client.ip,
            client_port: key.client.port,
            server_ip: key.server.ip,
            server_port: key.server.port,
            protocol,
            c2s,
            s2c,
            flag_direction: FlagDirection::from_counts(exact_c2s_flags, exact_s2c_flags),
            flag_count: exact_c2s_flags.saturating_add(exact_s2c_flags),
            incomplete: timed_out || c2s_truncated || s2c_truncated || !(c2s_closed && s2c_closed),
            http,
        }
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
        self.bytes.extend_from_slice(&payload[..accepted]);
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

fn unix_micros() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_micros().min(i64::MAX as u128) as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use regex::bytes::Regex;

    use super::{scan_new_matches, StreamAssembler};

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
