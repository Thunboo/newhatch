# Codex Context

## Product

`newhatch` is the internal name of a lightweight A/D CTF traffic analyzer. The UI/display name is `Нюхач`. It provides a Packmate-like sources, sessions, payload inspection, search and stolen-flag workflow while targeting constrained vulnbox resources.

Read `PROJECT.md` for the product brief and implementation summary. Treat `README.md` as the short startup runbook.

## Core Pipeline

```text
NIC
 |
 | classic kernel BPF: enabled monitored TCP ports only
 v
Linux AF_PACKET socket (nonblocking recvmsg)
 |
 v
consistent flow-worker sharding
 |
 +-> bounded TCP reassembly: C2S and S2C
 +-> streaming plus final flag scan
 +-> HTTP metadata / WebSocket-upgrade classification
 |
 v
session metadata -> SQLite
session payload  -> versioned append-only segment files
```

Suricata is an optional passive IDS running in parallel. It is not in the analyzer hot path, and its EVE output is not ingested or correlated yet.

## Current Repository State

- `crates/analyzer`: Rust/Tokio capture, flow, reassembly, protocol classification, storage and Axum API.
- `frontend`: React/TypeScript/Vite UI branded `Нюхач`.
- `compose.yaml`: analyzer, frontend, passive Suricata and opt-in `test-flag` services.
- `test/`: nginx test endpoint at `GET /flag` on TCP port `18080`.
- Root `logo.png`: canonical UI logo mounted directly into frontend nginx.
- Default UI URL: `http://localhost:8080`.

## Implemented Behavior

- Source CRUD with unique TCP ports, soft deletion and live analyzer BPF rebuilds.
- Local Sources search by name/port and sorting by name/port.
- Cursor-based session browsing; API page size is capped at 200.
- Metadata filters for source, flag presence, protocol, endpoint IP/port and start time.
- Bounded payload substring search over at most 2,000 metadata-prefiltered candidates per request.
- Text and hex payload views plus flag-match highlighting.
- Version 1 segment records: 52-byte header, CRC32, C2S bytes, then S2C bytes.
- Segment rotation/retention and batched SQLite inserts of up to 500 sessions.

## Known Limitations

- Capture is Linux-only and has not yet been exercised on the final vulnbox topology.
- `PACKET_MMAP` is not implemented; capture currently uses `recvmsg`.
- No VLAN parsing or IPv6 extension-header walking.
- WebSocket upgrades are classified, but frames are not decoded.
- No Suricata EVE ingestion, session correlation or live source-filter synchronization.
- No crash-tail repair/reconciliation for the latest segment.
- No explicit request/reply payload-range linkage for flag-containing server responses.
- The UI exposes only source, protocol, flag and payload filters although the API supports more endpoint/time filters.

## Guardrails

- Filter irrelevant traffic in the kernel before userspace.
- Persist sessions, never raw packets or one database row per packet.
- Keep payload bytes out of SQLite.
- Keep queues and active flow state bounded; make drops observable.
- Keep Suricata optional and outside the Rust hot path.
- Keep MVP protocol scope to raw TCP, HTTP/1.x and WebSocket.
- Do not introduce heavyweight infrastructure without explicit user approval.

## Documentation Map

- `PROJECT.md`: product brief and implementation status.
- `README.md`: quick start.
- `docs/agent-decisions.md`: fixed decisions and implementation choices.
- `docs/architecture.md`: system design and component status.
- `docs/storage.md`: SQLite/segment format and persistence behavior.
- `docs/mvp.md`: target UX and current coverage.
- `docs/todo.md`: unresolved user notes.
