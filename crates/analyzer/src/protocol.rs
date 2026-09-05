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
    if request.parse(bytes).is_err() || request.method.is_none() {
        return RequestInfo::default();
    }

    RequestInfo {
        parsed: true,
        websocket_upgrade: has_websocket_upgrade(request.headers),
        method: request.method.map(str::to_owned),
        host: header_value(request.headers, "host"),
        path: request.path.map(str::to_owned),
        content_type: header_value(request.headers, "content-type"),
    }
}

fn parse_response(bytes: &[u8]) -> ResponseInfo {
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut response = httparse::Response::new(&mut headers);
    if response.parse(bytes).is_err() || response.code.is_none() {
        return ResponseInfo::default();
    }

    ResponseInfo {
        parsed: true,
        websocket_upgrade: has_websocket_upgrade(response.headers),
        status: response.code,
        content_type: header_value(response.headers, "content-type"),
    }
}

fn has_websocket_upgrade(headers: &[httparse::Header<'_>]) -> bool {
    headers.iter().any(|header| {
        header.name.eq_ignore_ascii_case("upgrade")
            && String::from_utf8_lossy(header.value).eq_ignore_ascii_case("websocket")
    })
}

fn header_value(headers: &[httparse::Header<'_>], name: &str) -> Option<String> {
    headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case(name))
        .map(|header| String::from_utf8_lossy(header.value).trim().to_owned())
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
