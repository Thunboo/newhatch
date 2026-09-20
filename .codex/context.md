# Codex Quick Context

## Product

`newhatch` / `Нюхач` is a lightweight live A/D CTF traffic analyzer. Read `PROJECT.md`, `README.md`, `docs/agent-decisions.md`, then the relevant focused doc before architectural work.

## Pipeline

```text
enabled source ports -> kernel BPF -> cooked AF_PACKET capture
-> bounded classified packets -> worker-local TCP reassembly
-> flag/protocol analysis -> SQLite metadata + segment payload files
-> authenticated API -> React UI
```

Persist sessions, not packets. Keep payload bytes out of SQLite. Suricata is optional parallel enrichment and is not integrated yet.

## Runtime

- Local: `ANALYZER=local`.
- Split analyzer: `ANALYZER=remote`, `LISTEN_CONNSTR=HOST:PORT`, optional `ALLOWED_COLLECTORS`.
- Collector: `ANALYZER_CONNSTR=HOST:PORT`, `QUEUE_CAPACITY=8192` default.
- IP/FQDN endpoints are supported; bracket literal IPv6. Empty collector allowlist accepts any host. No PSK yet.
- Auth: `USERNAME`, `PASSWORD`, `SESSION_EXPIRACY`, `AUTH_ALLOWED_SUBNETS`, `AUTH_COOKIE_SECURE`.

## Current UI Invariants

- Sessions merge uniquely by ID; loaded count is frontend row count.
- Poll newest every five seconds only near the top.
- Pause newest polling in history; upward motion performs one anchored catch-up.
- Infinite cursor pagination loads older sessions automatically.
- Open detail highlights its row and closes with `Escape`.
- Detail exposes C2S/S2C copy, decoded text/path display and optional body JSON formatting; Hex remains raw.
- Wheel over detail never scrolls the underlying list; wheel over exposed list does.
- Desktop sidebar is fixed.
- Remote mode shows analyzer and collector status lights; local mode has no collector light.

## Important Storage Behavior

Retention is by rotated segment count. Rotation and expiration are checked only when appending a completed session, so downtime does not independently age-delete sessions.

## Current Verification

- Frontend production Docker build passes.
- Playwright: three scenarios pass, including current session-feed and detail interactions.
- Real split deployment over VPN/FQDN works after rebuilding the collector from current sources.

## Next Work

Use `.agents/tasks/backlog.md`. Major deferred areas: collector PSK, WebSocket frames, Suricata correlation, segment crash recovery, `PACKET_MMAP`, VLAN/IPv6 extensions and S2C-to-C2S flag linkage.
