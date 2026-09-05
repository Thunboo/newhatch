# Decisions

Журнал важных решений.

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
- Capture socket is nonblocking/Tokio-backed; `PACKET_MMAP` remains the next measured optimization.
- Source changes rebuild the capture socket and BPF program.
- Flows are deterministically sharded to worker-local tables.
- Reassembly uses bounded per-direction buffers and `BTreeMap` pending segments.
- SQLite access uses `rusqlite` with bundled SQLite and WAL.
- HTTP metadata parsing uses `httparse`.
- Frontend uses React, TypeScript, Vite and npm.
- Segment format v1 is a 52-byte versioned header, CRC32, C2S bytes, then S2C bytes.
- Analyzer and Suricata use host networking with packet capabilities; frontend publishes port 8080 from a bridge network.

## Unresolved Details

- WebSocket parser.
- Suricata correlation mechanism.
- Extremely long-lived TCP session handling.
- Packet loss/reassembly gap behavior.
- Exact request/reply linkage model for flag-containing server responses.
- Meaning of `docs/todo.md` "Auto-removal" note: cleanup policy, query budget or write batching.
