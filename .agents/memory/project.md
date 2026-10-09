# Project Memory

## Product

- Internal name: `newhatch`; UI name: `Нюхач`.
- Lightweight live-traffic analyzer for Attack/Defence CTF competitions.
- Main entity: reconstructed bidirectional TCP session, not an individual packet.
- Goal: Packmate-like workflow with bounded memory and low vulnbox overhead.

## Canonical Files

- `PROJECT.md`: product and implementation overview.
- `README.md`: operator quick start and deployment guide.
- `docs/agent-decisions.md`: fixed architecture decisions.
- `docs/architecture.md`: capture and processing pipeline.
- `docs/storage.md`: SQLite plus append-only segments.
- `docs/authentication.md`: ingress and login model.
- `docs/session-chains.md`: Issue #13 agreed contract, API and query limits.
- `docs/python-replay.md`: implemented Issue #20 Python replay contract and verification.
- `.agents/tasks/backlog.md`: deferred work.

## Current Architecture

- Rust/Tokio backend with Linux cooked `AF_PACKET/SOCK_DGRAM` capture.
- Enabled source ports are compiled into a kernel BPF filter.
- Bounded worker-local TCP reassembly produces C2S/S2C streams.
- Flag detection runs on reconstructed streams.
- SQLite stores metadata and segment pointers; payload bytes live in append-only segment files.
- React/TypeScript/Vite frontend is served by nginx.
- Suricata remains optional passive enrichment; EVE correlation is not implemented.

## Deployment Modes

- `ANALYZER=local` (default): analyzer captures and processes traffic in one container.
- `ANALYZER=remote`: analyzer receives classified packets from remote collectors.
- Collector target: `ANALYZER_CONNSTR=HOST:PORT`.
- Analyzer receiver: `LISTEN_CONNSTR=HOST:PORT`.
- Both connection strings support IPs and FQDNs. Literal IPv6 endpoints require brackets.
- `ALLOWED_COLLECTORS` accepts comma-separated IPs/FQDNs; empty accepts any peer. FQDN allowlist entries resolve at analyzer startup.
- Collector `QUEUE_CAPACITY` defaults to 8192 packets.
- Transport is currently unencrypted and unauthenticated; PSK is backlog work.

## Authentication

- Required env: `NEWHATCH_USERNAME`, plaintext `NEWHATCH_PASSWORD`, and optional `SESSION_EXPIRACY` (default `86400s`).
- Password is Argon2id-hashed in memory during analyzer startup.
- nginx admits loopback plus `AUTH_ALLOWED_SUBNETS`; empty means loopback only.
- Analyzer API binds loopback and requires server-side sessions for protected routes.
- Cookies are HttpOnly/SameSite Strict; `AUTH_COOKIE_SECURE` controls HTTPS-only cookies.
- Frontend has no analyzer data volume; nginx returns 404 for dotfiles and storage/database/segment artifact paths.

## Implemented UI Behavior

- Sessions, Sources and Collectors views are operational.
- Optional session chains use key `collector_id + client_ip + source_id` and adjacent start gaps <= 1 second. The off-by-default checkbox groups complete session pairs; filtered cards retain full context and order C2S before its S2C by session start/ID.
- Chain cards scroll between fixed-size, independently scrollable payload windows. C2S alone exposes per-window cURL/Python export. Payloads load near the viewport and release offscreen; late bridging sessions replace obsolete chain rows on refresh.
- A centered chain-toolbar Python action exports every snapshot member's supported HTTP requests, including multi-request C2S, through paginated one-payload-at-a-time reads. One requests.Session replays in session/stream order with initial first-request cookies followed by actual Set-Cookie updates. Unsupported input/retention changes prevent partial copy; closing aborts preparation. Format JSON follows per-message display rules; dynamic tokens require manual edits. See `docs/session-chains.md`.
- Cookie-seeding helper/imports are conditional on reuse of an initial cookie name later for the same host. Without reuse, the first Cookie remains a normal header and no helper is emitted; Session still processes live response cookies.
- A transparent strip (`right: 0px; width: 26%; min-width: 100px`) reaches the right edge of chain windows and routes native scrolling to the card. Copy actions are above it via z-index; uncovered text retains independent payload scrolling. Ordinary session cards have no overlay.
- A small localized "Scroll sessions here" hint appears below Text/Hex only in chain detail, explaining the strip.
- Session feed merges by ID in `id DESC` order and counts unique loaded rows.
- Newest-page polling runs every five seconds only near the live edge; it pauses while older traffic is inspected.
- Scrolling upward triggers one anchored catch-up refresh. Older pages load automatically through an observer sentinel.
- Opening session detail highlights its table row with a black inner outline.
- Detail closes with its close button or `Escape`.
- Detail shows an `Esc to close` hint next to the close button.
- Text payload view decodes JSON/Unicode escapes and percent-encoded URL components; session-header HTTP paths use the same visual decoding.
- C2S has cURL/Python logo buttons with text fallbacks before ordinary copy for frontend-only Bash/cURL and Python/requests export. Use the displayed text body and Format JSON setting; retain readable Unicode. Validate one complete plain-text HTTP request against raw framing, recalculate Content-Length and omit connection headers. Hex/unsupported input disables export with a localized reason. Curl requires 7.87+; captured destinations and Host are preserved by default.
- Issue #20 Python exports accept one optional positional IP/FQDN to replace all connection hosts and existing Host values using each captured server port. Without argv, preserve each session's destination and captured Host; absent Host stays out of explicit headers. Print each actual response with `print(response.text, flush=True)`. Cookie scope follows effective Host or destination, unifying different captured hosts under argv; conditional helper setup accounts for this. Contract: `docs/python-replay.md`.
- Each C2S/S2C panel has one-click copy. Optional frontend-only JSON formatting keeps JSON compact when disabled and pretty-prints bodies plus bounded nested JSON strings when enabled; Hex remains raw.
- Wheel input over detail cannot scroll the underlying list; wheel input over the exposed list still can.
- Desktop sidebar is fixed. In remote mode it shows analyzer and aggregate collector status lights; local mode omits the collector light.
- UI strings are provided by English/Russian dictionaries. Browser language supplies the unsaved default; the sidebar switch above Sign out persists `newhatch_language`.
- `frontend/src/assets/logo.png` is bundled by Vite.

