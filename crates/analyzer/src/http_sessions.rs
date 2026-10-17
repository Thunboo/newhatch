//! Incremental HTTP/1.x framing over ordered TCP bytes. Completed exchanges are
//! ordinary sessions; retained bodies never determine the next message boundary.

use std::collections::VecDeque;

use crate::domain::{Direction, SessionProtocol};

const INLINE_HEADERS: usize = 64;
const MAX_PENDING_REQUESTS: usize = 256;
const MAX_PENDING_CHUNKS: usize = 256;

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct CaptureTimes {
    pub first: Option<i64>,
    pub last: Option<i64>,
}

impl CaptureTimes {
    pub fn observe(&mut self, timestamp: i64) {
        self.first = Some(self.first.map_or(timestamp, |value| value.min(timestamp)));
        self.last = Some(self.last.map_or(timestamp, |value| value.max(timestamp)));
    }
}

pub(crate) struct Exchange {
    pub request_offset: usize,
    pub response_offset: usize,
    pub c2s: Vec<u8>,
    pub s2c: Vec<u8>,
    pub request_times: CaptureTimes,
    pub response_times: CaptureTimes,
    pub incomplete: bool,
    pub protocol: SessionProtocol,
}

#[derive(Clone, Copy)]
enum Role {
    Request,
    Response,
}

#[derive(Clone, Copy, PartialEq)]
enum RequestMethod {
    Head,
    Connect,
    Other,
}

enum Head {
    Request {
        method: RequestMethod,
        websocket: bool,
    },
    Response {
        status: u16,
    },
}

struct Message {
    offset: usize,
    bytes: Vec<u8>,
    times: CaptureTimes,
    head: Option<Head>,
    total: u64,
    dropped: bool,
}

impl Message {
    fn new() -> Self {
        Self {
            offset: 0,
            bytes: Vec::new(),
            times: CaptureTimes::default(),
            head: None,
            total: 0,
            dropped: false,
        }
    }

    fn retain(&mut self, bytes: &[u8], timestamp: i64, limit: usize, role: Role) {
        if bytes.is_empty() {
            return;
        }
        self.times.observe(timestamp);
        self.total = self.total.saturating_add(bytes.len() as u64);
        if self.dropped {
            return;
        }
        if matches!(role, Role::Request) && self.total > limit as u64 {
            self.discard();
        } else {
            let count = bytes.len().min(limit.saturating_sub(self.bytes.len()));
            append_bounded(&mut self.bytes, &bytes[..count], limit);
        }
    }

    fn discard(&mut self) {
        self.bytes = Vec::new();
        self.dropped = true;
    }
}

enum Body {
    Headers,
    Fixed(u64),
    Chunked(Chunk),
    UntilClose,
    Done,
}

enum Chunk {
    Size(Vec<u8>),
    Data(u64),
    DataEnd(u8),
    Trailers { line: Vec<u8>, total: usize },
}

struct Decoder {
    consumed: usize,
    role: Role,
    limit: usize,
    message: Message,
    body: Body,
}

impl Decoder {
    fn new(role: Role, limit: usize) -> Self {
        Self {
            consumed: 0,
            role,
            limit,
            message: Message::new(),
            body: Body::Headers,
        }
    }

    fn take(&mut self) -> Message {
        self.body = Body::Headers;
        let mut message = std::mem::replace(&mut self.message, Message::new());
        message.offset = self.consumed;
        self.consumed = self.consumed.saturating_add(message.total as usize);
        message
    }

