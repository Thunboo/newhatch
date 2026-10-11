# Session Filtering v2 (Issues #25 and #22)

Status: implemented and locally verified on 2026-10-11 after user authorization.
This document records the approved behavior, API and acceptance criteria.

- [Issue #25: improved filtering](https://github.com/Thunboo/newhatch/issues/25).
- [Issue #22: stale session results after editing search](https://github.com/Thunboo/newhatch/issues/22).

## Goal and scope

Replace the single payload-search term with an accumulated list of removable
filter chips below the search input. The interaction is inspired by adding
filters in Grafana Explore for VictoriaLogs. In this version, every chip remains
a literal payload substring, using the existing session-search semantics.

Issue #22's application/reset behavior is implemented as part of Issue #25,
with the chip workflow below, for ordinary sessions and grouped chains.

## Matching rules

- Each chip contains one plain-text search term. Preserve current input trimming
  and exact, case-sensitive UTF-8 byte-substring matching against stored streams.
- Search the complete reconstructed C2S and S2C payloads, including captured
  protocol headers and bodies. UI text decoding and JSON formatting do not alter
  matching; metadata columns are not added to the searched bytes.
- Combine all chips with AND. A session matches only when every term appears in
  its C2S or S2C stream. Different terms may occur in different directions and do
  not need to be adjacent or in chip order.
- Do not concatenate C2S and S2C to create a match across their boundary.
- An empty applied chip list adds no payload constraint. Existing metadata
  filters still apply.
- Multiple chips are separate constraints, not replacements for the previous
  search term.
- Identical terms may be added repeatedly. Each addition creates another chip;
  duplicates are not removed and each chip counts toward the filter limit.
- LogsQL, field-specific expressions, regular expressions, and logical OR are
  outside this version. Characters such as `:`, quotes, `.*`, and the word `OR`
  are literal search text, not operators.

For chains, one member session must satisfy all payload terms and all active
metadata filters. Terms split across different member sessions do not qualify
the chain. Opening a matching chain still shows all retained members as context.
Whole-session persistence, chain membership and pair order remain unchanged.

## Editable and applied state

Keep these separate:

1. Search input: text being prepared for the next chip.
2. Editable chip list: the chips displayed below the input.
3. Applied chip list: the last applied snapshot used for requests and results.

Typing in or clearing the search input does not remove existing chips, change
the applied list or trigger a payload search. The pending-change notice concerns
differences between the chip lists, not unfinished text in the input.

## User actions

| Action | Editable chips | Applied filter and results |
| --- | --- | --- |
| Type or clear input text | Unchanged | Unchanged |
| Enter / Add-to-filter with a new term | Add the term as a chip | Immediately apply the entire editable list, including earlier removals |
| Enter / Add-to-filter with blank normalized input | Add nothing | Apply pending edits if there are any; otherwise do nothing without an API request |
| Click a chip's X | Remove that chip | Keep the previously applied list and results until manual application |
| Refresh | Use the complete displayed chip list | Apply that list and reload results |
| Change Source, Protocol or Flags-only | Keep the displayed chip list | Apply the new metadata filter together with the whole editable chip list, including pending removals |
| Scheduled polling | Unchanged | Use only the applied snapshot; never commit editable changes |

When the editable list differs from the applied list, display a notice beside
the chips. Agreed Russian copy:

> Фильтры изменены. Для применения нажмите Refresh.

The notice uses the same border, thicker bottom edge and rounded corners as the
close-hint key badge, with a `#f09892` background (updated on 2026-10-12).

Enter / Add-to-filter, Refresh and metadata-control changes apply the whole
edited set. Adding a new term must not silently preserve earlier removed chips
in the applied snapshot.
Starting application of a changed edited set hides the notice immediately. Do
not show it merely because the request is in flight: Enter/Add already applies
the filters and must not flash a request to press Refresh. New chip removals
during loading still show the notice because they have not been submitted.
If the current payload-filter application fails, display the error and restore
the notice; successful loading clears that failure state. Obsolete failures must
not restore a notice for a newer filter (updated on 2026-10-12).

Removing the last chip followed by Refresh removes the payload constraint.
Clearing only the search input does not reset the accumulated filters. Refresh
applies the chip list; adding the unfinished input text is the separate
Enter / Add-to-filter action.

Blank normalized input creates no chip. Enter / Add-to-filter with blank input
applies the displayed set only when pending changes exist. If there are no
changes, it must not issue an API request, reset pagination or reload the list.
Repeated blank submissions while the same application request is already in
flight must not create duplicate requests.

## Request lifecycle and polling

- Apply one complete snapshot atomically; do not send one request per chip.
- A changed applied filter resets the displayed result list and pagination
  cursor, then loads the first page. Never merge rows from different filters.
- Ignore or cancel obsolete responses from earlier filter generations, including
  older-page requests. Pagination must retain the applied snapshot of its feed.
- Refresh with an unchanged applied set retains the existing ordinary-session
  and chain refresh behavior, including their history/scroll handling.
- Scheduled polling never copies editable chips or input text into the applied
  snapshot, restores removed chips, or clears the pending-change notice.
- Preserve existing polling timing and eligibility gates. This specification
  does not independently enable polling during active payload search.
- Use the same chip/application semantics in ordinary sessions and chains.

Example: the applied filter is `A AND B`. Removing B leaves chip A and shows the
notice, while the applied snapshot remains `A AND B`. Any eligible timer refresh
continues using `A AND B`. Adding C with Enter immediately applies `A AND C`,
including the earlier removal of B. Refresh without adding C would apply A.
Blank Enter with that pending removal also applies A; another blank Enter after
successful application does nothing. Changing Source, Protocol or Flags-only
also applies A together with the newly selected metadata filter.

## Payload list API and limits

- `payload` is the text-search parameter, not the reconstructed session body.
  Change its API/data model from a scalar string to a list of search strings for
  both sessions and chains. A single term is a one-element list; an empty list
  adds no payload constraint. Use one list contract for single and multiple terms.
- Allow at most **10 chips/search terms**, including duplicates.
- Set the count limit in **one backend value**, shared by both search paths.
  Increasing that value must be sufficient to raise the limit. Expose the
  backend limit to the frontend; do not require a second hardcoded UI change.
- Each normalized search term is limited to **128 Unicode characters**. Count
  Unicode code points rather than UTF-8 bytes, consistently on frontend and
  backend; matching remains byte-substring based after encoding.
- The backend validates both limits. The UI uses the backend-supplied limits and
  reports excessive count/length; do not silently truncate a term or drop chips.

### Implemented transport

Both `GET /api/sessions` and `GET /api/chains` accept one URL-encoded `payload`
query value containing a JSON string array. For example, `payload=["POST","FLAG"]`
is sent as `payload=%5B%22POST%22%2C%22FLAG%22%5D`. JSON preserves duplicates,
commas, quotes and literal operator-looking text. Omit the parameter or send
`payload=[]` for no payload constraint. Scalar strings, malformed lists, blank
terms and limits violations return HTTP 400. Terms use browser-compatible trim.

Authenticated `GET /api/payload-search/limits` returns
`{ "max_terms": 10, "max_term_chars": 128 }`. Both values come from
`crates/analyzer/src/payload_search.rs`; increasing `MAX_PAYLOAD_TERMS` alone
raises the accepted backend and UI count. The UI keeps additions disabled until
limits arrive and offers retry if they cannot be loaded. Existing feed and
cursor response shapes are unchanged.

The nginx template allows 32 KiB request-line buffers because percent-encoded
full-length Unicode lists can exceed 8 KiB; auth/ingress checks cover this bound.

The old scalar `payload` contract is replaced; API clients must send arrays.
Session scans and chain scans share the validated matcher and load each
candidate payload once for all terms. No segment/schema/capture changes are needed.

## Implementation constraints

- Extend the session and chain search paths to receive and evaluate the complete
  applied `payload` list under the shared backend limits above.
- Evaluate AND on analyzer-side reconstructed session payloads. Filtering only
  the already loaded frontend rows or one API page does not satisfy the contract.
- Keep SQLite for metadata prefiltering and append-only segments for payloads;
  preserve bounded scans, pagination, chain snapshots and query budgets.
- Reuse a candidate session's loaded C2S/S2C bytes for all terms. Keep request
  work bounded as the number and length of terms increase.
- No capture/collector work, packet persistence, storage-format change or new
  external service is required by this specification.

## Acceptance checks

Verified on 2026-10-11 with 49 Rust tests, strict workspace clippy/fmt,
TypeScript checking, a production Vite build, eight Python ingress/config checks
and all 45 unique local Chromium/replay scenarios. The final nine filtering
scenarios were rerun after the notice fixes; Russian mobile layout was inspected.
Tests use temporary storage/mocked browser responses. No Docker or production
traffic was used; live Linux capture remains the existing operational follow-up.

On 2026-10-12, the notice lifecycle fix passed TypeScript checking, a production
Vite build and all 11 filtering browser scenarios. Delayed responses and DOM
mutation tracking verify no transient notice on Enter/Add in either mode;
current failures restore it and obsolete failures do not.

1. Enter and Add-to-filter create chips below the input and apply the full set.
2. Two or more terms combine with AND; one matching term alone is insufficient.
3. Terms in opposite directions of the same session match; a single term spanning
   the artificial C2S/S2C boundary does not.
4. Literal matching preserves current casing and byte semantics; operator-looking
   text remains literal.
5. Typing and clearing input alone do not change active filters or submit them.
6. Removing a chip changes the displayed list, shows the notice and retains the
   previous applied filter until Enter / Add-to-filter, Refresh or a change to
   Source, Protocol or Flags-only.
7. Adding a new chip after pending removals immediately applies the entire edited
   set, including those removals.
8. Refresh after removal applies the displayed chips; removing all chips and
   refreshing restores results without a payload constraint.
9. Eligible automatic refreshes retain the applied snapshot and pending notice.
10. Stale responses and pagination from an earlier filter cannot contaminate the
    new result list; request failures remain visible.
11. Chains require all terms in one matching member, retain full card context,
    and share ordinary-session add/remove/application behavior.
12. Existing metadata filters, authentication, bounded search and pagination keep
    working with the applied term list.
13. Duplicates create separate chips and consume separate slots; ten terms are
    accepted and an eleventh is rejected. One backend count-limit change also
    updates frontend behavior without changing a frontend constant.
14. Terms of 128 characters are accepted and 129 are rejected, including
    non-ASCII strings whose UTF-8 byte length exceeds their character count.
15. Blank Enter/Add applies pending changes without creating a chip. With no
    changes it sends no API request, including repeated key presses; repeated
    submissions of the same in-flight application do not duplicate requests.
16. Source, Protocol and Flags-only changes apply pending chip removals together
    with the new metadata filter in one snapshot, in both browsing modes.
17. Enter/Add never flashes the Refresh notice while automatically applying
    filters, including pending removals. A failed current application restores
    the notice, a successful retry clears it and a superseded failure is ignored.

Follow the repository's Docker restriction for verification: agents may run
appropriate host checks but must leave Docker-backed commands to the user.
