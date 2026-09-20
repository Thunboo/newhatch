# Backlog

Only unfinished tasks that are not currently being implemented belong here.

## Architecture Decisions To Finalize

- [ ] Choose WebSocket parser crate.
- [ ] Define how flag-containing S2C replies link to triggering C2S/client payload.
- [ ] Clarify the tentative auto-removal threshold (`rows >= 5000 OR query time >= 100 ms`): determine whether it governs retention cleanup, query/search budgeting, write batching or another subsystem.
- [ ] Evaluate Suricata correlation strategy.

## Collector Transport Hardening

- [ ] Add per-collector pre-shared-key authentication to the collector/analyzer handshake. The initial split trusts `ANALYZER_CONNSTR` on the collector and optional IP/FQDN filtering through `ALLOWED_COLLECTORS` on the analyzer; it does not provide application-layer authentication or encryption.

## MVP Implementation Milestones

- [ ] WebSocket frame extraction after the implemented HTTP Upgrade classification.
- [ ] Explore HTTP action-chain reconstruction for Attack/Defence analysis. First parse every request/response transaction inside persistent TCP connections; then evaluate heuristic correlation across connections using collector/source, client IP, cookies or tokens, usernames, object IDs and a bounded time window. A practical first UI may show related actions preceding a flag-bearing response rather than claim a definitive attack graph. Account for NAT, parallel attacks, connection reuse and the inability to inspect encrypted HTTPS without decryption.
- [ ] Suricata alert ingestion/correlation as optional enrichment.
- [ ] Add a Russian/English language switch for the frontend. Move visible UI strings into a small localization layer, persist the selected language in the browser, and default to the browser language when no preference has been saved.
- [ ] Optional: investigate chronological ordering in the web traffic view under high RPS. Parallel flow workers may finalize and persist sessions out of capture-time order; define the intended ordering by packet/session capture timestamps and make API pagination plus UI updates stable without adding expensive global serialization to the ingest hot path.
