# Live Session Feed And Automatic Pagination

Status: implemented and verified.

Implementation notes:

- `frontend/src/App.tsx` keeps independent initial, newest-refresh and older-page request state, merges rows by session ID and preserves the oldest cursor.
- Live polling runs every five seconds only at the live edge. Scrolling upward from history performs one anchored catch-up refresh.
- An `IntersectionObserver` sentinel replaces the normal `Load older` button; pagination failures expose a compact retry action.
- `test/auth/session-feed.spec.cjs` covers unique loaded counts, automatic history loading, preserved history, polling pause and live-edge resume.
- Verified with the production frontend Docker build and the Playwright browser suite (`3 passed`).

## Goal

Make the Sessions list behave as a stable live traffic feed:

1. The header counter always reflects the number of sessions currently loaded in the browser.
2. New sessions do not move a player away from the traffic they are inspecting when they have scrolled down.
3. Reaching the end of the loaded list automatically loads the next older page (of a 100 sessions) without a `Load older` button.
4. Live polling is suspended while the player scrolls down or reads older traffic, avoiding requests and state updates they do not currently need.

This task is primarily frontend state and scroll behavior. The existing cursor-based Sessions API should remain unchanged unless implementation proves that it cannot satisfy the requirements.

## Current Behavior And Problem

The frontend currently uses one `loadSessions(append)` path for initial load, five-second live refresh and manual history pagination.

- Initial/live refresh replaces the full `sessions` array with the newest API page.
- `Load older` appends a cursor page.
- Every response overwrites `nextCursor`.
- One shared `loading` flag represents all request types.

This means a live refresh can discard previously loaded older pages, reset the pagination cursor and change the document height while the player is reading. Concurrent live and history requests can also complete out of order. The header currently derives its value from `sessions.length`; it must stay correct after every merge, append, refresh and reset.

## Required Feed State

Model these operations separately:

- `initial`: first page after opening Sessions or changing filters;
- `refresh`: newest page requested by polling or the refresh button;
- `older`: next cursor page requested by the bottom sentinel.

Keep a dedicated oldest-page cursor. A live refresh must not replace that cursor after older pages have been loaded.

Merge sessions by `id` and keep one deterministic order:

```text
id DESC
```

Requirements:

- Deduplicate overlapping pages and live refresh results by session ID.
- Never discard already loaded older sessions during a normal live refresh.
- Reset sessions, cursor and request generation when filters or payload search change.
- Ignore or cancel stale responses from the previous filter/search generation.
- Prevent duplicate simultaneous `older` requests for the same cursor.
- Preserve the current behavior where automatic live refresh is paused while session detail is open or payload search is active.
- Pause automatic newest-page polling while the player is away from the live edge and is scrolling down or stationary in older history.
- Use separate loading/error state where necessary so a background refresh does not blank the table or block unrelated interaction.

## Loaded Counter

The Sessions header must show the number of unique session rows currently held in frontend state.

- Initial page: show that page's unique row count.
- Older page: increase after the page is merged.
- Live refresh: increase only for newly discovered IDs.
- Duplicate/overlapping rows must not inflate the value.
- Filter/search reset must replace the value with the new result set's count.

This is a loaded-row counter, not the total number of sessions stored in SQLite. Do not add an expensive backend `COUNT(*)` query for this task.

## Stable Position During Live Updates

Treat the player as following the live edge only while they are at, or very close to, the top of the Sessions list.

### Player At The Top

- Merge newly discovered sessions at the top.
- Keep the viewport at the top so the newest traffic remains visible.

### Player Scrolled Down

- Do not poll or load newly arrived sessions while the player continues scrolling down or remains stationary in older history.
- Existing loaded rows remain unchanged except for automatic older-page loading at the bottom.
- When the player changes direction and scrolls upward, issue one catch-up refresh for the newest page. Do not issue one request per scroll event.
- Merge newly discovered sessions without automatically scrolling the player.
- Preserve the same visible anchor row and its pixel offset after React renders the new rows above it.
- Use a stable `session.id` anchor, not a row index.
- A practical implementation may capture the first visible row and its `getBoundingClientRect().top`, then compensate the scroll position in `useLayoutEffect` after the merge.
- Do not rely only on total `scrollHeight` deltas: responsive row/layout changes can make that imprecise.
- Do not reorder or remove loaded rows during a live refresh.

After that one upward catch-up request, keep periodic polling suspended until the player reaches or approaches the live edge. Reaching the live edge resumes normal five-second polling.

The following actions always request the newest page regardless of scroll position:

- full page reload or initial mount;
- explicit Refresh button click;
- filter or payload-search reset/load.