    fn feed(
        &mut self,
        bytes: &[u8],
        timestamp: i64,
        request_method: Option<RequestMethod>,
        retain_limit: usize,
    ) -> Result<(usize, Option<Message>), ()> {
        let mut used = 0;
        loop {
            match std::mem::replace(&mut self.body, Body::Headers) {
                Body::Headers => {
                    while used < bytes.len() {
                        // Headers need their own bounded parsing buffer even when
                        // a pipelined request has no remaining retention allowance.
                        if self.message.bytes.len() >= self.limit {
                            return Err(());
                        }
                        append_bounded(&mut self.message.bytes, &bytes[used..used + 1], self.limit);
                        self.message.total += 1;
                        self.message.times.observe(timestamp);
                        used += 1;
                        if self.message.bytes.ends_with(b"\r\n\r\n")
                            || self.message.bytes.ends_with(b"\n\n")
                        {
                            let (head, body) =
                                parse_head(&self.message.bytes, self.role, request_method)?;
                            self.message.head = Some(head);
                            self.body = body;
                            if matches!(self.role, Role::Request)
                                && (self.message.bytes.len() > retain_limit
                                    || matches!(self.body, Body::Fixed(length)
                                        if length > (retain_limit.saturating_sub(self.message.bytes.len())) as u64))
                            {
                                self.message.discard();
                            }
                            break;
                        }
                    }
                    if matches!(self.body, Body::Headers) {
                        return Ok((used, None));
                    }
                }
                Body::Done | Body::Fixed(0) => return Ok((used, Some(self.take()))),
                Body::Fixed(remaining) => {
                    let count = remaining.min((bytes.len() - used) as u64) as usize;
                    self.message.retain(
                        &bytes[used..used + count],
                        timestamp,
                        retain_limit,
                        self.role,
                    );
                    used += count;
                    self.body = Body::Fixed(remaining - count as u64);
                }
                Body::UntilClose => {
                    self.message
                        .retain(&bytes[used..], timestamp, retain_limit, self.role);
                    self.body = Body::UntilClose;
                    return Ok((bytes.len(), None));
                }
                Body::Chunked(chunk) => {
                    self.body = Body::Chunked(chunk);
                    if used < bytes.len() {
                        let (count, next) = consume_chunk(
                            std::mem::replace(&mut self.body, Body::Headers),
                            &bytes[used..],
                            self.limit,
                        )?;
                        self.message.retain(
                            &bytes[used..used + count],
                            timestamp,
                            retain_limit,
                            self.role,
                        );
                        used += count;
                        self.body = next;
                    }
                }
            }
            if used == bytes.len() && !matches!(self.body, Body::Done | Body::Fixed(0)) {
                return Ok((used, None));
            }
        }
    }

    fn finish(&mut self, clean_close: bool) -> Option<(Message, bool)> {
        self.message.head.as_ref()?;
        let incomplete = !(clean_close && matches!(self.body, Body::UntilClose));
        Some((self.take(), incomplete))
    }
}

fn consume_chunk(body: Body, bytes: &[u8], limit: usize) -> Result<(usize, Body), ()> {
    let Body::Chunked(chunk) = body else {
        return Err(());
    };
    match chunk {
        Chunk::Size(mut line) => {
            let count = append_line(&mut line, bytes, limit)?;
            if !line.ends_with(b"\r\n") {
                return Ok((count, Body::Chunked(Chunk::Size(line))));
            }
            let size = line[..line.len() - 2]
                .split(|byte| *byte == b';')
                .next()
                .ok_or(())?;
            let size = std::str::from_utf8(size).map_err(|_| ())?.trim();
            if size.is_empty() || !size.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err(());
            }
            let size = u64::from_str_radix(size, 16).map_err(|_| ())?;
            let next = if size == 0 {
                Chunk::Trailers {
                    line: Vec::new(),
                    total: 0,
                }
            } else {
                Chunk::Data(size)
            };
            Ok((count, Body::Chunked(next)))
        }
        Chunk::Data(remaining) => {
            let count = remaining.min(bytes.len() as u64) as usize;
            let next = if count as u64 == remaining {
                Chunk::DataEnd(0)
            } else {
                Chunk::Data(remaining - count as u64)
            };
            Ok((count, Body::Chunked(next)))
        }
        Chunk::DataEnd(index) => {
            if bytes[0] != b"\r\n"[index as usize] {
                return Err(());
            }
            let next = if index == 0 {
                Chunk::DataEnd(1)
            } else {
                Chunk::Size(Vec::new())
            };
            Ok((1, Body::Chunked(next)))
        }
        Chunk::Trailers { mut line, total } => {
            let count = append_line(&mut line, bytes, limit)?;
            let total = total
                .checked_add(count)
                .filter(|total| *total <= limit)
                .ok_or(())?;
            if line == b"\r\n" {
                return Ok((count, Body::Done));
            }
            if line.ends_with(b"\r\n") {
                let mut headers = [httparse::EMPTY_HEADER; 1];
                let mut complete = line.clone();
                complete.extend_from_slice(b"\r\n");
                if !matches!(
                    httparse::parse_headers(&complete, &mut headers),
                    Ok(httparse::Status::Complete(_))
                ) {
                    return Err(());
                }
                line.clear();
            }
            Ok((count, Body::Chunked(Chunk::Trailers { line, total })))
        }
    }
}

