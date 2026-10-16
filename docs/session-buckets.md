# Known and Unsorted Traffic

Session and chain browsing share two buckets. **Unsorted** is the initial tab;
**Known** contains traffic matching an enabled rule for the authenticated user.
Known traffic remains stored, inspectable, searchable and exportable.

## Rules and matching

Open **Known attack rules** to create, edit, enable, disable or delete rules.
Each rule belongs to one Source and has a custom label. The server supplies its
owner from the authenticated installation identity; clients cannot assign owners
or read/change another owner's rules. The current single-user login model is
unchanged: teammates using the same NEWHATCH_USERNAME share rules. This does not
introduce individual accounts or roles.

A rule has 1..8 ordered steps. Each step has 1..12 conditions joined with AND.
Every condition in a step must match the same HTTP transaction. A transaction
cannot satisfy two steps in the same occurrence. Noise before/after a matching
sequence does not matter; the rule need not describe the entire attack.

| Field | Value |
| --- | --- |
| method | Request method |
| path | Decoded request path without query |
| target | Decoded path plus query; never scheme, host or port |
| query | Decoded query, or any value of the selected decoded parameter name |
| request_body | Readable request body |
| status | Decimal response status |
| response_body | Readable response body |

All fields support case-sensitive **equals**, **contains** and **regex**.
Equals also accepts an empty value (for example an empty body or query parameter).
Query parameter boundaries and repeated names are identified before decoding.
Decoding follows [readable-v1](session-filtering-v2.md#canonical-payload-text);
in particular, `+` remains literal in inspection/matching. HTTP export retains
its existing form-query conversion of `+` to space.

Regex uses Rust regex syntax, including alternation, groups, classes and Unicode.
Lookaround and backreferences are unsupported. Invalid/oversized regex returns
400 with a validation explanation before the stored rule changes. Each expression
is limited to 2,048 UTF-8 bytes and 256 KiB compiled/DFA size. Labels use at most
160 bytes; optional query names use 128 bytes. Each owner can keep at most 100 rules.

Steps are adjacent by default (`adjacent=true`, `max_gap=0`). With adjacency off,
max_gap (0..100) counts intervening events **between each pair of matched steps**.
Matching retains viable partial occurrences instead of greedily selecting a
middle event that cannot reach the final step.

Example: `GET /users` with query parameter `id` containing `../../../`, THEN
`GET /profile` with response containing `root:`. A preceding `/register` does
not participate in the signature. If the two selected transactions match, the
entire correlated chain, including registration and other context, is Known.
Standalone browsing gives every member the same bucket as its chain.

Membership and order follow [session chains](session-chains.md#correlation),
before protocol, flag or payload filters. Filtering cannot manufacture adjacent
events. Different Sources, collectors or clients never contribute steps to one
occurrence. Any enabled rule makes a chain Known; summaries include every matching
rule's label and the first matching sequence of session IDs/transaction ordinals.
Deleting/disabling one rule retains any other matching labels. Removing the last
match restores Unsorted on the next query. Late members and retention can also
change classification because chains are heuristic.

## HTTP events and retained data

New HTTP sessions already contain one request/final-response pair. Rules reuse
the ingest HTTP framer to extract events from retained payloads, including legacy
multi-request records, pipelining, informational replies, no-body responses,
Content-Length, chunked and connection-delimited bodies. Chunk framing is removed
for event body fields; compressed bodies are unavailable for body conditions.
Request/response pairing is never inferred from decoded bytes or other connections.

Ambiguous/malformed records, unsupported request targets, raw TCP and WebSocket
records are opaque barriers. They cannot satisfy a step or silently disappear
from adjacency checks. An unsupported legacy record remains inspectable; rules
conservatively treat that whole record as opaque. Event extraction shares the
ingest parser's 256-pending-request bound. Stored bytes are never split or rewritten.

## Persistence, reads and UI state

SQLite stores rule definitions, source and owner. Classification is derived at
read time instead of storing a global category on sessions or issuing mass
UPDATEs. Existing/new records therefore use the same rules immediately, and one
owner's triage cannot move another owner's traffic. SQLite/WAL and payload
segments remain the storage model; no external database is needed for this design.

Matching runs in existing bounded blocking chain readers, outside ingest and
the storage writer. One request reads at most 2,000 member records for classification,
retains only matcher state plus one record's extracted events, and checks the
two-second reader deadline between records. SQL interruption/admission limits
also apply. Exceeding a budget returns 503, never a guessed Unsorted result;
narrow source/client filters or reduce rules to reduce read work. A single
payload read/normalization/regex scan may overrun the between-record checkpoint.

Bucket filtering follows each bounded metadata/payload page. An empty page with
a continuation cursor is valid; both feeds continue loading automatically.
Rule changes made in the editor invalidate loaded rows and chain caches while
preserving unapplied search-chip edits. Refresh reconciles the scanned newest
session span; older loaded history retains its previous read until reloaded.
An open chain card keeps its selected snapshot until reopened.

Applied Source/protocol/flag/payload filters are saved separately per bucket in
`newhatch_session_buckets_v1:<encoded username>` localStorage. Switching buckets
restores that bucket's applied filters and clears unfinished input/edits. Reload
starts in Unsorted. Invalid/unavailable storage falls back to defaults; these are
preferences, not credentials. They can contain search terms and remain after logout.

Soft-deleted Sources keep their retained traffic and rules. Such rules can still
be edited/disabled or deleted; the editor identifies an unavailable Source by ID.

## API

All routes use existing authentication, loopback transport and mutation-origin checks.

| Route | Contract |
| --- | --- |
| GET /api/attack-rules | Current owner's rules, ordered by ID |
| POST /api/attack-rules | RuleInput; returns 201 and rule with server-assigned ID/owner |
| PUT /api/attack-rules/{id} | Replace owned rule; unknown/other-owned ID returns 404 |
| DELETE /api/attack-rules/{id} | Delete owned rule; 204 or 404 |
| GET /api/sessions or /api/chains | Optional bucket=known or bucket=unknown; omitted keeps the existing unpartitioned API without computing known_matches |
| GET /api/sessions/{id}/http-events | Ordered extracted events with zero-based transaction ordinals, or an empty list for opaque records |

RuleInput example:

```json
{
  "source_id": 1,
  "label": "Traversal",
  "enabled": true,
  "adjacent": true,
  "max_gap": 0,
  "steps": [
    [
      {"field": "path", "op": "equals", "value": "/users"},
      {"field": "query", "name": "id", "op": "contains", "value": "../../../"}
    ],
    [
      {"field": "path", "op": "equals", "value": "/profile"},
      {"field": "response_body", "op": "contains", "value": "root:"}
    ]
  ]
}
```

Verification: [shared text fixtures](../test/payload_text/README.md),
[backend/browser suites](../test/auth/README.md).
