# `newhatch` - High-Performance A/D Traffic Analyzer

## Project Goal

Build a lightweight, high-performance traffic analysis tool for Attack/Defence CTF competitions.

The tool should provide a workflow and UI comparable to Packmate, while using fewer resources on a typical vulnbox and remaining responsive during a full game.

The primary design priorities are:

1. High packet/session processing throughput.
2. Low CPU and RAM overhead on the vulnbox.
3. Do not process traffic that the user did not explicitly choose to monitor.
4. Keep the UI responsive during long Attack/Defence games.
5. Make stolen-flag traffic immediately visible.
6. Keep the architecture simple enough to run with Docker Compose.
7. Avoid production-enterprise infrastructure that is unnecessary for a CTF tool.

This is **not** intended to become a general-purpose enterprise IDS platform.

## Core Stack

- Rust
  - packet ingest
  - TCP flow tracking
  - TCP reassembly
  - session assembly
  - HTTP/WebSocket parsing
  - flag detection
  - backend API
- Tokio
- Linux `AF_PACKET`
- `PACKET_MMAP` / RX ring where appropriate
- kernel BPF capture filters
- Suricata
  - runs in parallel with the Rust analyzer
  - IDS / enrichment / alert source
  - must not be in the Rust analyzer hot path
- SQLite
  - metadata and indexes only
- Custom append-only segment files
  - reconstructed session payloads
- TypeScript frontend
- Docker Compose

## Why Suricata

Suricata runs in parallel with the Rust analyzer as an optional IDS layer. The analyzer remains the source of reconstructed TCP sessions and flag-leak evidence; Suricata adds attack signatures, protocol/security alerts and triage context.

In Attack/Defence, that context can help identify an exploit and prepare a narrow temporary blocking rule while the service is patched. The current Compose configuration is passive and does not block traffic. Inline `drop` would require separate deployment work and carefully reviewed rules to avoid false positives that break checkers and SLA. EVE ingestion and session correlation are not implemented yet.

## Explicitly Not Used

The current version must not introduce:

- PostgreSQL
- TimescaleDB
- ClickHouse
- Elasticsearch
- Prometheus
- Kubernetes
- permanent PCAP storage
- raw packet persistence
- a database row per captured packet

Since it may lower the performance of a typical A/D vulnbox

## Capture Model

The player configures which service ports should be monitored.

Example:

```text
service-a -> 8080
service-b -> 9000
```

The capture filter must be applied **before packets enter the application**.

Conceptually:

```text
NIC
 |
 | kernel BPF:
 | tcp and (port 8080 or port 9000)
 |
 v
AF_PACKET
 |
 v
Rust analyzer
```

Traffic outside configured monitored ports should not be copied into the analyzer and should consume as little application CPU/RAM as possible.

## User-Level Data Model

The primary entity visible to the user is a reconstructed bidirectional TCP session:

```text
SESSION
  ├── metadata
  ├── client -> server reconstructed bytes
  └── server -> client reconstructed bytes
```

The MVP should not persist individual raw packets.

## MVP Protocol Support

Required:

- generic TCP session tracking
- HTTP/1.x
- WebSocket

Out of scope for the first MVP:

- broad generic DPI
- DNS analysis
- UDP session model
- HTTP/2
- QUIC
- arbitrary protocol plugins

The architecture should allow future protocol support without redesigning the storage format.

## Flag Detection

A single environment variable defines the flag regex for the entire installation:

```text
FLAG_REGEX=<regex>
```

The regex is applied after TCP reassembly, not independently to each packet.

This avoids missing a flag that is split across multiple TCP segments.

The analyzer should detect flags while reconstructed stream data becomes available, rather than waiting for the TCP connection to close when practical.

For every session, store at least:

```text
contains_flag
flag_direction
flag_count
```

Possible directions:

```text
none
c2s
s2c
both
```

The frontend must:

- visibly highlight sessions containing flags
- provide a filter equivalent to "show only stolen-flag traffic"

## Runtime Model

Target usage:

```text
docker compose up -d
```

The user should primarily need to:

1. choose the network interface
2. configure monitored sources/services and ports
3. provide `FLAG_REGEX`
4. configure a login hash and allowed team subnets
5. start the stack and sign in

The user should not need to manually capture PCAP files.

## Expected Environment

Design for a typical Attack/Defence game rather than a large enterprise sensor.

Working sizing assumption:

- 1 Gbit network
- approximately 8 hours of gameplay
- approximately 20 teams
- approximately 5 players per team
- a vulnbox with constrained CPU/RAM

These values are not strict capacity guarantees. They exist to guide architecture decisions toward low overhead and predictable memory use.

## Important Design Principle

Performance must be achieved primarily by:

- rejecting irrelevant traffic in the kernel
- keeping packet processing in Rust
- avoiding unnecessary allocations and copies
- bounded queues and explicit backpressure
- keeping active flow state bounded
- sequential append-only writes
- storing metadata separately from payload
- avoiding heavyweight external databases

Do not assume that rewriting something in Rust alone guarantees higher performance.

## Current Implementation

The first executable vertical slice includes:

- Linux cooked `AF_PACKET` live capture with a classic kernel BPF port filter, including L3 WireGuard/TUN interfaces
- bounded packet queues and consistently sharded flow workers
- bidirectional TCP stream reconstruction with basic out-of-order and retransmit handling
- flag detection over reconstructed streams
- HTTP metadata and WebSocket upgrade classification
- SQLite metadata plus versioned append-only payload segments
- source CRUD, bounded session browsing, payload retrieval and payload search APIs
- a TypeScript/React UI for sources, filters, session browsing and text/hex payload inspection
- `Нюхач` UI branding plus local source search and sorting by name or port
- Docker Compose services for analyzer, frontend and parallel passive Suricata capture
- an opt-in nginx flag-capture fixture on TCP port 18080
- single-user Argon2id authentication, bounded server-side sessions, loopback-only API and nginx team-CIDR access control; see `docs/authentication.md`

The next capture milestone is `PACKET_MMAP`/RX ring support after the socket path is measured. WebSocket frame decoding, Suricata event correlation, segment tail recovery, IPv6 extension headers and target-host VLAN validation remain incomplete.
