# Unfinished Work

Track only pending work here. Remove completed entries; history belongs in
GitHub issues. Clarify related design choices in
[open questions](../memory/open-questions.md) when implementing.

## Implementation

- [ ] Decode WebSocket frames after HTTP Upgrade classification.
- [ ] Ingest/correlate Suricata alerts and synchronize its capture filter.
- [ ] Add per-collector PSK authentication; resolve replay protection, rotation and encryption requirements before implementing.
- [ ] Implement segment-tail recovery and SQLite/file reconciliation.
- [ ] Measure the socket capture path before considering PACKET_MMAP/RX rings; validate IPv6 extensions and VLAN handling.
- [ ] Define application/player identity and request-level flag linkage for non-HTTP traffic.
- [ ] Low priority: restrict local storage to a dedicated analyzer UID/GID, data directory 0700 and SQLite/WAL/SHM/segment files 0600; review .env permissions and minimum capture capabilities. Add relevant permission/runtime checks. This is separate from HTTP authorization.
- [ ] Optional: investigate capture-time ordering in session browsing under high RPS, accounting for worker finalization order, stable pagination and live updates without global ingest serialization.
- [ ] Future refinement: evaluate cookie/token/user/object correlation beyond existing session chains. Account for NAT, parallel traffic, connection reuse and encrypted HTTPS; this is not required by the existing chain contract.

## Target-host validation

- [ ] Validate final competition-network local/split capture and chain browsing on the actual Linux/vulnbox topology, including genuine team-client ingress addresses. Commands and coverage are in test/collector_split, test/auth and test/session_chains.
