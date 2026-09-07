# Open Questions

Questions that should be clarified before the related implementation step.

## Product / UX

- When a server response contains a flag, what exactly should identify the player: source IP, source team, source service, authenticated user if parsed, or another mapping?
- Should session detail jump to request/response boundaries for HTTP/WebSocket when showing how a flag was obtained?

## Storage / Retention

- `docs/todo.md` mentions auto-removal with `rows >= 5000 OR query time >= 100 ms (maybe)`. Clarify whether this is retention cleanup, search candidate limiting, SQLite batch policy, or another mechanism.
- For flag-containing S2C replies, decide the exact SQLite metadata fields for linking to the related C2S payload range.

## Implementation Choices

- Collector admission is resolved for the initial split: collector pins the receiver IP/port and receiver permits only a configured collector source IP. There is no application-layer authentication; PSK hardening is tracked in backlog.
- WebSocket parser crate.
- Suricata EVE ingestion, correlation and live filter synchronization. Current Compose operation is passive and uses a separate static BPF expression.
- `PACKET_MMAP` ring sizing after capture benchmarks.
- VLAN-aware BPF and IPv6 extension-header behavior.
- Crash-tail repair and SQLite reconciliation for append-only segments.
- Production policy for very long-lived or highly gapped TCP sessions.
