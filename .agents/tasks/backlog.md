# Backlog

Задачи, которые пока не взяты в работу.

## Project Setup

- [x] Create initial Rust workspace for analyzer/backend.
- [x] Create TypeScript frontend scaffold.
- [x] Create Docker Compose scaffold for `analyzer`, `frontend`, `suricata`.
- [x] Add basic config loading for `CAPTURE_INTERFACE`, `FLAG_REGEX`, `SEGMENT_DURATION`, `SEGMENT_RETENTION_COUNT`, `DATA_DIR`.

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

## MVP Implementation Milestones

- [x] Source/service CRUD with `name`, `port`, `enabled`.
- [x] Generate kernel BPF filter from enabled sources.
- [x] Live capture with `AF_PACKET`.
- [x] TCP flow tracking and bidirectional session identity.
- [x] TCP reassembly with split-flag-safe stream scanning.
- [x] HTTP/1.x metadata parsing.
- [ ] WebSocket upgrade/frame extraction.
- [x] Append-only segment writer.
- [x] SQLite metadata schema and initial indexes.
- [x] Session list API with bounded pagination/cursors.
- [x] Payload retrieval API by segment location.
- [x] Payload search through SQLite prefilter plus Rust range scan.
- [x] Packmate-like UI: sources, session list, session detail, search and flag filter.
- [ ] Suricata alert ingestion/correlation as optional enrichment.