## Storage And Retention

- Empty `raw_tcp` sessions with zero reconstructed bytes are rejected and legacy rows are removed.
- Retention is segment-count based, not a continuous wall-clock cleanup job.
- Rotation is checked only when a completed session is appended. Starting the analyzer or leaving it powered off does not by itself delete old sessions.
- Defaults: `SEGMENT_DURATION=30m`, `SEGMENT_RETENTION_COUNT=3`.
- Analyzer persists collector identity and earliest directional non-empty payload capture times. Nullable startup migration preserves old segments; unknown collectors stay singleton chains. On-demand SQL grouping has two concurrent readers, a 2-second/50-million-step SQL budget, capped pages and resumable 2,000-candidate payload search. Cursors pin an insertion watermark; retention can still remove members.

## Verification

- Issue #20's clarified Host behavior passed TypeScript lint, Vite production build and all 36 unique local browser/replay checks on 2026-10-10: 35 in the full run plus the chain clipboard test rerun after updating its seeding expectation. Real IPv4/IPv6 and locally resolved FQDN requests verify adaptive existing Host with preserved ports, invalid argv, fresh/deleted cookies and flushed flags. Different captured hosts converge under argv across ports; mixed/absent Host also works. Temporary NEWHATCH_TEST_RESULTS was set; no Docker was run.
- Issue #14's namespaced credential regression passes all 7 Python auth/Compose checks and the user-run Docker auth E2E suite on 2026-09-29. The E2E build compiled the analyzer and validated real IPv4/IPv6 login behavior with the new variables.
- Issue #13 passed 45 Rust tests (including budget interruption), strict workspace clippy, formatting, TypeScript checking, production Vite build and 7 local Chromium browser scenarios on 2026-10-08. Coverage includes legacy migration, exact window/key rules, snapshot/member/search pagination, late merging, C2S exports and desktop/mobile nested scroll. Live Linux capture remains target-host follow-up; no Docker was run by the agent.
- Issue #13's current UI follow-ups passed TypeScript/build and four chain browser scenarios on 2026-10-08: responsive strip sizing, right-edge outer-scroll routing, copy controls above the overlay, chain-only hint, mobile layout and blocked background scrolling at boundaries.
- Whole-chain Python export passed TypeScript/build and 18 unique local Chromium/replay scenarios on 2026-10-08 (full 17-test run plus a targeted final rerun adding IPv6 Host). Generated scripts executed against a temporary HTTP receiver: all requests, Unicode framing, initial/fresh/deleted cookies, explicit redirects and continuation after HTTP 500. UI coverage includes pagination, offscreen failure, cancellation, retry, retention changes and Russian mobile clipboard fallback.
- Conditional cookie-helper generation passed TypeScript/build and nine focused browser/Python replay tests on 2026-10-08: cookie-free/single-request/first-only/distinct-cookie/different-host scripts omit unused code, initial headers are preserved, and both response cookies and reused-cookie seeding keep working.
- Issue #12's Docker regression image passed all 20 Playwright tests on 2026-09-29, including execution of generated Bash/cURL and Python/requests snippets against nginx, unsupported-input handling, Unicode/JSON bodies, clipboard fallback and existing auth/session-feed regressions.

- Frontend production image builds successfully.
- Auth verification passes across Rust workspace/integration tests, nginx/Compose unit tests and the isolated IPv4/IPv6 Docker network suite, including sensitive artifact denial and unauthorized mutation side effects.
- The previous Playwright suite had four passing scenarios, including auth, English/Russian browser defaults and persistence, live feed/pagination, collector status, selected-row state, independent scrolling, fixed sidebar, mobile layout and Escape close.
- Split deployment has worked over the user's VPN after rebuilding the collector with current FQDN-capable code.

## Remaining High-Level Work

- Collector PSK authentication/encryption.
- WebSocket frame decoding.
- Suricata EVE ingestion/correlation and filter synchronization.
- Segment tail recovery/reconciliation.
- `PACKET_MMAP` benchmarking and possible adoption.
- VLAN and IPv6 extension-header handling.
- Exact linkage from flag-bearing S2C replies to triggering C2S data.
