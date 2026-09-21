# Backlog

Only unfinished tasks that are not currently being implemented belong here.

## Architecture Decisions To Finalize

- [ ] Choose WebSocket parser crate.
- [ ] Define how flag-containing S2C replies link to triggering C2S/client payload.
- [ ] Clarify the tentative auto-removal threshold (`rows >= 5000 OR query time >= 100 ms`): determine whether it governs retention cleanup, query/search budgeting, write batching or another subsystem.
- [ ] Evaluate Suricata correlation strategy.

## Collector Transport Hardening

- [ ] Add per-collector pre-shared-key authentication to the collector/analyzer handshake. The initial split trusts `ANALYZER_CONNSTR` on the collector and optional IP/FQDN filtering through `ALLOWED_COLLECTORS` on the analyzer; it does not provide application-layer authentication or encryption.

## Low-Priority Security Hardening

- [ ] Harden local storage against unprivileged host users: run analyzer as a dedicated non-root UID/GID, restrict the data directory to `0700` and SQLite/WAL/SHM/segment files to `0600`, review `.env` permissions, retain only capture capabilities required by `AF_PACKET`, and add Unix-permission/container-runtime regression tests. This is separate from the HTTP authorization boundary covered by GitHub issue #7.

## MVP Implementation Milestones

- [ ] WebSocket frame extraction after the implemented HTTP Upgrade classification.
- [ ] Explore HTTP action-chain reconstruction for Attack/Defence analysis. First parse every request/response transaction inside persistent TCP connections; then evaluate heuristic correlation across connections using collector/source, client IP, cookies or tokens, usernames, object IDs and a bounded time window. A practical first UI may show related actions preceding a flag-bearing response rather than claim a definitive attack graph. Account for NAT, parallel attacks, connection reuse and the inability to inspect encrypted HTTPS without decryption.
- [ ] Suricata alert ingestion/correlation as optional enrichment.
- [ ] Optional: investigate chronological ordering in the web traffic view under high RPS. Parallel flow workers may finalize and persist sessions out of capture-time order; define the intended ordering by packet/session capture timestamps and make API pagination plus UI updates stable without adding expensive global serialization to the ingest hot path.
