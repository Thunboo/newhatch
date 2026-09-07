# Backlog

Tasks that are not currently being implemented.

## Project Setup

- [x] Create initial Rust workspace for analyzer/backend.
- [x] Create TypeScript frontend scaffold.
- [x] Create Docker Compose scaffold for `analyzer`, `frontend`, `suricata`.
- [x] Add runtime configuration loading and validation for capture, storage, queue and flow limits.

## Architecture Decisions To Finalize

- [x] Choose direct Linux packet capture implementation.
- [x] Choose SQLite Rust library.
- [x] Choose HTTP parser crate.
- [ ] Choose WebSocket parser crate.
- [x] Choose TypeScript frontend framework/library.
- [x] Define initial Docker capabilities/network mode for packet capture.
- [x] Define initial session idle timeout policy.
- [x] Define exact binary segment record format.
- [ ] Define how flag-containing S2C replies link to triggering C2S/client payload.
- [ ] Clarify `docs/todo.md` auto-removal note: rows threshold, query-time threshold and target subsystem.
- [ ] Evaluate Suricata correlation strategy.

## Collector Transport Hardening

- [ ] Add per-collector pre-shared-key authentication to the collector/analyzer handshake. The initial split trusts the configured receiver endpoint on the collector and the configured collector source IP on the receiver; it does not provide application-layer authentication or encryption.

## MVP Implementation Milestones

- [x] Source/service CRUD with `name`, `port`, `enabled`.
- [x] Generate kernel BPF filter from enabled sources.
- [x] Live capture with `AF_PACKET`.
- [x] TCP flow tracking and bidirectional session identity.
- [x] TCP reassembly with split-flag-safe stream scanning.
- [x] HTTP/1.x metadata parsing.
- [ ] WebSocket frame extraction after the implemented HTTP Upgrade classification.
- [x] Append-only segment writer.
- [x] SQLite metadata schema and initial indexes.
- [x] Session list API with bounded pagination/cursors.
- [x] Payload retrieval API by segment location.
- [x] Payload search through SQLite prefilter plus Rust range scan.
- [x] Packmate-like UI: sources, session list, session detail, search and flag filter.
- [ ] Suricata alert ingestion/correlation as optional enrichment.
- [ ] Do not persist or display empty `raw_tcp` sessions with `0 B` reconstructed payload. Confirm that control-only TCP packets (SYN/FIN/RST without payload) still update/close flow state correctly, then discard the finalized session when both C2S and S2C payloads are empty.
- [ ] Add a Russian/English language switch for the frontend. Move visible UI strings into a small localization layer, persist the selected language in the browser, and default to the browser language when no preference has been saved.
