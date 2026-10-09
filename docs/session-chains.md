# Session Chains (Issue #13)

- Issue: https://github.com/Thunboo/newhatch/issues/13
- Specification and implementation: 2026-10-08.
- Implemented on `codex/feature-issue-13-session-chains`.
- UX summary: [MVP](mvp.md#session-chains-issue-13). Storage details: [storage](storage.md).

## Goal

The Sessions view offers optional chain-based browsing of related TCP sessions. Players can inspect interactions by one client with one service, including sessions preceding a flag-bearing response, while keeping capture and collector overhead low.

These are heuristic session chains, not proof that all members belong to one exploit or player. Correlation does not depend on HTTP status codes.

## 1. Correlation Unit

One chain step is one reconstructed bidirectional TCP session with its existing complete C2S/S2C pair.

Multiple requests and responses inside a persistent TCP connection remain one session and one pair. Do not split that session into individual HTTP transactions, packet windows or directional turns for this issue.

## 2. Correlation Key

Sessions can join the same chain only when all three fields match:

```text
collector_id + client_ip + source_id
```

`source_id` is the existing service identifier. Different client ports may belong to one chain. Different collectors, clients or sources must never merge. Do not add server IP, client port, cookies or other attributes to the agreed key.

## 3. Time Rule And Ordering

Within each key, order sessions by `started_at` ascending, with session ID as a deterministic tie-breaker.

- A gap between adjacent session starts of at most 1 second continues the chain.
- A gap greater than 1 second starts a new chain.
- This is a sliding adjacency rule, not fixed wall-clock buckets or a maximum total chain duration.
- Use capture timestamps, not analyzer arrival, session completion or SQLite insertion times.
- The comparison is start-to-start, not end-to-start or between directional payload timestamps.

For example, session starts at 0.0, 0.8 and 1.6 seconds form one chain; a session at 2.7 seconds begins another. Exactly 1 second remains within the same chain.

## 4. Analyzer-Owned Metadata

Persist the following metadata for new sessions:

```text
collector_id
first_payload_c2s_at
first_payload_s2c_at
```

For each direction, the first-payload timestamp is the earliest capture timestamp of an observed packet carrying non-empty payload in that direction. Packets without payload do not supply this timestamp. A direction with no payload has no first-payload timestamp.

The analyzer gathers this metadata while processing incoming packets and assembling sessions, regardless of the display checkbox state. Preserve the originating collector identity through session finalization and storage. Local capture uses its designated local collector identity.

The collector continues its existing lightweight capture, classification and forwarding work. Do not move correlation or additional metadata aggregation to the collector. Use the capture timestamps already supplied with packets.

No detailed per-packet chronology or per-fragment timeline is required for the agreed whole-session-pair UI.

## 5. Backend And Persistence

- Compute chains on demand in the backend from stored session metadata.
- Keep each session and its payload as an independent stored record. Do not duplicate payload bytes for chains.
- SQLite remains metadata/index storage; reconstructed bytes remain in append-only segments.
- Sessions become available after finalization and persistence, as in the existing UI. This issue does not introduce active-session payload browsing.
- A late-persisted session may extend or merge previously displayed chains. Subsequent reads must use capture-time order and include newly available members.
- Bound query work, response sizes and memory. Do not add an unbounded live chain buffer or global serialization to the packet hot path.

## 6. List Display Toggle

A checkbox labelled "Group into chains" / "Группировать в цепочки" is off by default.

- Off: retain the existing session list with one row per TCP session.
- On: show one row per chain. A singleton is a chain of one session.
- A chain row shows service, client IP, chain start, session count, total traffic bytes and flag presence.
- Flag-bearing chains remain visually prominent.
- Order chain rows newest first by chain start, with a deterministic tie-breaker. Order members inside an opened chain oldest first by session start.

The checkbox changes browsing representation; it does not enable or disable capture or the metadata collection needed for this representation.

## 7. Chain Detail Card

Use the existing detail panel and payload-window components as a vertical sequence of complete session pairs.

For each session, display C2S immediately followed by that session's S2C. Keep pairs together and order them by session start. Do not interleave windows from different sessions according to individual payload timestamps, even if their connections overlap.

Each window identifies its session, direction and first-payload capture time when available. Its header puts the session/direction above the timestamp to keep both readable on narrow screens. Existing Text/Hex, JSON formatting, flag highlighting and ordinary payload-copy behavior are reused.

The per-window Python/requests and Bash/cURL export buttons, shown as `[copy .py]` and `[copy curl]` in the agreed mockup, appear only in C2S headers. S2C headers must not expose those export buttons. These individual actions retain their existing single-request validation and disabled-state explanations.

### Whole-Chain Python Export (2026-10-08)

The center of the chain card's `payload-toolbar` has a Python-logo button with a
localized **Copy chain as Python (requests)** tooltip. It is exclusive to chain
detail and available in Text mode. Preparation shows a spinner and session-count
progress in the tooltip; success/failure uses the existing copy feedback.

On click, fetch every member page using the selected `snapshot_id`, including
sessions not yet loaded or visible in the card. Read C2S payloads one at a time
and retain only generated script text; export does not expand the card's lazy
windows. Closing the card aborts preparation. Membership/count changes caused by
retention require reopening the chain; unsupported requests or failed payload
reads report the session and, when known, request index. Never copy a partial
script or silently skip a request.

Split each C2S into complete HTTP/1.x requests using raw-byte header boundaries
and Content-Length before applying the existing text/JSON display transforms
per message. Multiple complete requests in one TCP session are supported here;
the session still remains one C2S/S2C pair in the card. Supported input retains
the origin-form, uncompressed UTF-8, no-body/Content-Length and header restrictions
of individual exports; binary, chunked, Upgrade and incomplete messages fail the
whole export.

The generated Python 3 script uses one `requests.Session()`. It sends sessions
in `(started_at, id)` order and requests in each C2S byte order, waiting for each
response without replaying captured timing gaps. Each editable block identifies
its session and request. By default, captured server IP/port and Host are preserved; displayed
UTF-8 bodies follow Format JSON and Content-Length is recalculated. Timeout is
10 seconds per request and automatic redirects are disabled. HTTP error statuses
are printed and do not skip later captured requests; transport failures stop the
script.

Issue #20's [Python replay contract](python-replay.md) is implemented. With no
argument, each member keeps its own captured server
IP/port and Host; one optional IP/FQDN argument replaces every connection host
and any existing Host value, using each captured server port. Missing Host is not synthesized in
explicit headers. Each actual response body is printed with flush enabled;
the ordering, Session cookies and complete-export rules below remain required.

Include `seed_cookies`, its SimpleCookie/urlsplit imports and its explanatory
comment only when a cookie name from the first request appears in a later
request for the same Host (or destination host). Cookie names are case-sensitive;
host comparison ignores case and port, and cookie values may change. The check
includes later requests within the first TCP session as well as later members.
If no initial cookie is reused, send the first Cookie header directly and omit
the helper; this also keeps single-request and cookie-free scripts compact.
Use the effective Host for cookie scope, or the effective destination when Host
is absent. When the argument determines whether hosts match, guard cookie setup
at runtime; otherwise retain the compact unconditional or omitted helper.

When reuse is detected, cookies from the first request initialize the jar for
the effective host at path `/`. Later captured Cookie headers are omitted so
Set-Cookie updates, expiry and deletion from actual responses govern following
requests, including scripts without the helper. Initial cookie paths cannot be
inferred from captured request headers.
Response-dependent CSRF tokens, IDs and other values require manual editing;
their extraction/substitution is outside this export.

```text
Chain / Цепочка · web · 10.0.4.17 · 2 sessions · FLAG

12:03:21.100 · Session #41 · C2S [copy .py] [copy curl]
┌─────────────────────────────────────────────────┐
│ GET /login …                                    │
└─────────────────────────────────────────────────┘

12:03:21.140 · Session #41 · S2C
┌─────────────────────────────────────────────────┐
│ HTTP/1.1 200 OK …                                │
└─────────────────────────────────────────────────┘

12:03:21.700 · Session #42 · C2S [copy .py] [copy curl]
┌─────────────────────────────────────────────────┐
│ GET /secret …                                   │
└─────────────────────────────────────────────────┘

12:03:21.760 · Session #42 · S2C · FLAG
┌─────────────────────────────────────────────────┐
│ HTTP/1.1 200 OK … FLAG{…}                        │
└─────────────────────────────────────────────────┘
```

### Two Independent Scroll Levels

- The chain card has a vertically scrollable body for navigating the sequence of C2S/S2C windows.
- Every expanded C2S and S2C window retains the current standalone payload-window dimensions for the same viewport. Its height stays fixed while browsing; it must not grow with payload length or shrink as more chain members are added.
- Each window's payload body keeps its own scrollable area, so a long request or response can be inspected without resizing the window or moving through the chain.
- Window dimensions may adapt to viewport changes as the existing responsive layout does; "fixed" does not require one universal pixel size for every screen.
- Both levels must remain usable with wheel/trackpad/touch input. Scrolling over uncovered payload content moves that payload; scrolling the card outside the payload body navigates between windows.
- A transparent strip with `right: 0px`, `width: 26%` and `min-width: 100px` overlays the right side of each C2S/S2C window, routing native scrolling to the chain body instead of the payload. It reaches the window's right edge, including the payload scrollbar area; payload scrolling remains available over the uncovered text. The gaps between windows already scroll the same body. Window dimensions and payload rendering stay unchanged.
- Copy/export actions are positioned above the overlay with a higher z-index. They remain clickable; scrolling over their headers also moves the chain. The overlay is only present in grouped chain cards.
- A small "Scroll sessions here" hint below Text/Hex identifies the chain-scroll area. It appears only in chain detail; the Russian locale shows "Прокручивайте сессии здесь".
- Detail-panel interactions must not scroll the background session list, including when an inner scroll area reaches its boundary.
- Any optional collapse control may collapse a long window; an expanded window returns to the fixed size above rather than expanding to fit all content.
- Load members and payloads progressively for large chains. Additional loaded members must not compress existing windows to fit the panel.

## 8. Filters And Pagination

Chain membership is determined by the key and time rule, independently of the displayed page and content filters.

A chain appears in filtered results when at least one of its sessions satisfies all active filters. Opening that chain exposes its other retained members to preserve context. For example, a flag-only filter must not hide preceding non-flag sessions from the opened chain.

Paginate chain rows and member retrieval with bounded work and response sizes. A page boundary does not terminate a chain. An unloaded portion must remain accessible through pagination/progressive loading.

## 9. Existing Data And Retention

- Migrations preserve existing sessions and segment payloads.
- Do not infer or fabricate a collector identity for legacy records that lack it. Such records remain browsable; in grouped mode they stay separate as singleton chains.
- Display unknown legacy first-payload times as unknown. Do not substitute session start as though it were a measured first-payload time.
- Build chains from retained, available sessions. Keep the existing segment-retention policy; this issue does not add packet persistence, payload duplication or special chain retention.

## 10. Scope And Documentation

Implementation includes analyzer metadata, SQLite migration, backend/API grouping, frontend list/card behavior and corresponding documentation/memory synchronization.

Out of scope:

- transaction-level browsing or request/response pairing inside one TCP session (frontend export may split C2S requests without changing the session model);
- packet-by-packet chronology or interleaved fragment rendering;
- Suricata EVE ingestion/correlation;
- attack-success classification using HTTP status codes;
- cross-service or cross-collector chains;
- additional collector processing or new infrastructure dependencies.

The agreed specification is preserved in this project document. Agent task state and compact summaries are maintained separately.

## 11. Acceptance And Verification

- Exactly 1 second joins; a greater gap splits; chained adjacency can produce total duration over 1 second.
- Every component of the three-field key isolates chains correctly, including local and remote capture.
- Different client ports can join, while a multi-request TCP session stays one C2S/S2C pair.
- Capture-time ordering stays deterministic under equal timestamps and late persistence.
- Grouped filters retain the full available context; list/member pagination does not cut chain membership at page boundaries.
- Legacy records, missing directional payloads and retention remain inspectable without invented metadata.
- Turning grouping off retains existing individual-session browsing.
- Each pair stays together in the card; per-window export buttons appear only on C2S and retain existing supported-input rules.
- The toolbar Python action exports every member and every supported C2S request in order, including offscreen/later-page sessions. Validate cookie updates/deletion, framing before display transforms, abort on close and no partial clipboard writes on failure.
- With many pairs, the card scrolls between them. With long payloads, each fixed-size window scrolls internally. Existing window sizes do not shrink when more pairs load or grow with payload bytes.
- Both scroll levels work without moving the background session list, including at boundaries and after viewport changes. Verify both C2S and S2C, Text/Hex and a narrow/mobile viewport.
- The responsive right-side strip scrolls the outer chain while inner offsets stay unchanged; copy actions above it remain accessible on desktop and mobile. Its width is 26% of the window with a 100 px minimum and no right inset. At the chain boundary it must not scroll the background.
- Run relevant available Rust/frontend checks when implementing. Agents must never invoke Docker, Docker Compose, the Docker daemon or OrbStack; provide exact Docker-backed verification commands for the user to run when needed.

## 12. Implemented API And Work Limits

Both routes share the existing authenticated data boundary:

- `GET /api/chains`: the session metadata/payload filters plus `limit` (default 100, maximum 200) and opaque `chain_cursor`. Returns `{ items, next_cursor }`.
- `GET /api/chains/{id}/sessions`: `snapshot_id`, opaque `cursor`, and `limit` (default 20, maximum 200). Returns `{ chain, items, next_cursor }`; an unavailable anchor returns 404.

A chain summary includes collector, source and client identity, start/end, member count, total directional bytes, flags, alerts, incomplete status and `snapshot_id`. Its ID is the smallest retained session ID in that chain; members sort by `(started_at, id)`. Any member ID can resolve its chain through the member route.

Pagination pins the maximum persisted session ID at the first request. Later pages exclude new insertions until refresh, so a late bridge cannot duplicate or skip rows within that snapshot. Refresh takes a new snapshot and replaces the previously loaded chain span, removing obsolete split rows. This is a pagination watermark, not an archival snapshot: retention can still remove records. The open card uses its selected snapshot and does not change membership while being inspected.

Grouping uses SQLite window queries over retained metadata. Only complete key filters (source/client IP) may prune input before calculating gaps; other filters select chains after membership is formed. Queries run in blocking workers, with at most two concurrent chain reads. Each reader has a 250 ms SQLite lock timeout, a 2 MiB page cache, file-backed temporary sorting, and a progress handler interrupting SQL at 2 seconds or 50 million VM steps. Busy admission or an interrupted query returns 503 with an actionable error. Large catalogs may require narrower source/client filters; constant-time whole-catalog grouping is not guaranteed.

Payload search scans at most 2,000 matching candidate sessions per request in chunks of 200. It saves progress within a chain, so an empty page with a continuation cursor is valid and does not discard later matches. The same candidate must satisfy all metadata and payload filters. The deadline is checked between candidates; a single segment read/byte scan can run beyond that checkpoint. Payloads remain in their original segments.

The frontend loads member metadata in pages of 20, fetches payloads only near the visible card area, aborts obsolete requests and releases payload bytes when pairs leave that area. Window placeholders keep the same dimensions; inner scroll positions survive payload unloading. At the live edge, grouped metadata refreshes every five seconds while the card is closed and payload search is inactive. A manual refresh is always available. Mobile cards end above the bottom navigation.

See [chain verification](../test/session_chains/README.md) for runnable checks and coverage.
