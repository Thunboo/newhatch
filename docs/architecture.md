# Architecture

## High-Level Architecture

```mermaid
flowchart TD
    NIC["Network Interface"]
    BPF["Kernel BPF Filter<br/>configured monitored ports only"]

    NIC --> BPF

    BPF --> RUST["Rust Analyzer<br/>AF_PACKET / PACKET_MMAP"]
    BPF --> SURI["Suricata"]

    RUST --> FLOW["TCP Flow Tracking"]
    FLOW --> REASM["TCP Reassembly"]
    REASM --> PARSE["HTTP / WebSocket Parsing"]
    REASM --> FLAG["Streaming Flag Detection"]

    PARSE --> SESSION["Session Finalization"]
    FLAG --> SESSION

    SESSION --> SEG["Append-Only Segment Storage"]
    SESSION --> SQLITE["SQLite Metadata Index"]

    SURI --> CORR["Suricata Event Correlation"]
    CORR --> SQLITE

    SQLITE --> API["Rust Backend API"]
    SEG --> API

    API --> UI["TypeScript Frontend"]
```

## Core Principle

Suricata and the Rust analyzer are parallel consumers of the relevant traffic.

Suricata must **not** sit in front of the Rust analyzer:

```text
BAD:

NIC
 -> Suricata
 -> JSON
 -> Rust analyzer
```

That would add unnecessary serialization, parsing, I/O, and coupling.

Preferred architecture:

```text
                 +-> Rust analyzer -> sessions
NIC -> filter ---+
                 +-> Suricata -----> alerts/enrichment
```

The Rust analyzer is the source of truth for reconstructed traffic sessions.

Suricata is an optional/parallel enrichment and IDS source.

## Capture Layer

### Requirements

- Linux only for the first implementation.
- Capture from a live interface.
- Use `AF_PACKET`.
- Prefer `PACKET_MMAP` / RX ring based capture to reduce syscall overhead.
- Capture filter must be installed in the kernel.
- Traffic outside configured monitored ports should not enter the application hot path.

### Source Configuration

A "source" is a user-visible monitored service definition, similar to Packmate.

Minimum fields:

```text
id
name
port
enabled
```

Example:

```text
web-service
port 8080

flag-service
port 9000
```

The effective BPF filter can be generated from enabled sources.

Example:

```text
tcp and (port 8080 or port 9000)
```

The exact implementation may use classic BPF/eBPF or library abstractions, but the filtering decision must happen before userspace packet processing.

## Concurrency Model

The packet-processing hot path should avoid a global mutex.

Preferred conceptual design:

```text
NIC RX
  |
  v
capture
  |
  v
flow hash
  |
  +--> worker 0 -> local flow table
  +--> worker 1 -> local flow table
  +--> worker 2 -> local flow table
  +--> worker N -> local flow table
```

Packets belonging to one flow/session should consistently be handled by the same worker.

Possible flow key:

```text
src_ip
src_port
dst_ip
dst_port
protocol
```

Direction normalization should allow the same bidirectional TCP connection to resolve to one internal session identity.

### Backpressure

All inter-task channels on the ingest path must be bounded.

The application must prefer:

- observable drops
- explicit queue saturation
- bounded memory usage

over unbounded memory growth.

## TCP Reassembly

The analyzer must reconstruct two logical byte streams:

```text
client -> server
server -> client
```

Reassembly must account for at least:

- TCP sequence numbers
- out-of-order segments
- duplicate/retransmitted data
- connection close
- idle timeout
- partial/incomplete sessions

The session storage format should represent reconstructed bytes, not original packets.

## Flag Detection

Flag scanning occurs on reconstructed stream data.

Do not search each raw packet independently.

Example failure mode of packet-level scanning:

```text
packet 1:  FLAG{ABC
packet 2:  DEF123}
```

A packet-level search misses the flag.

A stream-level search sees:

```text
FLAG{ABCDEF123}
```

Flag scanning should be streaming where practical.

Global configuration:

```text
FLAG_REGEX=<regex>
```

Per-session metadata:

```text
contains_flag: bool
flag_direction: none | c2s | s2c | both
flag_count: integer
```

## Protocol Parsing

MVP:

### HTTP/1.x

Extract enough metadata for Packmate-like browsing:

- method
- host if available
- URI/path
- status code
- content type if available
- request/response direction
- request/response boundaries where possible

### WebSocket

Support:

- WebSocket upgrade detection
- frame extraction
- direction
- text/binary distinction where practical
- payload presentation in the session UI

### Raw TCP

If a session does not parse as HTTP/WebSocket, it must still remain available as a raw reconstructed TCP session.

## Suricata Integration

Suricata should receive the same monitored traffic independently.

Purpose:

- IDS alerts
- signatures
- protocol/security enrichment
- future triage features

Do not make Suricata output required for basic session capture.

If Suricata stops, the Rust analyzer should continue collecting sessions.

If the Rust analyzer stops, Suricata may continue independently.

### Correlation

Correlation mechanism is not fully fixed yet.

Preferred options to evaluate:

1. Community ID
2. normalized 5-tuple + timestamp window
3. another stable flow identifier if available

Do not invent a tight coupling before implementation evidence justifies it.

Store Suricata correlation as metadata, not inside payload segment records unless needed for recovery.

## Timestamp Model

Do not use "time when application logic happened" as the primary packet time.

Use receive/capture timestamps provided by the packet capture mechanism.

Session timestamps:

```text
started_at = first observed packet timestamp
ended_at   = last observed packet timestamp
```

Use a compact integer representation in storage, preferably Unix nanoseconds or microseconds.

## Failure Isolation

The Rust analyzer should continue to function if:

- frontend is unavailable
- API client disconnects
- Suricata is unavailable
- SQLite readers are slow

The storage writer is critical and must fail loudly if it cannot persist new session records.

## Out of Scope

Do not design for:

- Kubernetes orchestration
- multi-node clustering
- HA databases
- distributed ingestion
- distributed storage
- enterprise SIEM integrations
- multi-tenant authentication
