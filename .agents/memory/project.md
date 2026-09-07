# Project Memory

## Summary

`newhatch` is the internal project name. The user-facing UI name is `Нюхач`.

The product is a lightweight, high-performance traffic analyzer for Attack/Defence CTF competitions. It aims for a Packmate-like workflow with lower CPU and RAM use on a constrained vulnbox. It is not a general-purpose enterprise IDS or SIEM.

## Documentation Ownership

- `PROJECT.md`: product brief, fixed product direction and implementation summary.
- `README.md`: short startup and operator runbook.
- `docs/agent-decisions.md`: fixed technical decisions and current implementation choices.
- `docs/architecture.md`: capture, flow, reassembly, protocol, storage and Suricata design.
- `docs/storage.md`: implemented SQLite and append-only segment model.
- `docs/mvp.md`: target UX plus current UI/API coverage.
- `docs/todo.md`: unresolved user notes; do not silently resolve them.

## Product Goals

- High packet/session throughput with bounded memory.
- No userspace work for traffic outside explicitly monitored TCP ports.
- Responsive session browsing during an A/D game.
- Immediate visibility of stolen-flag traffic.
- Simple local deployment through Docker Compose.

## Normal Workflow

1. Configure capture, an Argon2id login hash and allowed team CIDRs in `.env`.
2. Start analyzer and frontend, then sign in.
3. Add monitored sources/services by TCP port in the UI.
4. Browse reconstructed sessions and inspect C2S/S2C payloads.
5. Search payloads or filter flag-containing sessions.

Normal operation is live capture; users should not create PCAP files manually.

## Core Stack

- Rust and Tokio.
- Linux `AF_PACKET` with classic kernel BPF filters.
- `PACKET_MMAP` / RX ring remains a future measured optimization.
- Suricata as optional parallel IDS/enrichment.
- SQLite for metadata and indexes only.
- Versioned append-only segment files for reconstructed payload bytes.
- React, TypeScript and Vite frontend.
- Docker Compose runtime.

## Current Implementation

- `crates/analyzer`: live capture, packet parsing, flow sharding, TCP reassembly, flag scanning, HTTP metadata classification, storage and Axum API.
- `frontend`: operational Sessions and Sources views. The UI is branded `Нюхач`; Sources supports local name/port search and sorting.
- `compose.yaml`: analyzer and frontend runtime, a passive Suricata service, and the opt-in `test-flag` profile.
- `test/flag_test/`: nginx fixture on TCP port `18080`; `GET /flag` returns a test flag.
- `test/auth/`: backend, CIDR, multi-subnet Docker and browser authentication tests.
- Root `logo.png` is canonical. Compose mounts it read-only at `/opt/newhatch/logo.png`; startup copies it into the web root with readable permissions. Browser caching is disabled.
- Default UI URL: `http://localhost:8080`.

## Implemented Boundaries

- Sources are unique by TCP port and soft-deleted in SQLite.
- Enabled source changes rebuild the analyzer capture socket and BPF program.
- Ethernet parsing supports untagged IPv4 and basic IPv6/TCP; VLAN and IPv6 extension headers are not handled yet.
- WebSocket upgrades are classified, but frames are not decoded.
- Session list API pagination is cursor-based with a maximum page size of 200.
- Payload substring search scans at most 2,000 metadata-prefiltered candidates per request.
- Suricata is passive and independent. EVE ingestion, alert correlation and live filter synchronization are not implemented.
- Segment tail recovery/reconciliation is not implemented.

## Core Data Model

The user-visible entity is a reconstructed bidirectional TCP session with metadata plus C2S and S2C byte streams. Raw packets are not persisted. SQLite stores metadata and direct segment locations; segment files store payload bytes.

## Current Open Decisions

Single-user authentication is implemented: required Argon2id PHC credentials, tower-sessions with a bounded ephemeral store (1024 sessions), absolute TTL (default 24h), cookie rotation and logout. nginx checks actual client peers against loopback plus `AUTH_ALLOWED_SUBNETS`; analyzer binds loopback and checks actual peer plus session. Both production services use host networking. Empty CIDRs mean loopback only. No forwarded-header trust or permissive CORS. See `docs/authentication.md` and `.agents/tasks/active.md`; final vulnbox ingress validation remains pending. Roles, registration and external identity providers remain excluded.

- WebSocket frame parser and representation.
- Suricata event correlation and filter synchronization.
- Request/reply linkage for flag-containing server responses.
- Long-lived session policy and behavior under large reassembly gaps.
- Crash-tail recovery/reconciliation.
- VLAN and IPv6 extension-header handling.
- Meaning of the `rows >= 5000 OR query time >= 100 ms` auto-removal note.
