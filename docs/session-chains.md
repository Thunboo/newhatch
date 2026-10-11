# Session Chains

Current contract for [#13](https://github.com/Thunboo/newhatch/issues/13).
Chains help inspect one client's traffic to one service, including context before
a flag-bearing response. Correlation is heuristic, not proof of one exploit/player;
HTTP status is not correlation evidence.

## Correlation

One step is one complete reconstructed TCP session with its existing C2S/S2C pair.
Persistent connections containing multiple HTTP exchanges remain one step.
Do not split browsing/persistence into transactions or interleave directional turns.

The key is collector_id + client_ip + source_id. Client port, server IP, cookies
and status do not join the key. Different collectors, clients or sources never merge.

Within a key, sort by capture-time started_at ascending, then session ID.
Adjacent starts at most one second apart continue a chain; greater gaps split.
This is start-to-start sliding adjacency, not fixed buckets, end-to-start matching,
directional-payload timing or a maximum total duration. Starts 0.0, 0.8 and 1.6
seconds join; 2.7 begins another chain. Exactly one second joins.

The analyzer preserves collector_id and earliest observed non-empty payload
capture times first_payload_c2s_at / first_payload_s2c_at, including retransmissions.
Empty directions have unknown times. Local capture uses its local collector identity.
Collectors retain lightweight classification/forwarding; no extra aggregation or
per-packet persistence is introduced.

## Persistence and context

Compute chains on demand from finalized, stored session metadata, independently
of filters/pages. Do not add a live chain buffer, chain payload copy or global
ingest serialization. A late-persisted session may extend or merge chains.

One member must satisfy every active metadata/payload filter; opening the chain
includes other retained members. A flag-only filter therefore preserves preceding
non-flag context. A page boundary cannot terminate membership.
[Payload matching and chip application](session-filtering-v2.md) are shared with sessions.

Existing catalogs receive nullable metadata fields without rewriting segments.
Legacy rows lacking collector identity stay singleton chains; unknown directional
times remain unknown rather than being substituted with session start.
Normal retention applies and may remove members.

## List and card

“Group into chains” / “Группировать в цепочки” is off by default.
It changes browsing only; capture/metadata collection continue regardless.
Chain rows show service, client, start, count, directional byte totals and flags,
ordered newest first by chain start with a deterministic ID tie-breaker.
Card members are oldest first by (started_at, id), C2S immediately before its S2C.

Reuse Text/Hex, JSON formatting, flag highlighting and ordinary copy.
Headers identify session, direction and first-payload capture time, with
session/direction above the timestamp on narrow screens.
Individual cURL/Python export appears only on C2S and retains
[its single-request restrictions](mvp.md#individual-http-export).

The card scrolls between fixed-size, independently scrollable payload windows.
Window dimensions follow the standalone responsive layout and neither grow with
payload length nor shrink as more members load. Detail interactions cannot scroll
the background list, including at inner/outer boundaries.

A transparent overlay on each window's right side uses right: 0px, width: 26%,
min-width: 100px. It reaches the edge/scrollbar and routes native wheel/touch to
the card. Uncovered text scrolls its payload; copy/export buttons stay above the
overlay. The overlay appears only in chain detail.
Below Text/Hex, show “Scroll sessions here” / “Прокручивайте сессии здесь”.

Load member metadata progressively in pages of 20 and payloads near the visible
area. Abort obsolete requests and release offscreen payload bytes. Placeholders
preserve dimensions and inner scroll offsets survive unloading.
Mobile cards end above bottom navigation.

## Whole-chain Python export

A centered Python-logo toolbar action with localized “Copy chain as Python
(requests)” tooltip is available in Text mode. Preparation shows a spinner and
member-count progress; success/error uses ordinary copy feedback.

Fetch all member pages using the selected snapshot_id, including offscreen and
later-page sessions. Read C2S one at a time and keep generated script text;
preparation must not expand lazy card windows. Closing the card aborts it.
Retention-driven membership/count changes require reopening. Unsupported messages
or failed payload reads identify the session/request where possible and prevent
partial clipboard writes; never silently skip a request.

Split complete HTTP/1.x C2S requests using raw-byte header boundaries and
Content-Length before applying readable text/Format JSON per message.
Multi-request streams are supported here while browsing remains one TCP pair.
The input restrictions are otherwise those of individual export: origin-form,
uncompressed UTF-8, no-body/Content-Length framing and supported headers.
Binary, chunked, Upgrade and incomplete messages fail the entire export.

Script execution, destination/Host override, actual response output and conditional
cookie seeding are specified once in [Python replay](python-replay.md).
Dynamic tokens/IDs require manual edits; automatic extraction is outside scope.

## API and work limits

Both routes require the existing authenticated data boundary:

| Route | Parameters and response |
| --- | --- |
| GET /api/chains | Session metadata/payload filters, limit (default 100, max 200), opaque chain_cursor; returns { items, next_cursor } |
| GET /api/chains/{id}/sessions | snapshot_id, opaque cursor, limit (default 20, max 200); returns { chain, items, next_cursor }; unavailable anchor is 404 |

Summaries include collector/source/client, start/end, count, directional bytes,
flags, alerts, incomplete state and snapshot_id. Chain ID is its smallest retained
session ID; any member ID can resolve the chain through the member route.

Each request reads one SQLite transaction. First-page pagination pins maximum
persisted session ID; subsequent pages exclude newer insertions. Refresh takes
a new watermark and replaces the loaded span, removing obsolete split rows after
a late bridge. This is not archival isolation: retention can still remove members.
An open card retains its selected snapshot.

SQLite window queries form membership before content filters. Only complete key
filters (source/client IP) may prune input before gap calculation.
At most two blocking chain readers run concurrently. Each uses a 250 ms lock
timeout, 2 MiB page cache, file-backed temporary sorting and SQL interruption at
two seconds or 50 million VM steps. Busy/interrupted queries return 503;
narrower source/client filters may be needed for large catalogs.

Payload search scans at most 2,000 metadata-matching candidates in chunks of 200.
Continuation can resume inside a chain: an empty page with a cursor is valid.
The same candidate must satisfy all terms; load its payload once. Deadlines are
checked between candidates, so one read/byte scan may exceed that checkpoint.

At the live edge, grouped metadata refreshes every five seconds only when the
card is closed and payload search is inactive. Manual Refresh remains available.
See [storage](storage.md) and [verification](../test/session_chains/README.md).