fn append_line(line: &mut Vec<u8>, bytes: &[u8], limit: usize) -> Result<usize, ()> {
    for (index, byte) in bytes.iter().enumerate() {
        if line.len() >= limit {
            return Err(());
        }
        append_bounded(line, &[*byte], limit);
        if *byte == b'\n' {
            if !line.ends_with(b"\r\n") {
                return Err(());
            }
            return Ok(index + 1);
        }
    }
    Ok(bytes.len())
}

fn parse_head(bytes: &[u8], role: Role, method: Option<RequestMethod>) -> Result<(Head, Body), ()> {
    let mut headers = [httparse::EMPTY_HEADER; INLINE_HEADERS];
    match role {
        Role::Request => {
            let mut request = httparse::Request::new(&mut headers);
            let parsed = request.parse(bytes);
            let overflow = matches!(parsed, Err(httparse::Error::TooManyHeaders));
            if !matches!(parsed, Ok(httparse::Status::Complete(_))) && !overflow {
                return Err(());
            }
            let method = match request.method.ok_or(())? {
                "HEAD" => RequestMethod::Head,
                "CONNECT" => RequestMethod::Connect,
                _ => RequestMethod::Other,
            };
            let mut websocket = false;
            let mut fields = Framing::default();
            let mut visit = |header: httparse::Header<'_>| {
                websocket |= header.name.eq_ignore_ascii_case("upgrade")
                    && header.value.eq_ignore_ascii_case(b"websocket");
                fields.push(header)
            };
            if overflow {
                crate::protocol::for_each_header(bytes, visit)?
            } else {
                for header in request.headers {
                    visit(*header)?
                }
            }
            Ok((
                Head::Request { method, websocket },
                fields.body(true, false)?,
            ))
        }
        Role::Response => {
            let mut response = httparse::Response::new(&mut headers);
            let parsed = response.parse(bytes);
            let overflow = matches!(parsed, Err(httparse::Error::TooManyHeaders));
            if !matches!(parsed, Ok(httparse::Status::Complete(_))) && !overflow {
                return Err(());
            }
            let status = response.code.ok_or(())?;
            let no_body = method == Some(RequestMethod::Head)
                || (100..200).contains(&status)
                || status == 204
                || status == 304
                || (method == Some(RequestMethod::Connect) && (200..300).contains(&status));
            let mut fields = Framing::default();
            if overflow {
                crate::protocol::for_each_header(bytes, |header| fields.push(header))?
            } else {
                for header in response.headers {
                    fields.push(*header)?
                }
            }
            Ok((Head::Response { status }, fields.body(false, no_body)?))
        }
    }
}

#[derive(Default)]
struct Framing {
    length: Option<u64>,
    transfer: Option<bool>,
}

impl Framing {
    fn push(&mut self, header: httparse::Header<'_>) -> Result<(), ()> {
        if header.name.eq_ignore_ascii_case("content-length") {
            for value in header.value.split(|byte| *byte == b',') {
                let value = std::str::from_utf8(value).map_err(|_| ())?.trim();
                if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                    return Err(());
                }
                let value = value.parse::<u64>().map_err(|_| ())?;
                if self.length.is_some_and(|old| old != value) {
                    return Err(());
                }
                self.length = Some(value);
            }
        } else if header.name.eq_ignore_ascii_case("transfer-encoding") {
            for value in header.value.split(|byte| *byte == b',') {
                let value = std::str::from_utf8(value).map_err(|_| ())?.trim();
                if value.is_empty() || self.transfer == Some(true) {
                    return Err(());
                }
                self.transfer = Some(value.eq_ignore_ascii_case("chunked"));
            }
        }
        Ok(())
    }

    fn body(self, request: bool, no_body: bool) -> Result<Body, ()> {
        if self.length.is_some() && self.transfer.is_some() {
            return Err(());
        }
        if no_body {
            return Ok(Body::Done);
        }
        match self.transfer {
            Some(true) => return Ok(Body::Chunked(Chunk::Size(Vec::new()))),
            Some(false) => {
                return if request {
                    Err(())
                } else {
                    Ok(Body::UntilClose)
                }
            }
            None => {}
        }
        Ok(match self.length {
            Some(length) => Body::Fixed(length),
            None if request => Body::Done,
            None => Body::UntilClose,
        })
    }
}

struct BufferedChunk {
    bytes: Vec<u8>,
    timestamp: i64,
}

