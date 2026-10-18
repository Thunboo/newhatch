# Open Questions

Clarify these when implementing the related area. Unfinished work is in
[backlog](../tasks/backlog.md); do not duplicate the questions there.

| Area | Unresolved choice |
| --- | --- |
| Flags | Application/player identity and request-level linkage for non-HTTP traffic |
| Protocols | WebSocket parser, frame representation and request/response boundaries in detail |
| Reassembly | Production policy for very long-lived non-HTTP flows and large TCP gaps |
| Capture | VLAN behavior, IPv6 extension headers, benchmark-justified PACKET_MMAP strategy and RX-ring size |
| Recovery | Truncated segment tails and SQLite/file reconciliation |
| Suricata | Correlation key and live Source/filter synchronization |
| Retention | Whether immediate disk reclamation is needed beyond whole-segment retention |
| Personal classification (#29) | Define custom-folder overlap/priority and manual-assignment behavior, stable session/Source references across catalog replacement or ID reuse and retention cleanup, plus request encoding and bounded classification limits. Agreed browser persistence and backend evaluation are in [session filtering](../../docs/session-filtering-v2.md#planned-personal-classification-29). |
| Collector security | PSK format/rotation, replay protection and need for encryption |
| Unassigned note | What subsystem the tentative threshold rows >= 5000 OR query time >= 100 ms governs: retention, query/search budgets, batching or something else. It is not the current writer's 500-session batch limit. |
