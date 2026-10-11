# Open Questions

Clarify these when implementing the related area. Unfinished work is in
[backlog](../tasks/backlog.md); do not duplicate the questions there.

| Area | Unresolved choice |
| --- | --- |
| Flags | Identity/ranges linking flag-bearing S2C replies to triggering player/C2S requests |
| Protocols | WebSocket parser and frame representation; explicit HTTP/WebSocket request/response boundaries in detail |
| Reassembly | Production policy for very long-lived flows and large TCP gaps |
| Capture | VLAN behavior, IPv6 extension headers, benchmark-justified PACKET_MMAP strategy and RX-ring size |
| Recovery | Truncated segment tails and SQLite/file reconciliation |
| Suricata | Correlation key and live Source/filter synchronization |
| Retention | Whether immediate disk reclamation is needed beyond whole-segment retention |
| Collector security | PSK format/rotation, replay protection and need for encryption |
| Unassigned note | What subsystem the tentative threshold rows >= 5000 OR query time >= 100 ms governs: retention, query/search budgets, batching or something else. It is not the current writer's 500-session batch limit. |