pub(crate) struct HttpSessions {
    limit: usize,
    request: Decoder,
    response: Decoder,
    requests: VecDeque<Message>,
    request_bytes: usize,
    responses: VecDeque<BufferedChunk>,
    response_bytes: usize,
    response_closed: Option<bool>,
    client_upgrade: bool,
    upgrade_client: Vec<u8>,
    upgrade_times: CaptureTimes,
    upgraded: Option<Exchange>,
    pub failed: bool,
    pub discarded_requests: u64,
}

impl HttpSessions {
    pub fn new(limit: usize) -> Self {
        Self {
            limit,
            request: Decoder::new(Role::Request, limit),
            response: Decoder::new(Role::Response, limit),
            requests: VecDeque::new(),
            request_bytes: 0,
            responses: VecDeque::new(),
            response_bytes: 0,
            response_closed: None,
            client_upgrade: false,
            upgrade_client: Vec::new(),
            upgrade_times: CaptureTimes::default(),
            upgraded: None,
            failed: false,
            discarded_requests: 0,
        }
    }

    pub fn feed(&mut self, direction: Direction, bytes: &[u8], timestamp: i64) -> Vec<Exchange> {
        let mut ready = Vec::new();
        if self.failed {
            return ready;
        }
        if let Some(upgrade) = &mut self.upgraded {
            let (target, times) = match direction {
                Direction::C2s => (&mut upgrade.c2s, &mut upgrade.request_times),
                Direction::S2c => (&mut upgrade.s2c, &mut upgrade.response_times),
            };
            upgrade.incomplete |= bytes.len() > self.limit.saturating_sub(target.len());
            append_bounded(target, bytes, self.limit);
            times.observe(timestamp);
            return ready;
        }
        if direction == Direction::C2s {
            let mut rest = bytes;
            while !rest.is_empty() && !self.failed {
                if self.client_upgrade {
                    append_bounded(&mut self.upgrade_client, rest, self.limit);
                    self.upgrade_times.observe(timestamp);
                    break;
                }
                let budget = self.limit.saturating_sub(self.request_bytes);
                match self.request.feed(rest, timestamp, None, budget) {
                    Ok((used, message)) => {
                        rest = &rest[used..];
                        if let Some(message) = message {
                            if message.dropped {
                                self.discarded_requests = self.discarded_requests.saturating_add(1)
                            }
                            if self.requests.len() >= MAX_PENDING_REQUESTS {
                                self.fail();
                                break;
                            }
                            self.client_upgrade = matches!(
                                message.head,
                                Some(Head::Request {
                                    websocket: true,
                                    ..
                                })
                            );
                            self.request_bytes += message.bytes.len();
                            self.requests.push_back(message);
                        }
                    }
                    Err(()) => self.fail(),
                }
            }
        } else if !bytes.is_empty() {
            self.process_response(bytes, timestamp, &mut ready, false);
        }
        self.flush_responses(&mut ready);
        ready
    }

    fn flush_responses(&mut self, ready: &mut Vec<Exchange>) {
        while !self.failed && (!self.requests.is_empty() || self.upgraded.is_some()) {
            let Some(chunk) = self.responses.pop_front() else {
                break;
            };
            self.response_bytes -= chunk.bytes.len();
            self.process_response(&chunk.bytes, chunk.timestamp, ready, true);
        }
        if !self.failed
            && self.response_closed.is_some()
            && self.responses.is_empty()
            && !self.requests.is_empty()
        {
            if let Some((response, incomplete)) =
                self.response.finish(self.response_closed == Some(true))
            {
                self.pair(response, incomplete, ready);
            }
        }
    }

    fn process_response(
        &mut self,
        bytes: &[u8],
        timestamp: i64,
        ready: &mut Vec<Exchange>,
        prepend: bool,
    ) {
        let mut rest = bytes;
        while !rest.is_empty() && !self.failed {
            if let Some(upgrade) = &mut self.upgraded {
                upgrade.incomplete |= rest.len() > self.limit.saturating_sub(upgrade.s2c.len());
                append_bounded(&mut upgrade.s2c, rest, self.limit);
                upgrade.response_times.observe(timestamp);
                break;
            }
            let Some(request) = self.requests.front() else {
                if self.response_bytes.saturating_add(rest.len()) > self.limit
                    || self.responses.len() >= MAX_PENDING_CHUNKS
                {
                    self.fail();
                    break;
                }
                let chunk = BufferedChunk {
                    bytes: rest.to_vec(),
                    timestamp,
                };
                if prepend {
                    self.responses.push_front(chunk)
                } else {
                    self.responses.push_back(chunk)
                }
                self.response_bytes += rest.len();
                break;
            };
            let Some(Head::Request { method, .. }) = &request.head else {
                self.fail();
                break;
            };
            match self
                .response
                .feed(rest, timestamp, Some(*method), self.limit)
            {
                Ok((used, message)) => {
                    rest = &rest[used..];
                    if let Some(response) = message {
                        self.pair(response, false, ready)
                    }
                }
                Err(()) => self.fail(),
            }
        }
    }

