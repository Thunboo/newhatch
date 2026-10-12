use crate::domain::{HttpMetadata, SessionProtocol};

pub fn classify(c2s: &[u8], s2c: &[u8]) -> (SessionProtocol, HttpMetadata) {
    let request = parse_request(c2s);
    let response = parse_response(s2c);
    let websocket =
        request.websocket_upgrade || response.websocket_upgrade || response.status == Some(101);

    let protocol = if websocket {
        SessionProtocol::WebSocket
    } else if request.parsed || response.parsed {
        SessionProtocol::Http
    } else {
        SessionProtocol::RawTcp
    };

    (
        protocol,
        HttpMetadata {
            method: request.method,
            host: request.host,
            path: request.path,
            status: response.status,
            content_type: response.content_type.or(request.content_type),
        },
    )
}

#[derive(Default)]
struct RequestInfo {
    parsed: bool,
    websocket_upgrade: bool,
    method: Option<String>,
    host: Option<String>,
    path: Option<String>,
    content_type: Option<String>,
}

#[derive(Default)]
struct ResponseInfo {
    parsed: bool,
    websocket_upgrade: bool,
    status: Option<u16>,
    content_type: Option<String>,
}

fn parse_request(bytes: &[u8]) -> RequestInfo {
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut request = httparse::Request::new(&mut headers);
    let parsed = request.parse(bytes);
    let overflow = matches!(parsed, Err(httparse::Error::TooManyHeaders));
    if (!matches!(parsed, Ok(httparse::Status::Complete(_))) && !overflow)
        || request.method.is_none()
        || request.version.is_none()
    {
        return RequestInfo::default();
    }
    let mut info = RequestInfo {
        parsed: true,
        method: request.method.map(str::to_owned),
        path: request.path.map(str::to_owned),
        ..RequestInfo::default()
    };
    let mut visit = |header: httparse::Header<'_>| {
        if header.name.eq_ignore_ascii_case("host") && info.host.is_none() {
            info.host = Some(String::from_utf8_lossy(header.value).into_owned());
        }
        if header.name.eq_ignore_ascii_case("content-type") && info.content_type.is_none() {
            info.content_type = Some(String::from_utf8_lossy(header.value).into_owned());
        }
        info.websocket_upgrade |= header.name.eq_ignore_ascii_case("upgrade")
            && header.value.eq_ignore_ascii_case(b"websocket");
        Ok(())
    };
    if overflow {
        if for_each_header(bytes, visit).is_err() {
            return RequestInfo::default();
        }
    } else {
        for header in request.headers {
            let _ = visit(*header);
        }
    }
    info
}

fn parse_response(bytes: &[u8]) -> ResponseInfo {
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut response = httparse::Response::new(&mut headers);
    let parsed = response.parse(bytes);
    let overflow = matches!(parsed, Err(httparse::Error::TooManyHeaders));
    if (!matches!(parsed, Ok(httparse::Status::Complete(_))) && !overflow)
        || response.code.is_none()
        || response.version.is_none()
    {
        return ResponseInfo::default();
    }
    let mut info = ResponseInfo {
        parsed: true,
        status: response.code,
        ..ResponseInfo::default()
    };
    let mut visit = |header: httparse::Header<'_>| {
        if header.name.eq_ignore_ascii_case("content-type") && info.content_type.is_none() {
            info.content_type = Some(String::from_utf8_lossy(header.value).into_owned());
        }
        info.websocket_upgrade |= header.name.eq_ignore_ascii_case("upgrade")
            && header.value.eq_ignore_ascii_case(b"websocket");
        Ok(())
    };
    if overflow {
        if for_each_header(bytes, visit).is_err() {
            return ResponseInfo::default();
        }
    } else {
        for header in response.headers {
            let _ = visit(*header);
        }
    }
    info
}

// Overflow headers are validated one line at a time, so the common 64-field
// stack buffer is a fast path rather than a reason to drop a valid exchange.
// Scratch memory is proportional to the largest line, not the header count.
pub(crate) fn for_each_header(
    bytes: &[u8],
    mut visit: impl FnMut(httparse::Header<'_>) -> Result<(), ()>,
) -> Result<(), ()> {
    let mut cursor = bytes.iter().position(|byte| *byte == b'\n').ok_or(())? + 1;
    let mut scratch = Vec::new();
    while cursor < bytes.len() {
        let end = cursor
            + bytes[cursor..]
                .iter()
                .position(|byte| *byte == b'\n')
                .ok_or(())?
            + 1;
        let line = &bytes[cursor..end];
        if line == b"\r\n" || line == b"\n" {
            return Ok(());
        }
        if scratch.capacity() < line.len() + 2 {
            scratch = Vec::with_capacity(line.len() + 2)
        }
        scratch.clear();
        scratch.extend_from_slice(line);
        scratch.extend_from_slice(b"\r\n");
        let mut headers = [httparse::EMPTY_HEADER; 1];
        match httparse::parse_headers(&scratch, &mut headers) {
            Ok(httparse::Status::Complete((_, parsed))) if parsed.len() == 1 => visit(parsed[0])?,
            _ => return Err(()),
        }
        cursor = end;
    }
    Err(())
}

#[cfg(test)]
mod tests {
    use crate::domain::SessionProtocol;

    use super::classify;

    #[test]
    fn extracts_http_metadata() {
        let c2s = b"GET /objects/42 HTTP/1.1\r\nHost: service\r\n\r\n";
        let s2c = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{}";
        let (protocol, metadata) = classify(c2s, s2c);
        assert_eq!(protocol, SessionProtocol::Http);
        assert_eq!(metadata.method.as_deref(), Some("GET"));
        assert_eq!(metadata.path.as_deref(), Some("/objects/42"));
        assert_eq!(metadata.host.as_deref(), Some("service"));
        assert_eq!(metadata.status, Some(200));
    }

    #[test]
    fn recognizes_websocket_upgrade() {
        let request = b"GET /ws HTTP/1.1\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n";
        let response = b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n\r\n";
        assert_eq!(classify(request, response).0, SessionProtocol::WebSocket);
    }
}
