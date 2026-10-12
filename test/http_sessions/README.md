# Atomic HTTP Session Checks

The behavior contract is [Atomic HTTP Sessions](../../docs/http-sessions.md).
Checks use generated traffic and temporary SQLite/segment files. They do not
modify the local `.env` or existing stored traffic.

## Live keep-alive fixture

[Server](keepalive_server.py) and [client](keepalive_client.py) use only Python's
standard library (Python 3.7+). Copy the server to the capture host and the client
to another machine, or run them from an existing repository checkout. Start the
server in a terminal on the capture host:

```bash
python3 test/http_sessions/keepalive_server.py
```

Create/enable a Source for TCP port **18081** on the collector observing this
host. Wait for the capture filter to become active before running the client.
From another machine, replace `SERVER_IP` with that host's address:

```bash
python3 test/http_sessions/keepalive_client.py SERVER_IP
```

The client sends five sequential numbered GET/POST requests, reads and validates
every response, checks the original socket and a server-assigned connection ID,
then keeps TCP open for 20 seconds. All five printed `connection=` values must
match. In newhatch, expect five distinct session IDs, one request and its matching
`reply-N` response per session, and complete POST bodies. They must be visible
while the client is holding the connection open. Response 3 includes
`FLAG{KEEPALIVE_TEST_3}`; if FLAG_REGEX matches this format, only that pair has a
flag. Chain grouping still depends on the existing request-start adjacency rule.

To check continuation after an idle pause above the pending timeout:

```bash
python3 test/http_sessions/keepalive_client.py SERVER_IP --pause-after 2
```

This pauses for 20 seconds after pair 2, then verifies the remaining pairs on the
same TCP connection. They form a later chain because of the gap between requests.
`--count 10` sends more pairs; `--hold 0` closes immediately after validation.
The server port can be changed with `--port`; use the same port in the client and
Source. Stop the server with Ctrl-C and disable/remove the temporary Source.

For physical-interface capture, use a client on another machine: a request to
`127.0.0.1` or the server's own address may travel over loopback instead. These
fixtures do not start or configure newhatch, containers, or firewall rules.
The HTTP/1.1 server uses accurate Content-Length framing for persistence, as
described in the [Python documentation](https://docs.python.org/3/library/http.server.html#http.server.BaseHTTPRequestHandler.protocol_version).

## Native checks

From the repository root with Rust 1.85 or newer:

```bash
cargo test -p newhatch-analyzer --lib http_sessions::
cargo test -p newhatch-analyzer --lib flow::
cargo test --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

`http_sessions.rs` covers pipelining, informational responses, fixed-length and
chunked request/response bodies, chunk extensions/trailers and bytewise splits,
HEAD/204/304, close-delimited bodies, delayed client capture, one-sided/incomplete
messages, invalid or conflicting framing, bounded pipeline metadata, response
clipping, oversized-request disposal and continuation with the next valid pair.
Requests and responses with more than 64 headers retain their complete payloads
and metadata, including framing fields after the inline parsing buffer.

`flow.rs` covers SYN anchors, endpoint reuse, sequence wraparound, overlapping
pending ranges, out-of-order headers, retransmissions, FIN arriving before missing
data, per-session capture timestamps/flags, Fast Open and monotonic inactivity
expiry. Empty keep-alive contexts remain usable after a pause above 15 seconds.
It verifies unchanged raw TCP/WebSocket connection records and long query headers.

The actual 16 MiB limit is exercised with a generated 20 MiB response. Worker and
writer integration uses real temporary SQLite/segment storage to verify distinct
session IDs before TCP closes, byte-exact CRC-checked retrieval, flag-bearing chain
context, per-session AND matching, a fully retained 5 MiB response and chain gaps
based on request capture starts inside the same TCP connection.

## Frontend and related regression checks

```bash
npm --prefix frontend run lint
npm --prefix frontend run build
```

The existing session and chain components render the stored directional payloads;
their contracts and browser checks are in
[session-chain verification](../session_chains/README.md).
Full Rust regression includes storage, authentication/API and collector transport
tests. Some collector tests need temporary loopback listeners.

## Limits and target-host verification

Native fixtures validate reconstruction/preprocessing and storage, not Linux
AF_PACKET capture, kernel buffers, NIC/offload behavior or performance under the
competition's real packet rate. Local and split capture need target-host checks
using [collector verification](../collector_split/README.md).

On the target, generate several HTTP/1.x requests over one keep-alive connection.
Verify one request/response per session/window, records becoming visible before
connection closure, chain grouping and flags. Include ordinary responses and a
5–10 MB response; an optional response above 16 MiB should be clipped without a
dedicated frontend notice, and the following request must still be paired correctly.
Capture should begin before the TCP connection so both SYN anchors are observed.

Agents must not invoke Docker, Docker Compose or OrbStack. Container-dependent
checks require the user to execute commands from the related runbooks.