    fn pair(&mut self, response: Message, incomplete: bool, ready: &mut Vec<Exchange>) {
        let Some(Head::Response { status }) = response.head else {
            self.fail();
            return;
        };
        if (100..200).contains(&status) && status != 101 {
            return;
        }
        let Some(request) = self.requests.pop_front() else {
            self.fail();
            return;
        };
        self.request_bytes -= request.bytes.len();
        let websocket = status == 101
            || matches!(
                request.head,
                Some(Head::Request {
                    websocket: true,
                    ..
                })
            );
        let tunnel = matches!(
            &request.head,
            Some(Head::Request {
                method: RequestMethod::Connect,
                ..
            })
        ) && (200..300).contains(&status);
        if !request.dropped {
            let mut exchange = Exchange {
                request_offset: request.offset,
                response_offset: response.offset,
                c2s: request.bytes,
                s2c: response.bytes,
                request_times: request.times,
                response_times: response.times,
                incomplete,
                protocol: if websocket {
                    SessionProtocol::WebSocket
                } else {
                    SessionProtocol::Http
                },
            };
            if websocket {
                exchange.incomplete |=
                    self.upgrade_client.len() > self.limit.saturating_sub(exchange.c2s.len());
                append_bounded(&mut exchange.c2s, &self.upgrade_client, self.limit);
                if let Some(time) = self.upgrade_times.last {
                    exchange.request_times.observe(time)
                }
                self.upgrade_client = Vec::new();
                self.upgraded = Some(exchange);
            } else {
                ready.push(exchange);
            }
        }
        if tunnel {
            self.fail()
        }
    }

    pub fn close_response(&mut self, clean_close: bool) -> Vec<Exchange> {
        self.response_closed = Some(clean_close);
        let mut ready = Vec::new();
        self.flush_responses(&mut ready);
        ready
    }

    pub fn finish(
        mut self,
        clean_response_close: bool,
        incomplete_connection: bool,
    ) -> Vec<Exchange> {
        let mut ready = self.close_response(clean_response_close);
        if let Some(mut upgraded) = self.upgraded.take() {
            upgraded.incomplete |= incomplete_connection;
            ready.push(upgraded);
        }
        ready
    }

    pub fn is_websocket(&self) -> bool {
        self.upgraded.is_some()
    }

    pub fn has_pending(&self) -> bool {
        self.failed
            || !self.requests.is_empty()
            || !self.responses.is_empty()
            || self.request.message.times.first.is_some()
            || self.response.message.times.first.is_some()
    }

    pub fn observe_probe_time(&mut self, direction: Direction, timestamp: i64) {
        let decoder = match direction {
            Direction::C2s => &mut self.request,
            Direction::S2c => &mut self.response,
        };
        if decoder.message.times.first.is_some() {
            decoder.message.times.observe(timestamp)
        }
    }

    fn fail(&mut self) {
        if !self.failed {
            tracing::debug!("HTTP association unavailable; suppressing connection remainder");
        }
        self.failed = true;
        self.requests.clear();
        self.responses.clear();
        self.request = Decoder::new(Role::Request, self.limit);
        self.response = Decoder::new(Role::Response, self.limit);
        self.upgrade_client = Vec::new();
        self.request_bytes = 0;
        self.response_bytes = 0;
    }
}

