# Codex Context

Краткий контекст для будущих сессий Codex.

## What This Project Is

`newhatch` is a lightweight high-performance A/D CTF traffic analyzer.
It should feel familiar to a Packmate user while staying lighter on CPU/RAM.

Core loop:

```text
NIC
 |
 | kernel BPF: only monitored TCP ports
 v
AF_PACKET / PACKET_MMAP
 |
 v
Rust analyzer
 |
 +-> TCP flow tracking
 +-> TCP reassembly
 +-> streaming flag detection
 +-> HTTP/WebSocket parsing
 |
 v
SESSION metadata -> SQLite
SESSION payload  -> append-only segments
```

Suricata receives the same monitored traffic in parallel and provides optional alert enrichment.
It must not be in the Rust analyzer hot path.

## Current Repository State

- Rust workspace and analyzer/backend live in `crates/analyzer`.
- React/TypeScript/Vite frontend lives in `frontend`.
- `compose.yaml` runs analyzer, frontend and Suricata.
- The first vertical slice implements source CRUD, kernel BPF capture, flow reassembly, flag scanning, segment/SQLite storage, browsing/search APIs and payload UI.
- `README.md` lists current limitations and startup instructions.

## Important Not To Break

- Kernel filtering before userspace.
- Session-oriented persistence.
- No raw packet persistence in MVP.
- No database row per packet.
- SQLite metadata only.
- Payloads stored in custom append-only segment files.
- Flag detection after TCP reassembly.
- Flag-containing server replies should remain linked to the triggering client/player payload through metadata/pointers.
- MVP protocols only: `raw_tcp`, HTTP/1.x, WebSocket.
- Packmate-like UX, not generic SIEM dashboard.

## Useful Docs

- `README.md` - product goal and stack.
- `docs/agent-decisions.md` - fixed decisions and unresolved details.
- `docs/architecture.md` - system architecture.
- `docs/storage.md` - storage design.
- `docs/mvp.md` - UX and MVP scope.
- `docs/todo.md` - user notes.