An explicit Refresh while scrolled down must still preserve the visible anchor after merging new rows.

The result must allow a player to compare adjacent historical packets/sessions without spending requests or frontend work on unseen new traffic.

Changing filters/search may reset the viewport to the top. Opening or closing session detail must not unexpectedly jump the underlying list.

## Automatic Older-Page Loading

Remove the visible `Load older` button. Add a sentinel after the Sessions table and observe it with `IntersectionObserver`.

When the sentinel enters or approaches the viewport:

- request the page identified by the dedicated oldest cursor;
- append/merge unique older sessions;
- update the oldest cursor from that response;
- continue automatically if the newly appended content is still too short to move the sentinel outside the preload area;
- stop when the API returns no next cursor;
- do nothing while an `older` request is already running;
- do nothing when the component/view is not active.

Use a modest positive `rootMargin` so loading starts shortly before the user reaches the absolute bottom. The sentinel must have stable dimensions and an accessible loading status. Do not trigger an unbounded request loop when the API repeatedly returns an unchanged cursor or only duplicate rows; detect lack of pagination progress and stop with an error/retry state.

History-load failures must leave existing rows and the cursor intact. Provide a compact retry action at the bottom only after an error; the normal successful flow has no manual load button.

## Request And Race Handling

- Initial/filter-reset responses may replace the current feed only if they belong to the latest request generation.
- Refresh responses merge at the top and do not update the oldest cursor of an expanded feed.
- Older responses merge at the bottom and may update only the cursor they were requested for.
- A slow response from an obsolete filter, cursor or component lifecycle must not mutate current state.
- The manual refresh icon always uses refresh semantics, regardless of scroll position, and must not collapse loaded history.
- An explicit/upward catch-up refresh may overlap an older-page request; both state updates must remain functional and ID-deduplicated.
- Scroll handling must be passive/throttled or animation-frame-coalesced. It must track direction and live-edge proximity without setting React state for every raw scroll event.
- One upward-scroll transition may trigger at most one catch-up request until it finishes or the direction changes again; repeated scroll events must not create a request storm.

## API And Backend Assumptions

Current API behavior is expected to be sufficient:

```text
GET /api/sessions?cursor=<oldest-loaded-page-cursor>&limit=<bounded-limit>
```

The response remains:

```json
{"items":[],"next_cursor":null}
```

No backend changes are planned for this task. If testing finds unstable cursor behavior under concurrent inserts, stop and document the exact failure before changing the API contract. Do not introduce offset pagination.

## Frontend Tests

- Initial load displays the correct unique loaded count.
- Loading an older page increases the counter by the number of unique new IDs.
- Overlapping refresh/history pages are deduplicated and do not inflate the counter.
- A live refresh does not remove already loaded older rows or replace their cursor.
- At the top, newly arrived sessions become visible at the live edge.
- Scrolling down and remaining in older history pauses five-second newest-page polling.
- Continuing to scroll down does not request or merge new sessions.
- Changing direction upward triggers exactly one newest-page catch-up request.
- After upward catch-up, periodic polling remains paused until the live edge is reached.
- Reaching the live edge resumes periodic newest-page polling.
- When scrolled down, upward catch-up or explicit Refresh keeps the same session row at the same visual offset after new rows are inserted above it.
- Explicit Refresh requests newest sessions even while the player is scrolled down.
- Automatic loading starts when the sentinel enters the preload area.
- Repeated observer callbacks while loading produce only one request for a cursor.
- If the sentinel remains visible after a short page, loading continues until content fills the viewport or pagination ends.
- `next_cursor=null` stops additional requests.
- An unchanged cursor/no-progress response stops looping and exposes retry/error state.
- Older-load failure preserves rows and allows retry.
- Filter and payload-search changes reset rows, cursor, counter and scroll position; stale prior responses are ignored.
- Manual refresh and live-edge-only five-second polling merge new sessions without collapsing expanded history.
- Desktop and mobile layouts do not jump or overlap while loading.

## Acceptance Criteria

- The header count always equals the number of unique sessions rendered/loaded in the frontend.
- A player who scrolls down does not spend requests or frontend work loading newly arrived sessions and stays anchored on the same traffic.
- Scrolling upward performs one anchored catch-up; periodic polling resumes only near the live edge.
- A player at the top continues to follow newest sessions.
- Reload and explicit Refresh always fetch the newest page regardless of scroll position.
- Older history loads automatically near the bottom until pagination ends.
- The normal `Load older` button is removed; only an error retry action may appear.
- Live refresh, filters, payload search and cursor pagination cannot overwrite each other's state with stale responses.
- No backend count query or offset pagination is added.
- Frontend typecheck/build and relevant automated/browser tests pass.