pub(crate) fn append_bounded(target: &mut Vec<u8>, bytes: &[u8], limit: usize) {
    let count = bytes.len().min(limit.saturating_sub(target.len()));
    let needed = target.len() + count;
    if needed > target.capacity() {
        let capacity = needed.max(target.capacity().saturating_mul(2)).min(limit);
        target.reserve_exact(capacity - target.len());
    }
    target.extend_from_slice(&bytes[..count]);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(path: &str) -> Vec<u8> {
        format!("GET {path} HTTP/1.1\r\nHost: test\r\n\r\n").into_bytes()
    }

    fn response(body: &[u8]) -> Vec<u8> {
        let mut bytes =
            format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len()).into_bytes();
        bytes.extend_from_slice(body);
        bytes
    }

    #[test]
    fn pipelining_pairs_each_request_with_its_final_response() {
        let mut http = HttpSessions::new(1024);
        let requests = [request("/one"), request("/two")].concat();
        assert!(http.feed(Direction::C2s, &requests, 10).is_empty());
        let replies = [
            b"HTTP/1.1 100 Continue\r\n\r\n".to_vec(),
            response(b"one"),
            b"HTTP/1.1 103 Early Hints\r\n\r\n".to_vec(),
            response(b"two"),
        ]
        .concat();
        let pairs = http.feed(Direction::S2c, &replies, 20);
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0].c2s, request("/one"));
        assert_eq!(pairs[0].s2c, response(b"one"));
        assert_eq!(pairs[1].c2s, request("/two"));
        assert_eq!(pairs[1].s2c, response(b"two"));
        assert!(http.finish(false, true).is_empty());
    }

    #[test]
    fn delayed_client_capture_preserves_response_backlog_order() {
        let mut http = HttpSessions::new(1024);
        let first = [response(b"one"), response(b"two")].concat();
        assert!(http.feed(Direction::S2c, &first, 20).is_empty());
        assert!(http
            .feed(Direction::S2c, &response(b"three"), 30)
            .is_empty());
        let one = http.feed(Direction::C2s, &request("/one"), 10);
        assert_eq!(one[0].s2c, response(b"one"));
        let two = http.feed(Direction::C2s, &request("/two"), 11);
        assert_eq!(two[0].s2c, response(b"two"));
        let three = http.feed(Direction::C2s, &request("/three"), 12);
        assert_eq!(three[0].s2c, response(b"three"));
    }

    #[test]
    fn message_like_body_bytes_are_not_boundaries() {
        let mut http = HttpSessions::new(4096);
        let body = b"HTTP/1.1 200 OK\r\n\r\nGET /fake HTTP/1.1\r\n\r\n\0\xff";
        let mut post = format!(
            "POST /upload HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
            body.len()
        )
        .into_bytes();
        post.extend_from_slice(body);
        let replies = [response(body), response(b"real")].concat();
        assert!(http
            .feed(
                Direction::C2s,
                &[post.clone(), request("/real")].concat(),
                10
            )
            .is_empty());
        let pairs = http.feed(Direction::S2c, &replies, 20);
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0].c2s, post);
        assert_eq!(pairs[0].s2c, response(body));
        assert_eq!(pairs[1].c2s, request("/real"));
    }

    #[test]
    fn chunked_requests_responses_extensions_and_trailers_split_across_bytes() {
        let mut http = HttpSessions::new(2048);
        let req = b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n4;ext=yes\r\nDATA\r\n0\r\nX-Trailer: value\r\n\r\n";
        let reply = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nYES\r\n0\r\n\r\n";
        for byte in req {
            assert!(http.feed(Direction::C2s, &[*byte], 10).is_empty())
        }
        let mut pairs = Vec::new();
        for byte in reply {
            pairs.extend(http.feed(Direction::S2c, &[*byte], 20))
        }
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].c2s, req);
        assert_eq!(pairs[0].s2c, reply);
        assert!(!pairs[0].incomplete);
        assert!(http.feed(Direction::C2s, &request("/next"), 30).is_empty());
        assert_eq!(http.feed(Direction::S2c, &response(b"next"), 40).len(), 1);
    }

    #[test]
    fn no_body_responses_do_not_consume_the_next_reply() {
        let mut http = HttpSessions::new(2048);
        let reqs = [
            b"HEAD /head HTTP/1.1\r\n\r\n".to_vec(),
            request("/empty"),
            request("/cached"),
            request("/last"),
        ]
        .concat();
        let replies = [
            b"HTTP/1.1 200 OK\r\nContent-Length: 100000\r\n\r\n".to_vec(),
            b"HTTP/1.1 204 No Content\r\n\r\n".to_vec(),
            b"HTTP/1.1 304 Not Modified\r\nContent-Length: 100\r\n\r\n".to_vec(),
            response(b"last"),
        ]
        .concat();
        http.feed(Direction::C2s, &reqs, 10);
        let pairs = http.feed(Direction::S2c, &replies, 20);
        assert_eq!(pairs.len(), 4);
        assert_eq!(pairs[3].s2c, response(b"last"));
    }

    #[test]
    fn connection_delimited_reply_waits_for_server_fin() {
        let mut http = HttpSessions::new(1024);
        http.feed(Direction::C2s, &request("/"), 10);
        let reply = b"HTTP/1.0 200 OK\r\n\r\nbody";
        assert!(http.feed(Direction::S2c, reply, 20).is_empty());
        let pairs = http.close_response(true);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].s2c, reply);
        assert!(!pairs[0].incomplete);
        assert!(http.finish(true, true).is_empty());
    }

    #[test]
    fn incomplete_response_requires_a_complete_request_and_parsed_response_head() {
        let mut http = HttpSessions::new(1024);
        http.feed(Direction::C2s, &request("/"), 10);
        let reply = b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\n\r\nFLAG{partial}";
        http.feed(Direction::S2c, reply, 20);
        let pairs = http.finish(false, true);
        assert_eq!(pairs.len(), 1);
        assert!(pairs[0].incomplete);
        assert_eq!(pairs[0].c2s, request("/"));
        assert_eq!(pairs[0].s2c, reply);
        let mut request_only = HttpSessions::new(1024);
        request_only.feed(Direction::C2s, &request("/"), 10);
        assert!(request_only.finish(false, true).is_empty());
        let mut response_only = HttpSessions::new(1024);
        response_only.feed(Direction::S2c, reply, 20);
        assert!(response_only.finish(false, true).is_empty());
        let mut partial_head = HttpSessions::new(1024);
        partial_head.feed(Direction::C2s, &request("/"), 10);
        partial_head.feed(Direction::S2c, b"HTTP/1.1 200", 20);
        assert!(partial_head.finish(false, true).is_empty());
    }

    #[test]
    fn clipping_a_large_reply_preserves_the_next_pair_and_has_no_incomplete_mark() {
        let mut http = HttpSessions::new(128);
        let reqs = [request("/large"), request("/next")].concat();
        http.feed(Direction::C2s, &reqs, 10);
        let large = response(&vec![b'x'; 512]);
        let replies = [large.clone(), response(b"next")].concat();
        let pairs = http.feed(Direction::S2c, &replies, 20);
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0].c2s, request("/large"));
        assert_eq!(pairs[0].s2c, large[..128]);
        assert!(!pairs[0].incomplete);
        assert_eq!(pairs[1].c2s, request("/next"));
        assert_eq!(pairs[1].s2c, response(b"next"));
    }

    #[test]
    fn clipping_chunked_reply_keeps_counting_chunks_and_trailers() {
        let mut http = HttpSessions::new(128);
        http.feed(
            Direction::C2s,
            &[request("/large"), request("/next")].concat(),
            10,
        );
        let mut large = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n200\r\n".to_vec();
        large.extend_from_slice(&vec![b'x'; 512]);
        large.extend_from_slice(b"\r\n0\r\nTrailer: value\r\n\r\n");
        let pairs = http.feed(
            Direction::S2c,
            &[large.clone(), response(b"next")].concat(),
            20,
        );
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0].s2c, large[..128]);
        assert_eq!(pairs[1].s2c, response(b"next"));
    }

    #[test]
    fn oversized_request_drops_its_response_without_shifting_the_next_pair() {
        let mut http = HttpSessions::new(128);
        let mut post = b"POST /large HTTP/1.1\r\nContent-Length: 512\r\n\r\n".to_vec();
        post.extend_from_slice(&vec![b'x'; 512]);
        post.extend_from_slice(&request("/next"));
        http.feed(Direction::C2s, &post, 10);
        let pairs = http.feed(
            Direction::S2c,
            &[response(b"discard"), response(b"keep")].concat(),
            20,
        );
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].c2s, request("/next"));
        assert_eq!(pairs[0].s2c, response(b"keep"));
    }

    #[test]
    fn conflicting_lengths_or_framing_suppress_the_remainder() {
        for bad in [
            b"POST / HTTP/1.1\r\nContent-Length: 1\r\nContent-Length: 2\r\n\r\nx".as_slice(),
            b"POST / HTTP/1.1\r\nContent-Length: 1\r\nTransfer-Encoding: chunked\r\n\r\nx",
            b"POST / HTTP/1.1\r\nTransfer-Encoding: gzip\r\n\r\nx",
        ] {
            let mut http = HttpSessions::new(1024);
            http.feed(Direction::C2s, &request("/good"), 10);
            assert_eq!(http.feed(Direction::S2c, &response(b"good"), 20).len(), 1);
            http.feed(Direction::C2s, bad, 30);
            assert!(http.failed);
            assert!(http.feed(Direction::C2s, &request("/later"), 40).is_empty());
            assert!(http
                .feed(Direction::S2c, &response(b"wrong"), 50)
                .is_empty());
            assert!(http.finish(false, true).is_empty());
        }
    }

    #[test]
    fn recovered_request_can_have_later_capture_time_than_its_response() {
        let mut http = HttpSessions::new(1024);
        http.feed(Direction::S2c, &response(b"reply"), 10);
        let pairs = http.feed(Direction::C2s, &request("/recovered"), 20);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].c2s, request("/recovered"));
        assert_eq!(pairs[0].s2c, response(b"reply"));
        assert!(!http.failed);
    }

    #[test]
    fn many_keepalive_exchanges_release_completed_payloads() {
        let mut http = HttpSessions::new(128);
        for number in 0..1000 {
            let start = number * 100;
            http.feed(Direction::C2s, &request("/"), start);
            let pairs = http.feed(Direction::S2c, &response(b"result"), start + 50);
            assert_eq!(pairs.len(), 1);
            assert_eq!(http.request_bytes, 0);
            assert_eq!(http.response_bytes, 0);
            assert!(http.request.message.bytes.is_empty());
            assert!(http.response.message.bytes.is_empty());
        }
        assert!(!http.failed);
    }

    #[test]
    fn pipeline_metadata_is_bounded_even_for_tiny_requests() {
        let mut http = HttpSessions::new(1024 * 1024);
        for index in 0..=MAX_PENDING_REQUESTS {
            http.feed(Direction::C2s, &request("/"), index as i64);
        }
        assert!(http.failed);
        assert!(http.requests.is_empty());
        assert_eq!(http.request_bytes, 0);
    }

    #[test]
    fn websocket_upgrade_keeps_the_handshake_and_frames_in_one_connection_record() {
        let mut http = HttpSessions::new(1024);
        let req = b"GET /ws HTTP/1.1\r\nUpgrade: websocket\r\n\r\n";
        http.feed(Direction::C2s, req, 10);
        http.feed(Direction::C2s, b"client-frame", 15);
        let reply = b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n\r\nserver-frame";
        assert!(http.feed(Direction::S2c, reply, 20).is_empty());
        assert!(http.is_websocket());
        let sessions = http.finish(true, false);
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].protocol, SessionProtocol::WebSocket);
        assert_eq!(sessions[0].c2s, [req.as_slice(), b"client-frame"].concat());
        assert_eq!(sessions[0].s2c, reply);
    }
    #[test]
    fn header_counts_above_the_inline_buffer_keep_framing_and_metadata() {
        let mut http = HttpSessions::new(4096);
        let mut req = b"POST /many HTTP/1.1\r\n".to_vec();
        let mut reply = b"HTTP/1.1 200 OK\r\n".to_vec();
        for index in 0..80 {
            req.extend_from_slice(format!("X-{index}: value\r\n").as_bytes());
            reply.extend_from_slice(format!("X-{index}: value\r\n").as_bytes());
        }
        req.extend_from_slice(b"Host: test\r\nContent-Length: 4\r\n\r\nDATA");
        reply.extend_from_slice(b"Content-Type: text/plain\r\nContent-Length: 2\r\n\r\nOK");
        http.feed(
            Direction::C2s,
            &[req.clone(), request("/next")].concat(),
            10,
        );
        let pairs = http.feed(
            Direction::S2c,
            &[reply.clone(), response(b"next")].concat(),
            20,
        );
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0].c2s, req);
        assert_eq!(pairs[0].s2c, reply);
        assert_eq!(pairs[1].s2c, response(b"next"));
        let (protocol, meta) = crate::protocol::classify(&pairs[0].c2s, &pairs[0].s2c);
        assert_eq!(protocol, SessionProtocol::Http);
        assert_eq!(meta.host.as_deref(), Some("test"));
        assert_eq!(meta.method.as_deref(), Some("POST"));
        assert_eq!(meta.path.as_deref(), Some("/many"));
        assert_eq!(meta.content_type.as_deref(), Some("text/plain"));
    }
}
