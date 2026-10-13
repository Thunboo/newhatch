# Session Filtering

Current contract for [#25](https://github.com/Thunboo/newhatch/issues/25) and
[#22](https://github.com/Thunboo/newhatch/issues/22), shared by sessions and chains.
Text highlighting is defined by [#33](https://github.com/Thunboo/newhatch/issues/33).

## Matching

Each removable chip is one trimmed, literal, case-sensitive UTF-8 byte substring.
Search complete stored C2S/S2C streams, including protocol headers and bodies,
before UI decoding or JSON formatting. Combine terms with AND: every term must
occur in either direction of the same session. Terms may match different
directions, but never join bytes across the C2S/S2C boundary.

An empty applied list adds no payload constraint; metadata filters still apply.
Duplicates remain separate chips and consume separate limit slots.
LogsQL, regex, field expressions and OR are unsupported; operator-looking text
is literal. A chain matches only when one member satisfies all payload and
metadata conditions. Opening it retains all available members as context.

## Text highlighting

In standalone and chain Text windows, highlight every occurrence of the applied
payload terms in both C2S and S2C, including retained context members. Use the
same literal, case-sensitive UTF-8 byte matching as filtering, including
overlapping occurrences; duplicate chips do not create duplicate markup.
Draft input and pending chip removals cannot change highlighting before application.
No applied terms means no additional highlighting. Hex remains unmarked.

The frontend uses already loaded bytes and the applied snapshot, without new API
requests or changing backend filtering. It carries match marks through UTF-8,
JSON/percent escape decoding, JSON formatting and embedded-JSON expansion to
the corresponding visible characters. For example, applied `%41` marks its
displayed `A`, without marking an unrelated literal `A` elsewhere. Formatting
and copy/export text retain the existing payload-display contract.

Payload matches use green #86D9A0 with dark text. Flags retain yellow #F3C840;
overlapping characters keep the yellow background with a green underline.
Recompute for loaded bytes, applied terms, flag ranges or Format JSON changes,
not ordinary renders or Text/Hex toggles.

## Input, edited chips and applied snapshot

Keep input text, displayed editable chips and the request/result snapshot separate.
Typing or clearing input changes neither existing chips nor active filtering.

| Action | Behavior |
| --- | --- |
| Enter / Add-to-filter with a nonblank term | Add a chip and immediately apply the entire edited list, including earlier removals |
| Blank Enter / Add-to-filter | Add nothing; apply pending edits, otherwise send no request |
| Chip X | Remove the displayed chip; retain the previous applied list/results |
| Refresh | Apply displayed chips and reload; ignore unfinished input |
| Source / Protocol / Flags-only change | Apply new metadata and all displayed chips atomically |
| Scheduled polling | Use the applied snapshot; never submit pending chip edits or input |

Repeated blank submissions, including while the same application is in flight,
must not reload, reset pagination or create duplicate requests.
Removing all chips and applying clears the payload constraint.

For example, removing B from applied A AND B leaves chip A and a pending notice.
Adding C immediately applies A AND C; Refresh or blank Enter instead applies A.

## Pending notice and failures

When edited chips differ from the applied snapshot, show beside them:

- EN: “Filters changed. Press Refresh to apply.”
- RU: “Фильтры изменены. Для применения нажмите Refresh.”

Use the close-hint key badge's border, thicker bottom edge and rounded corners,
with background #f09892.

Starting application of changed chips hides the notice immediately; Enter/Add
must not flash it during loading. A new removal during that request still shows
the notice because it has not been submitted. A failed current payload-filter
application displays the error and restores the notice; successful loading clears
the failure state. Obsolete responses or errors cannot affect a newer snapshot.

## Requests and polling

Apply one complete snapshot, not one request per chip. A changed applied filter
clears rows and pagination before loading its first page. Ignore or cancel stale
first-page and older-page replies; never merge results from different filters.
Pagination retains its feed's snapshot. Unchanged Refresh preserves ordinary
session/chain history and scroll behavior.

Polling keeps existing timing/eligibility gates and does not run during active
payload search. It cannot restore removed chips or apply pending edits.
See [live-feed UX](mvp.md#session-list) and [chain snapshots](session-chains.md#api-and-work-limits).

## API and limits

Both GET /api/sessions and GET /api/chains accept one URL-encoded payload
query value containing a JSON string array, e.g.
`payload=%5B%22POST%22%2C%22FLAG%22%5D`.
Omit it or use `payload=[]` for no constraint. Scalar strings, malformed lists,
blank terms and limit violations return HTTP 400. Terms use browser-compatible trim.

- Maximum terms: 10, including duplicates.
- Maximum normalized term length: 128 Unicode code points, not UTF-8 bytes or UTF-16 units.
- Backend validates both limits; UI reports violations without truncating terms or dropping chips.
- Authenticated GET /api/payload-search/limits returns
  `{ "max_terms": 10, "max_term_chars": 128 }`.
- Both limits live in crates/analyzer/src/payload_search.rs. Changing
  MAX_PAYLOAD_TERMS alone changes backend acceptance and the advertised UI limit.
  No .env override is implemented.
- Additions remain disabled until limits load; failures expose retry.

The scalar payload API is replaced by this array contract. Existing response
shapes and capture/storage formats remain unchanged.
nginx's 32 KiB request-line buffers accommodate percent-encoded full-length
Unicode lists that exceed its usual 8 KiB buffer.

SQLite prefilters metadata; the analyzer loads each candidate payload once and
evaluates the whole list over stored streams. Preserve bounded scans, continuation,
chain snapshots and query budgets; frontend-only filtering is insufficient.

## Checks

Behavioral coverage lives in test/auth/session-filtering.spec.cjs,
test/auth/backend.rs, analyzer payload-search/storage tests and
test/auth/test_nginx.py. Highlight coverage lives in
test/auth/payload-highlighting.spec.cjs. It checks applied/draft separation,
all-direction/context matches, flag overlaps, decoded/formatted source identity,
formatting/copy preservation and no extra payload reads.
Filtering checks cover AND/direction boundaries, limits and Unicode,
duplicates, blank submissions, pending removals, delayed/failing/stale requests,
pagination, both view modes and mobile/Russian layout.
Runnable checks: [test/session_chains](../test/session_chains/README.md)
and [test/auth](../test/auth/README.md).
