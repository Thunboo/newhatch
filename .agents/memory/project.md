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

- Required env: `USERNAME`, plaintext `PASSWORD`, and optional `SESSION_EXPIRACY` (default `86400s`).
- Password is Argon2id-hashed in memory during analyzer startup.
- nginx admits loopback plus `AUTH_ALLOWED_SUBNETS`; empty means loopback only.
- Analyzer API binds loopback and requires server-side sessions for protected routes.
- Cookies are HttpOnly/SameSite Strict; `AUTH_COOKIE_SECURE` controls HTTPS-only cookies.
- Frontend has no analyzer data volume; nginx returns 404 for dotfiles and storage/database/segment artifact paths.

## Implemented UI Behavior

- Sessions, Sources and Collectors views are operational.
- Session feed merges by ID in `id DESC` order and counts unique loaded rows.
- Newest-page polling runs every five seconds only near the live edge; it pauses while older traffic is inspected.
- Scrolling upward triggers one anchored catch-up refresh. Older pages load automatically through an observer sentinel.
- Opening session detail highlights its table row with a black inner outline.
- Detail closes with its close button or `Escape`.
- Detail shows an `Esc to close` hint next to the close button.
- Text payload view decodes JSON/Unicode escapes and percent-encoded URL components; session-header HTTP paths use the same visual decoding.
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

## Verification

- Frontend production image builds successfully.
- Auth verification passes across Rust workspace/integration tests, nginx/Compose unit tests and the isolated IPv4/IPv6 Docker network suite, including sensitive artifact denial and unauthorized mutation side effects.
- Current Playwright suite has four passing scenarios, including auth, English/Russian browser defaults and persistence, live feed/pagination, collector status, selected-row state, independent scrolling, fixed sidebar, mobile layout and Escape close.
- Split deployment has worked over the user's VPN after rebuilding the collector with current FQDN-capable code.

## Remaining High-Level Work

- Collector PSK authentication/encryption.
- WebSocket frame decoding.
- Suricata EVE ingestion/correlation and filter synchronization.
- Segment tail recovery/reconciliation.
- `PACKET_MMAP` benchmarking and possible adoption.
- VLAN and IPv6 extension-header handling.
- Exact linkage from flag-bearing S2C replies to triggering C2S data.
