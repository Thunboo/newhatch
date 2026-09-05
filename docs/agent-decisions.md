# Agent Decisions and Constraints

This file contains project decisions that the coding agent should treat as fixed unless the user explicitly changes them.

## Fixed Technology Decisions

Use:

```text
Rust
Tokio
AF_PACKET
PACKET_MMAP where useful
kernel BPF filtering
Suricata
SQLite
custom append-only segment storage
TypeScript frontend
Docker Compose
```

Do not replace these with a different stack without explicit user approval.

## Fixed Architectural Decisions

### 1. Live Capture Only for MVP

Primary operation is live interface capture.

The normal user workflow is:

```text
choose interface
configure monitored sources/ports
start analyzer
```

Do not make manual PCAP creation part of the normal workflow.

Offline PCAP ingestion is not an MVP requirement.

### 2. Filter Before Userspace

Only explicitly monitored service ports should enter the Rust analyzer.

Use a kernel capture filter.

Do not capture all traffic and filter it later in Rust.

### 3. Session-Oriented Persistence

Persist reconstructed sessions.

Do not persist every packet.

Primary model:

```text
SESSION
  ├── metadata
  ├── reconstructed C2S bytes
  └── reconstructed S2C bytes
```

### 4. Flag Scan After Reassembly

Flag detection must run on reconstructed stream data.

Do not scan raw packets independently.

One global regex per installation:

```text
FLAG_REGEX
```

### 5. SQLite Is Metadata Only

SQLite stores:

- session metadata
- indexes
- segment location
- source configuration
- flag metadata
- Suricata correlation metadata

SQLite must not become the main payload store.

### 6. Payload Storage Is Append-Only

Use rotating append-only segment files.

Default conceptual policy:

```text
SEGMENT_DURATION=30m
SEGMENT_RETENTION_COUNT=3
```

Store direct byte location:

```text
segment_id
segment_offset
record_length
```

### 7. No PostgreSQL

Do not introduce PostgreSQL for MVP.

Do not add TimescaleDB, ClickHouse, Elasticsearch, Redis, or another external database unless the user explicitly revisits storage architecture.

### 8. No Raw PCAP Retention

Do not persist full packet captures in MVP.

After TCP reassembly has incorporated packet data, raw packet persistence is unnecessary.

### 9. No SQLite Vacuum Work in MVP

Retention deletes metadata for expired segments.

Automatic SQLite compaction/vacuum is out of scope.

### 10. Suricata Is Parallel Enrichment

Do not put Suricata in the Rust analyzer hot path.

Preferred model:

```text
monitored traffic
   +--> Rust analyzer
   +--> Suricata
```

The analyzer must still function if Suricata is unavailable.

### 11. Packmate-Like UX

The UI should be conceptually similar to Packmate:

- sources grouped/configured by port
- session list
- bidirectional reconstructed traffic inspection
- payload search
- clear stolen-flag visibility
- filter to only traffic containing flags

Do not redesign the product into a generic SIEM dashboard.

### 12. Protocol Scope

MVP:

```text
raw TCP
HTTP/1.x
WebSocket
```

Do not expand into a generic DPI framework before the core pipeline works.

## Performance Priorities

In order:

1. low resource use on vulnbox
2. high ingest/reassembly speed
3. bounded memory
4. responsive browsing/filtering
5. no unnecessary work for unmonitored traffic
6. correctness of reconstructed streams
7. extensibility

A more complex architecture is not automatically better.

Prefer simple local mechanisms when they satisfy the workload.

## Expected Agent Behavior

Before generating production code, the agent should:

1. inspect all project docs
2. keep terminology consistent with these files
3. avoid silently changing fixed decisions
4. identify unresolved details as TODO/TBD instead of inventing incompatible architecture
5. keep the MVP scope narrow
6. favor measurable/simple hot-path designs
7. avoid adding infrastructure dependencies "for future scalability"

## Known Unresolved Details

The following may still require implementation-time decisions:

- exact WebSocket parser
- exact Suricata correlation mechanism
- exact handling of extremely long-lived TCP sessions
- exact behavior under packet loss or reassembly gaps

These are not permission to replace the agreed architecture.

The agent should select the simplest implementation compatible with the fixed decisions and clearly document choices.

## Initial Implementation Choices

The first executable slice currently uses:

- direct `libc` calls for Linux `AF_PACKET`, `SO_ATTACH_FILTER` and kernel receive timestamps
- a nonblocking packet socket registered with Tokio; `PACKET_MMAP` is the next measured capture optimization
- a rebuilt capture socket when enabled source ports change
- deterministic flow hashing into worker-local `HashMap` flow tables
- a per-direction `BTreeMap` for bounded out-of-order TCP payloads
- `rusqlite` with bundled SQLite and WAL mode
- `httparse` for HTTP/1.x request/response metadata
- React, TypeScript, Vite and npm for the frontend
- host networking plus `NET_RAW`/`NET_ADMIN` for analyzer capture; frontend remains on a published bridge port
- a 30-second configurable flow idle timeout and a configurable per-direction stream byte limit

These are implementation decisions, not changes to the fixed architecture. Current limitations are recorded in `README.md`.
