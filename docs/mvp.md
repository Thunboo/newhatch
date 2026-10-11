# MVP Scope and UX

Players configure service ports, browse reconstructed TCP sessions, inspect both
directions, search payloads and locate flags. CAPTURE_INTERFACE is configured
through .env; there is no UI interface selector. Manual PCAP creation is not
part of normal operation.

The repository/service name is newhatch; browser/sidebar branding is «Нюхач» with
frontend/src/assets/logo.png. Screens remain operational and Packmate-like.

## Localization and navigation

EN/RU dictionaries select Russian when any browser language starts with ru,
otherwise English. A sidebar switch above Sign out persists newhatch_language,
updates document lang and locale-aware dates/numbers. No credentials enter storage.

Desktop sidebar is fixed; remote mode shows analyzer and aggregate collector
status lights, while local mode omits the collector light. Sources, Sessions and
Collectors views are available. Authentication states and expired-login behavior
are defined in [authentication](authentication.md).

## Sources and collectors

Sources support name/TCP-port creation, enable/disable, deletion and local
search/sort by name or port. Ports are unique; recreating a deleted port restores
its soft-deleted Source. Enabled-port changes replace analyzer/collector BPF.
Suricata's separate filter is not synchronized.

Collectors exposes remote connection/drop/reconnect counters. Capture remains
independent of the selected browsing screen. Local/split setup is in [README](../README.md).

## Session List

Show time, source, endpoints, protocol, byte totals, flags and alert count.
Merge unique sessions by ID in descending ID order; loaded count is the number
of unique frontend rows. Capture-time ordering under high RPS remains an open
follow-up because workers may finalize in a different order.

Newest-page polling runs every five seconds at the live edge, pauses in history
or during active payload search, and resumes near the top. Upward movement
performs one anchored catch-up refresh. An observer sentinel loads older cursor
pages. Requests/results remain bounded; stale generations cannot replace a new feed.

Flag-bearing rows are visually prominent and Flags-only narrows results.
Metadata already supplies contains_flag, flag_direction and flag_count; the UI
must not rescan every payload to decide row flags. Detail highlights matched ranges.
Suricata alert count is a placeholder until backend enrichment is implemented.

## Session Detail

Opening detail gives its row a black inner outline. Close with the button or
Escape; show “Esc to close” beside the button. Wheel input over detail cannot
scroll the background list, including at boundaries; exposed list space can scroll.

Show C2S and S2C, each with ordinary copy. Text decodes UTF-8, JSON escapes and
percent-encoded URL components; HTTP path labels use the same readable decoding.
Format JSON off keeps HTTP/standalone JSON compact; on pretty-prints and expands
JSON object/array string fields to bounded depth. Hex and stored bytes remain raw.
Protocol parsing must never remove raw visibility. Session values are raw_tcp,
http and websocket; only WebSocket Upgrade classification is currently implemented.

## Individual HTTP Export

C2S headers place cURL and Python logo actions before ordinary copy, with localized
tooltips and cUrl/python text fallback when assets fail. Export is frontend-only
and uses displayed UTF-8 text plus Format JSON; captured bytes validate framing.

Accept one complete, uncompressed UTF-8 HTTP/1.x request with origin-form target
and no body or Content-Length framing. Hex, partial/multiple requests,
binary/compressed/chunked bodies, Upgrade and unsupported/repeated headers disable
export with a reason. An incomplete TCP session may still hold one complete
exportable request. Ordinary copy remains available.

Use captured server IP/port by default and keep Host separately.
Decode query arguments into readable cURL options/Python params while preserving
repeated names; retain path escaping. Send displayed UTF-8 bodies with recalculated
Content-Length. Omit connection-specific headers and Accept-Encoding.
cURL requires 7.87+ for --url-query; Python requires Python 3 and requests.
Timeouts, redirects, destination overrides and response printing are defined in
[Python replay](python-replay.md). Runnable execution checks are in
[test/flag_test](../test/flag_test/README.md).

## Session Chains

“Group into chains” switches to related whole-session pairs; it is off by default.
The card retains full filtered context, orders C2S before S2C and provides fixed
payload windows within outer scrolling, lazy payload loading, C2S-only individual
export and a whole-chain Python action. Correlation, the right-side scroll strip,
snapshots, complete-export preparation and query budgets are defined once in
[session chains](session-chains.md).

## Search and Filtering

UI exposes Source, Protocol, Flags-only and accumulated literal AND payload chips.
[Session filtering](session-filtering-v2.md) owns matching, edited/applied state,
Enter/Add/Refresh/polling behavior, notices, backend limits and the array API.

Endpoint IP/port and started_after are API-only; there is no upper/end-time bound.
Session pages cap at 200 and a payload request scans at most 2,000 prefiltered
candidates. Keep cursor pagination and bounded rendering.

## Runtime and scope

.env.example owns runtime defaults; [README](../README.md) owns local/split
deployment. Source ports are managed through UI/API, not a second static list.
Compose packages analyzer/frontend, optional passive Suricata and collector,
plus an opt-in test-flag fixture on port 18080. Payloads/SQLite belong only to analyzer.
Use minimum capture capabilities rather than generic privileged containers.

The performance direction is responsive sustained traffic, bounded memory,
little or no userspace work for unmonitored ports and low vulnbox resource use.
A formal Packmate/Tulip comparison suite is outside MVP, but future measurement
must remain possible.

Prohibited infrastructure, protocol expansion, PCAP retention, enterprise scope,
RBAC and automatic SQLite VACUUM are listed in [fixed decisions](agent-decisions.md).
Per-collector Source assignment and fleet orchestration are outside MVP.
