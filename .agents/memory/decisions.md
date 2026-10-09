# Decision Summary

Canonical details live in `docs/agent-decisions.md`. This file is the compact working set.

## Fixed Architecture

- Normal operation is live capture; offline PCAP ingestion is outside MVP.
- Filter monitored TCP ports in kernel BPF before userspace.
- Persist reconstructed sessions, never packet rows or permanent raw PCAP.
- Scan flags after TCP reassembly.
- SQLite is metadata/index storage only.
- Store C2S/S2C payloads in versioned append-only segment files.
- Keep bounded queues and observable drops; never use unbounded buffering.
- Keep Suricata parallel and optional, outside the Rust hot path.
- MVP protocols are `raw_tcp`, HTTP/1.x and WebSocket upgrade classification.

## Implemented Runtime Choices

- Capture uses cooked `AF_PACKET/SOCK_DGRAM` and L3 parsing for Ethernet, loopback, Docker bridge and WireGuard/TUN consistency.
- Flow ownership is worker-local and deterministic.
- Split deployment boundary is after `ClassifiedPacket`.
- `crates/collector` owns capture, minimal classification and bounded forwarding.
- `crates/analyzer` owns reassembly, detection, persistence, API and UI state.
- `crates/protocol` owns shared versioned protobuf transport types.
- Runtime mode is `ANALYZER=local|remote`; do not reintroduce `NODE` or `PACKET_INGRESS_MODE`.
- Split endpoints support FQDN. Collector resolves again during reconnect; analyzer allowlist FQDNs resolve at startup.
- Empty `ALLOWED_COLLECTORS` intentionally accepts any collector peer until PSK hardening lands.

## Authentication Choices

- User configuration uses project-prefixed plaintext `NEWHATCH_USERNAME`/`NEWHATCH_PASSWORD`; analyzer hashes the password at startup.
- Use bounded ephemeral server-side sessions and absolute expiration.
- nginx enforces real client CIDRs; analyzer trusts only local proxy transport.
- Frontend must not mount analyzer data; nginx denies dotfile/database/segment artifact paths before the SPA fallback.
- Do not trust forwarded headers, use permissive CORS, store browser tokens, or expose analyzer port 3000.

## Agent Runtime Safety

- Agents must not invoke Docker/Docker Compose, contact the Docker daemon, or control OrbStack because doing so can break the user's VPN tunnel. Provide exact Docker commands for the user to run instead.

## UI Choices

- The user-facing name is `Нюхач`; the logo is a bundled frontend asset.
- Session pagination is cursor-based and merged by unique ID.
- Five-second polling runs only at the live edge and must not collapse loaded history.
- History loads automatically near the bottom; normal `Load older` UI is removed.
- Detail-panel interaction must not move background traffic unless the pointer is over the exposed session list.
- Desktop navigation is fixed in the viewport.
- Collector status is shown globally only in remote analyzer mode.
- Issue #12 exports are frontend-only. Generate cURL/Python requests from the visible text body, including Unicode and Format JSON; raw bytes validate framing but are not the replay representation. Use the bundled cURL/Python logos with text fallbacks left of C2S ordinary copy. See `docs/mvp.md`.

## Session Chains (Issue #13)

- Implemented after authorization on 2026-10-08. Durable specification/API limits: `docs/session-chains.md`; UX: `docs/mvp.md`.
- One step is a whole TCP session with its existing C2S/S2C pair, including multiple HTTP exchanges inside one connection.
- Key: `collector_id + client_ip + source_id`; continue while adjacent session start gaps are <= 1 second.
- Analyzer persists collector identity and first-payload capture times for both directions; do not add collector aggregation or packet/fragment chronology.
- Grouping is an off-by-default display option computed on demand. Keep pairs together in session-start order, with C2S before S2C and per-window cURL/Python exports only on C2S.
- The user authorized a centered toolbar Python export of all snapshot members and every supported HTTP request within C2S. Frontend-only byte-framed request extraction leaves session persistence/browsing intact. Use sequential requests.Session replay with initial first-request cookies and response cookie updates; omit later captured Cookie headers. Cancel on close and never silently skip failures or copy a partial script. Dynamic CSRF/IDs stay manual.
- The user requested conditional cookie scaffolding: emit seed_cookies/imports only when an initial cookie name appears later for the same host; otherwise keep the first Cookie header directly. Do not require a helper for Session's automatic response-cookie handling.
- The chain card scrolls between pairs; every expanded payload window keeps its current viewport-dependent size and scrolls internally. Payload length and chain-member count must not resize windows. Neither scroll level may move the background list.
- The user approved a transparent overlay strip (`right: 0px; width: 26%; min-width: 100px`) flush with the right edge of each chain window for outer scrolling. Raise copy actions above it with z-index; retain independent payload scrolling over uncovered text and window dimensions. Use native scroll routing rather than manually translating wheel deltas.
- Show the small localized "Scroll sessions here" hint below Text/Hex only in chain detail; ordinary session detail has no hint.
- Form membership before content filters; one member must match all filters and the card retains other context. Snapshot cursors exclude late insertions until refresh, which replaces stale split rows. Legacy NULL collectors stay singletons; retention is unchanged.
- Chain SQL uses at most two concurrent readers with progress interruption (2 seconds/50 million steps); pages cap at 200, member default 20, payload scan max 2,000 candidates with continuation within a chain. Do not move grouping into packet workers.

## Python Replay Export (Issue #20)

- Approved on 2026-10-09 and implemented on 2026-10-10. Canonical contract: `docs/python-replay.md`.
- Individual and chain scripts accept one optional positional IP/FQDN for every connection host. Preserve captured ports; no argument keeps each member's own captured destination.
- As clarified on 2026-10-10, preserve captured Host without argv; an override replaces its value with the new host and captured server port. Do not synthesize missing Host or destination-related explicit headers; normal HTTP-client transport behavior remains enabled.
- Keep `response` and print every actual replay body with `print(response.text, flush=True)`, including HTTP error bodies. Preserve sequential Session replay, response-updated cookies, conditional helper generation, timeouts, disabled redirects, cancellation and complete-export errors.
- Use effective Host for cookie scope or the effective connection host without Host. An override unifies cookie hosts even when captured Host values or ports differ. If reuse depends on argv, guard cookie setup at runtime; omit helpers when reuse cannot occur.

## Retention Choice

- Retain a configured count of rotated payload segments.
- Rotation/expiration happens on completed-session append, not from a periodic cleanup clock.
- Do not add automatic SQLite `VACUUM` or unsafe in-place segment compaction.
