# newhatch / Нюхач

A lightweight live-traffic analyzer for Attack/Defence CTF competitions.
It aims for Packmate-like browsing with low CPU/RAM use on a constrained vulnbox.
The primary entity is a reconstructed bidirectional TCP session; normal operation
captures a configured interface without requiring manual PCAP files.

## Product direction

Players configure monitored service ports, inspect C2S/S2C traffic, search
payloads and identify flag-bearing sessions. Irrelevant traffic is rejected in
the kernel. Bounded ingest, worker-local state and sequential storage take
priority over extra infrastructure.

The sizing assumption is a 1 Gbit network, roughly eight hours of gameplay,
20 teams and five players per team. These are design inputs, not capacity guarantees.
The project is not a generic SIEM or enterprise IDS.

Fixed stack and scope: [decisions](docs/agent-decisions.md).
Startup and deployment: [README](README.md).

## Current implementation

| Area | Status and detailed contract |
| --- | --- |
| Capture | Linux cooked AF_PACKET, classic kernel BPF, capture timestamps, bounded queues and worker-local TCP reassembly; [architecture](docs/architecture.md) |
| Protocols and flags | Raw TCP, HTTP/1.x metadata, WebSocket Upgrade classification and reconstructed-stream flag detection |
| Persistence | SQLite metadata, versioned append-only payload segments and count-based retention; [storage](docs/storage.md) |
| UI | React/TypeScript, source CRUD/search/sorting, live session browsing, text/hex, JSON formatting, flag highlighting and EN/RU localization; [UX](docs/mvp.md) |
| Filtering | Literal AND chips for sessions and chains, separate edited/applied sets and backend-supplied limits; [filtering](docs/session-filtering-v2.md) |
| Chains | Optional whole-session correlation, bounded queries, full-context cards and lazy payload loading; [chains](docs/session-chains.md) |
| Export | Individual cURL/Python HTTP export and whole-chain Python replay; [Python contract](docs/python-replay.md) |
| Access | Single-user authentication, loopback analyzer API and nginx team-CIDR ingress; [authentication](docs/authentication.md) |
| Split capture | Diskless collector, framed protobuf transport, analyzer-pushed Sources, collector-isolated flows and status counters; [architecture](docs/architecture.md#remote-collector-boundary) |
| Suricata | Optional passive parallel sensor; EVE ingestion, correlation and filter synchronization are not implemented |

Docker Compose packages analyzer, frontend and optional Suricata/collector services.
A test-profile nginx fixture provides flag-capture traffic on port 18080.

## Remaining work

PACKET_MMAP requires measurement before adoption. WebSocket frame decoding,
Suricata enrichment, crash-tail reconciliation, IPv6 extension headers and
collector transport authentication/encryption remain incomplete. VLAN behavior,
long-lived/reassembly-gap policy and competition-host deployment need validation.

See [open questions](.agents/memory/open-questions.md) and
[unfinished work](.agents/tasks/backlog.md). Detailed verification commands live
in test/*/README.md; agents must follow AGENTS.md's Docker restriction.
