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

- Agent restriction: never run Docker/Docker Compose or control OrbStack; give Docker commands to the user for manual execution because daemon operations can interrupt the VPN tunnel.
- Local: `ANALYZER=local`.
- Split analyzer: `ANALYZER=remote`, `LISTEN_CONNSTR=HOST:PORT`, optional `ALLOWED_COLLECTORS`.
- Collector: `ANALYZER_CONNSTR=HOST:PORT`, `QUEUE_CAPACITY=8192` default.
- IP/FQDN endpoints are supported; bracket literal IPv6. Empty collector allowlist accepts any host. No PSK yet.
- Auth: `NEWHATCH_USERNAME`, `NEWHATCH_PASSWORD`, `SESSION_EXPIRACY`, `AUTH_ALLOWED_SUBNETS`, `AUTH_COOKIE_SECURE`.
- Frontend has no `/data` mount; nginx explicitly rejects dotfile, database and segment artifact paths.

## Current UI Invariants

- Sessions merge uniquely by ID; loaded count is frontend row count.
- Optional whole-session chains use `collector_id + client_ip + source_id`, adjacent start gaps <= 1 second, and an off-by-default checkbox. Pairs stay together in start/ID order, C2S before S2C; filters select a matching member but cards show full context.
- Chain cards scroll between fixed-size payload windows with independent inner scrolling, C2S-only exports and viewport-local payload loading. Refresh replaces stale split rows after late merging. See `docs/session-chains.md`.
- The right-side transparent 90 px strip scrolls the chain with native wheel/touch routing. Copy actions are raised above it, and a 20 px edge area leaves native payload scrollbars accessible. Individual session cards have no strip.
- Poll newest every five seconds only near the top.
- Pause newest polling in history; upward motion performs one anchored catch-up.
- Infinite cursor pagination loads older sessions automatically.
- Open detail highlights its row and closes with `Escape`.
- Detail exposes C2S/S2C copy, decoded text/path display and optional body JSON formatting; Hex remains raw.
- C2S cURL/Python logo buttons (with text fallbacks) copy Bash/cURL or Python/requests using the displayed body (Unicode and Format JSON preserved). Frontend only; one complete plain-text HTTP request. Unsupported input/Hex disables export with a reason. See `docs/mvp.md`; curl requires 7.87+.
- Wheel over detail never scrolls the underlying list; wheel over exposed list does.
- Desktop sidebar is fixed.
- English/Russian UI language follows the browser until the sidebar switch above Sign out persists `newhatch_language`.
- Remote mode shows analyzer and collector status lights; local mode has no collector light.

## Important Storage Behavior

Retention is by rotated segment count. Rotation and expiration are checked only when appending a completed session, so downtime does not independently age-delete sessions.

Analyzer stores collector and directional first-payload capture times; legacy
NULL collector rows stay singletons. Chains are read on demand, with at most two
readers and interruptible SQL (2 seconds/50 million steps), 200-row pages and
resumable 2,000-candidate payload scans. Cursors pin insertions until refresh;
retention can still remove members. No collector aggregation or payload duplication.

## Current Verification

- Issue #14's namespaced credential regression passes all 7 Python auth/Compose checks and the user-run Docker auth E2E suite on 2026-09-29. The E2E build compiled the analyzer and validated real IPv4/IPv6 login behavior with the new variables.
- Issue #13: 45 Rust tests, strict clippy/fmt, TypeScript/Vite build and 7 local Chromium browser scenarios passed on 2026-10-08. Covers migration, correlation, pagination/search budgets, late merging and desktop/mobile nested scroll. Live Linux capture is target-host follow-up; agent ran no Docker.
- The 90 px strip follow-up passed TypeScript/build and four chain browser scenarios, including copy controls above the layer and outer-scroll isolation on desktop/mobile.
- Issue #12's Docker regression image passed all 20 Playwright tests on 2026-09-29, including real execution of generated Bash/cURL and Python/requests snippets through nginx and the existing auth/session-feed regressions.

- Frontend production Docker build passes.
- Rust auth integration, nginx/Compose unit checks and isolated IPv4/IPv6 Docker auth e2e pass, including sensitive artifact denial and rejected mutation side effects.
- Previous Playwright run: four scenarios passed, including English/Russian browser defaults and persistence plus current session-feed and detail interactions.
- Real split deployment over VPN/FQDN works after rebuilding the collector from current sources.

## Next Work

Use `.agents/tasks/backlog.md`. Major deferred areas: collector PSK, WebSocket frames, Suricata correlation, segment crash recovery, `PACKET_MMAP`, VLAN/IPv6 extensions and S2C-to-C2S flag linkage.
