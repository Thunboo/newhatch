# Project Memory

Устойчивые факты о проекте.

## Summary

`newhatch` is a lightweight, high-performance traffic analysis tool for Attack/Defence CTF competitions.
It should provide a Packmate-like workflow and UI while using fewer resources on a typical vulnbox.

This is not a general-purpose enterprise IDS/SIEM platform.

## Product Goal

- High packet/session processing throughput.
- Low CPU and RAM overhead on constrained vulnboxes.
- No userspace work for traffic the user did not explicitly monitor.
- Responsive UI during long A/D games.
- Immediate visibility of stolen-flag traffic.
- Simple Docker Compose deployment.

## User Workflow

1. Start the stack.
2. Choose a network interface.
3. Configure monitored sources/services by TCP port.
4. Provide `FLAG_REGEX`.
5. Browse reconstructed sessions.
6. Inspect client/server reconstructed traffic.
7. Search payloads and filter flag-containing sessions.

The user should not manually create PCAP files for normal operation.

## Core Stack

- Rust.
- Tokio.
- Linux `AF_PACKET`.
- `PACKET_MMAP` / RX ring where useful.
- kernel BPF capture filters.
- Suricata as parallel IDS/enrichment.
- SQLite for metadata/indexes only.
- custom append-only segment files for reconstructed payloads.
- TypeScript frontend.
- Docker Compose.

## Core Data Model

The user-visible entity is a reconstructed bidirectional TCP session:

```text
SESSION
  ├── metadata
  ├── client -> server reconstructed bytes
  └── server -> client reconstructed bytes
```

Individual raw packets are not persisted in MVP.

## MVP Protocols

- `raw_tcp`
- `http` / HTTP/1.x
- `websocket`

Out of MVP: broad generic DPI, DNS analysis, UDP session model, HTTP/2, QUIC, arbitrary protocol plugins.

## Flag Detection

- Global config: `FLAG_REGEX=<regex>`.
- Scan reconstructed stream data, not individual packets.
- Store per-session metadata: `contains_flag`, `flag_direction`, `flag_count`.
- Directions: `none`, `c2s`, `s2c`, `both`.
- UI must highlight flag sessions and support "show only traffic containing flags".
- When a server reply contains a flag, the product should make it possible to inspect what the client/player sent to get that flag.
- This should use SQLite metadata and segment pointers, not payload bytes stored directly in SQLite.

## Expected Environment

- Typical Attack/Defence CTF game.
- Working sizing assumption: 1 Gbit network, about 8 hours, about 20 teams, about 5 players per team.
- Constrained CPU/RAM on vulnbox.

## Current Code

- `crates/analyzer`: Rust capture, flow/reassembly, storage and Axum API.
- `frontend`: React/TypeScript/Vite operational UI.
- `compose.yaml`: analyzer, frontend and Suricata runtime.
- UI URL: `http://localhost:8080`.
- Current limitations and next milestones are listed in `README.md` and `.agents/tasks/active.md`.

## Canonical Docs

- `README.md`
- `docs/agent-decisions.md`
- `docs/architecture.md`
- `docs/storage.md`
- `docs/mvp.md`
- `docs/todo.md`
