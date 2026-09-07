# Decisions

A compact log of decisions and implementation choices for future agent sessions.

## Canonical Source

The fixed project decisions are maintained in `docs/agent-decisions.md`.
This file is a compact working summary for agents.

## Fixed Decisions

### Live Capture Only for MVP

- Normal operation is live interface capture.
- Manual PCAP creation is not part of the normal MVP workflow.
- Offline PCAP ingestion is not required for MVP.

### Filter Before Userspace

- Only explicitly monitored service ports enter the Rust analyzer.
- Kernel BPF filtering is required before userspace packet processing.
- Do not capture all traffic and filter later in Rust.

### Session-Oriented Persistence

- Persist reconstructed sessions, not packets.
- Store reconstructed `c2s` and `s2c` streams.
- Do not create a database row per captured packet.

### Flag Scan After Reassembly

- Use one global `FLAG_REGEX`.
- Scan reconstructed stream data.
- Do not scan raw packets independently.
- If S2C traffic contains a flag, preserve enough metadata/pointers to inspect the related C2S payload/request.
- This must not turn SQLite into payload storage.

### SQLite Is Metadata Only

- SQLite stores metadata, indexes, source config, segment location, flag metadata and Suricata correlation metadata.
- SQLite must not become the main payload store.

### Payload Storage Is Append-Only

- Reconstructed payload bytes go into rotating append-only segment files.
- Default conceptual policy: `SEGMENT_DURATION=30m`, `SEGMENT_RETENTION_COUNT=3`.
- Store direct byte location: `segment_id`, `segment_offset`, `record_length`.

### No Heavy External Storage in MVP

- Do not introduce PostgreSQL, TimescaleDB, ClickHouse, Elasticsearch, Redis or another external DB without explicit user approval.

### No Raw PCAP Retention

- Raw packet persistence is out of MVP.
- Packets can be discarded after active reassembly state incorporates the needed information.

### No Automatic SQLite Vacuum Work in MVP

- Retention deletes expired segment metadata.
- Do not run `VACUUM` every rotation.

### Suricata Is Parallel Enrichment

- Suricata receives the monitored traffic independently.
- Suricata must not sit in front of Rust analyzer.
- Analyzer remains usable without Suricata.

### Packmate-Like UX

- Sources/services by port.
- Session list.
- Bidirectional reconstructed traffic detail.
- Payload search.
- Clear stolen-flag visibility and flag-only filtering.

### MVP Protocol Scope

- `raw_tcp`
- HTTP/1.x
- WebSocket

## Decision Log Template

Use this format for new decisions:

```text
Date:
Decision:
Why:
Alternatives:
Consequences:
Docs updated:
```

## Implemented Choices (2026-09-05)

- Capture uses direct Linux `libc` APIs for `AF_PACKET`, classic BPF attachment and kernel timestamps.
- Capture uses cooked `AF_PACKET/SOCK_DGRAM`; kernel BPF and Rust parsing operate on L3 IP offsets so WireGuard/TUN and Ethernet interfaces share one path. This replaced Ethernet-only `SOCK_RAW` after a real `wt0` capture exposed the mismatch.
- Capture socket is nonblocking/Tokio-backed; `PACKET_MMAP` remains the next measured optimization.
- Source changes rebuild the capture socket and BPF program.
- Flows are deterministically sharded to worker-local tables.
- Reassembly uses bounded per-direction buffers and `BTreeMap` pending segments.
- SQLite access uses `rusqlite` with bundled SQLite and WAL.
- HTTP metadata parsing uses `httparse`.
- Frontend uses React, TypeScript, Vite and npm.
- Segment format v1 is a 52-byte versioned header, CRC32, C2S bytes, then S2C bytes.
- Original topology: analyzer and Suricata used host networking; frontend used a bridge port. Superseded by host-network nginx for authentication on 2026-09-07.

## Implemented Choices (2026-09-06)

- `PROJECT.md` is the detailed product brief; `README.md` is the short operator runbook.
- `newhatch` remains the internal name and `Нюхач` is the UI/display name.
- Source TCP ports are unique. Deletion is soft so a later create on the same port restores the row.
- Session API pages are capped at 200 rows; payload substring search scans at most 2,000 metadata-prefiltered candidates per request.
- The Sources UI performs local search by name or port and local sorting by name or port.
- Root `logo.png` is the canonical logo. Compose mounts it into frontend nginx, and `/logo.png` is served with `Cache-Control: no-store`.
- The opt-in `test-flag` Compose profile exposes an nginx fixture on TCP port 18080.
- Current Suricata Compose operation is passive IDS only. Its BPF filter is configured separately and is not synchronized with Sources.

## Implemented Authentication (2026-09-07)

The user-approved task in `.agents/tasks/active.md` is implemented with Argon2id and tower-sessions. A custom store bounds ephemeral sessions to 1024; restart revokes all logins. TTL is absolute (default 24h), login explicitly cycles IDs and logout revokes server state. Password verification runs off Tokio workers with one concurrent verifier. Cookies are HttpOnly/Strict; Secure is explicit for HTTPS. Backend checks actual loopback TCP peers; nginx and analyzer share host networking. Frontend uses same-origin requests, login gating and centralized 401 handling; permissive CORS is removed. See `docs/authentication.md`.

nginx startup validates and renders `AUTH_ALLOWED_SUBNETS`. Empty/missing means loopback-only; malformed/non-canonical CIDRs fail startup. No forwarded headers grant trust. Docker E2E verifies real team and denied peers in a shared namespace, including IPv6. Production host-network ingress still requires an actual team-machine check on the target Linux deployment.

## Remaining Implementation Details

- WebSocket parser.
- Suricata EVE ingestion, correlation mechanism and live filter synchronization.
- Extremely long-lived TCP session handling.
- Packet loss/reassembly gap behavior.
- Segment tail recovery and SQLite reconciliation.
- Target-host VLAN behavior and IPv6 extension-header handling.
- Exact request/reply linkage model for flag-containing server responses.
- Meaning of `docs/todo.md` "Auto-removal" note: cleanup policy, query budget or write batching.
