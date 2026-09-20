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

- User configuration favors plaintext `USERNAME`/`PASSWORD`; analyzer hashes the password at startup.
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

## Retention Choice

- Retain a configured count of rotated payload segments.
- Rotation/expiration happens on completed-session append, not from a periodic cleanup clock.
- Do not add automatic SQLite `VACUUM` or unsafe in-place segment compaction.
