# Fixed Decisions and Constraints

Treat these decisions as fixed unless the user explicitly changes them.
PROJECT.md describes the product; focused documents own implementation contracts.

## Stack and scope

Use Rust/Tokio, Linux AF_PACKET, kernel BPF, SQLite metadata/indexes,
custom append-only payload segments, a TypeScript frontend and Docker Compose.
Evaluate PACKET_MMAP/RX rings when measurements justify them.
Suricata is optional parallel enrichment.

MVP protocols are raw_tcp, HTTP/1.x and WebSocket. Keep a Packmate-like workflow:
sources, reconstructed sessions, bidirectional inspection, search and flag visibility.

Without an explicit product decision, do not introduce:
- PostgreSQL, TimescaleDB, ClickHouse, Elasticsearch, Redis or other external databases.
- Prometheus/Grafana, Kubernetes, HA/database replication or fleet orchestration.
- Permanent PCAP/raw-packet persistence or a database row per packet.
- Generic SIEM/DPI/plugin scope, UDP sessions, HTTP/2 or QUIC.
- RBAC, registration, external identity providers or automatic SQLite VACUUM.

## Architecture invariants

- Normal operation is live interface capture; manual/offline PCAP ingestion is not an MVP requirement.
- Reject unmonitored traffic in the kernel before userspace. Do not capture everything and filter it later in Rust.
- The analyzer owns reconstructed sessions. Suricata must stay outside its hot path and remain optional.
- Persist sessions, not packets. SQLite holds metadata; C2S/S2C bytes stay in append-only segments addressed by segment_id, segment_offset and record_length.
- Detect flags on reconstructed streams with one installation-wide FLAG_REGEX. Preserve enough context to inspect the client traffic associated with flag-bearing replies; exact linkage remains unresolved.
- Bound queues, flow state and read work. Prefer observable drops and explicit backpressure over unbounded buffering.
- Keep collectors diskless and lightweight. Analyzer owns reassembly, enrichment, persistence and chain correlation; isolate flow identity by collector.
- Use capture timestamps rather than processing/insertion time for traffic chronology.
- A storage failure must be visible; slow readers, absent frontend or absent Suricata must not control ingest.
- Prefer simple local mechanisms and measured changes over speculative infrastructure.

## Feature contracts

These documents are authoritative; do not replace their agreed rules with a new design during unrelated work:

| Subject | Contract |
| --- | --- |
| Capture, reassembly, split transport and Suricata | [Architecture](architecture.md) |
| Planned atomic HTTP session model (#28) | [Atomic HTTP Sessions](http-sessions.md) |
| Segment format, writer ordering and retention | [Storage](storage.md) |
| Single-user login and two independent access checks | [Authentication](authentication.md) |
| UI, payload rendering and individual HTTP export | [MVP UX](mvp.md) |
| Whole-session correlation, card behavior, snapshots and chain export preparation | [Session chains](session-chains.md) |
| Python destination/Host override, actual response output and cookies | [Python replay](python-replay.md) |
| Literal AND chips, edited/applied state, limits and stale-response handling | [Session filtering](session-filtering-v2.md) |

Resource priorities are low vulnbox overhead, high ingest/reassembly throughput,
bounded memory, responsive browsing, no work for unmonitored traffic, correct
streams and extensibility. Do not silently change the stack to resolve a limitation.

## Unresolved decisions

The single list is [open questions](../.agents/memory/open-questions.md).
Clarify an item when work reaches that area; an unresolved detail is not permission
to replace the fixed architecture. Implementation work is tracked in
[backlog](../.agents/tasks/backlog.md). Agent workflow and documentation rules live
in AGENTS.md.
