# Atomic HTTP Sessions

Implementation specification for [issue #28](https://github.com/Thunboo/newhatch/issues/28).
The behavior below is agreed but not yet implemented. This document is the
authoritative specification for the new HTTP session granularity.

## Goal and scope

Make an HTTP session atomic: one C2S request and its corresponding S2C response.
A payload window must never contain several HTTP requests or several final
responses. This applies both to standalone session details and to chain cards.

Apply the change to newly captured HTTP/1.x traffic. Raw TCP and WebSocket keep
their existing TCP-session model. HTTP/2, other protocol expansion and WebSocket
frame decoding are outside this task.

Existing saved traffic is not split, rewritten or backfilled. Legacy traffic
migration is not a requirement; verification uses newly generated traffic.

Known/unsorted classification and URL rules from
[issue #29](https://github.com/Thunboo/newhatch/issues/29) are deferred until this
session model is implemented. They are not part of #28.

## Session identity and payloads

The request/response pair **is the session**. Its ID is the existing session ID
used by the database, API, session list, detail view and chain membership.
Do not introduce a separate pair entity or a second pair ID.

One TCP connection can produce several independently stored HTTP sessions:

```text
TCP connection
  request A -> response A -> session ID A
  request B -> response B -> session ID B
  request C -> response C -> session ID C
```

Each session contains:

- C2S: exactly one request, including its headers and body, without intentional
  truncation;
- S2C: exactly the corresponding final response, including the retained part
  of its body;
- metadata for that session: source, collector, endpoints, capture timestamps,
  HTTP fields, directional stored-byte counts and flag information.

Metadata and flag matches must describe the individual stored session rather
than being copied from the entire TCP connection. Use the existing FLAG_REGEX
and capture-time basis. Retained payload bytes remain available to Text/Hex,
copy and export under their existing contracts.

## Analyzer preprocessing and persistence

The analyzer owns TCP reassembly, HTTP preprocessing and pending payload buffers
in both local and split deployments. The collector continues to capture and
forward packets through its bounded queue; it does not assemble HTTP sessions.

```text
captured TCP packets
  -> analyzer TCP reassembly
  -> incremental HTTP request/response preprocessing
  -> completed atomic HTTP sessions
  -> existing storage writer
  -> payload segments + SQLite session metadata
  -> session browsing and existing chain assembly
```

The preprocessor works as data arrives. Once a request and its final response
have been assembled, submit the session for storage immediately, even if the
TCP connection remains open. Submission still uses the existing bounded writer
queue and batching; it does not require a synchronous SQLite commit per response.

Pending parts remain in analyzer RAM. Do not create provisional database rows,
temporary payload files or a disk-spill mechanism for incomplete exchanges.
After a completed session is written, release its payload memory. The TCP
assembly buffers must stop retaining already emitted HTTP payloads, so a long
keep-alive connection does not accumulate all of its historical traffic.

Preserve the current storage architecture: SQLite metadata, append-only payload
segments, one logical writer, payload synchronization before metadata commit,
and existing retention. This task does not introduce a database migration to
PostgreSQL or another persistence system. Detailed storage mechanics remain in
[Storage](storage.md).

## Correct association

Pairing must use reconstructed TCP byte order and HTTP message boundaries.
Packet arrival order, timestamp proximity, a matching URL or equal endpoints
across different connections are not sufficient association evidence.

Within one HTTP/1.x connection, final responses correspond to outstanding
requests in request order. Support both sequential keep-alive exchanges and
multiple requests sent before their responses. Informational responses such as
100 Continue must not consume the next request or create a separate session.
These association rules follow
[RFC 9112, section 9.2](https://www.rfc-editor.org/rfc/rfc9112.html#section-9.2).

Determine message boundaries from HTTP framing, including Content-Length,
chunked framing, responses with no body and connection-delimited responses.
Payload contents that resemble a request/status line must not start another
message inside a body. Preserve captured bytes rather than using frontend text
decoding to find boundaries. Framing rules are defined in
[RFC 9112, section 6.3](https://www.rfc-editor.org/rfc/rfc9112.html#section-6.3).

Never join a request from one TCP connection to a response from another.
Different collectors must remain isolated. Retried requests on a new connection
are separate exchanges, not missing halves of an earlier session.

When a gap, capture starting mid-connection or invalid framing prevents reliable
association, do not display an assumed pair. Preserve previously completed,
reliably associated sessions; suppress the ambiguous remainder. In particular,
dropping or expiring one request must not shift all later responses onto the
wrong requests.

## Waiting and missing data

Wait up to **15 seconds without new payload data** for an unfinished exchange.
Receipt of new payload data resets the timer. This is an inactivity limit, not
a maximum total response duration: a response can take longer than 15 seconds
while it continues to arrive.

During this interval, reassembly can fill gaps using delayed, out-of-order or
retransmitted packets from the same connection. The analyzer is passive and
cannot request retransmission of data missed only by its capture path.

The frontend must not show an HTTP session with only C2S or only S2C. If the
other side never arrives, discard the unmatched exchange after waiting rather
than showing a one-sided session. Apply the same rule in standalone browsing
and chain cards.

A response whose body ends prematurely can still be retained when the C2S
request is intact, response bytes are present and their association is reliable.
Do not invent missing bytes or complete a pair by guessing. Timeout handling
must preserve association correctness for any subsequent traffic.

## Buffer limits and oversized messages

The analyzer may run on a vulnbox with 2 GB of RAM, as well as on a separate
host. Allocate payload memory on demand; do not reserve a full buffer allowance
for every admitted connection.

Raise the existing MAX_STREAM_BYTES default from 4 MiB to **16 MiB per direction**.
Preserve its per-direction semantics: C2S and S2C each have their own limit,
rather than sharing one 16 MiB allowance.

Apply bounded buffering to outstanding data. Already emitted sessions must not
consume the TCP connection's buffer allowance indefinitely. Keep existing
bounded packet/storage queues, active-flow admission and out-of-order controls;
do not introduce an unbounded list of pending requests.

There is **no new aggregate byte budget across analyzer sessions or queues**.
Do not add a global 64 MiB limit or temporary disk buffering. Limits remain
configurable through the existing configuration mechanism.

| Oversized direction | Required behavior |
| --- | --- |
| C2S exceeds 16 MiB | Discard the whole atomic HTTP session, including its corresponding response. Never display a deliberately shortened C2S request. |
| S2C exceeds 16 MiB | Retain at most the first 16 MiB of S2C, including headers, with the complete C2S request. Do not add a dedicated truncation notice, text or badge to the frontend. |

For oversized S2C, continue tracking the response framing after the retained
prefix is full. Skip the remaining body and resume with subsequent sessions
when boundaries and association remain reliable. A discarded oversized request
must likewise consume its corresponding response in the pairing state rather
than assigning that response to the next request.

If correct continuation cannot be established, use the ambiguity policy above.
Dropping an exchange is preferable to an incorrect request/response link.

## Chains and frontend behavior

Preprocessing splits the TCP traffic into atomic sessions before persistence.
The existing chain mechanism then groups stored sessions; preprocessing does
not persist a separate chain or force all sessions from one TCP connection into
one chain.

Retain the existing collector/client/source correlation key, one-second
start-to-start adjacency, on-demand assembly, filtering context, snapshots and
query limits from [Session Chains](session-chains.md). Session timestamps used
for this mechanism must belong to the atomic session, not every exchange being
assigned the original TCP connection's start time.

In a chain card, display:

```text
session A: C2S request A
           S2C response A
session B: C2S request B
           S2C response B
session C: C2S request C
           S2C response C
```

Every C2S window contains one request, and every S2C window contains its one
corresponding final response. A window containing ten GETs, five POSTs or several
responses violates the requirement even if the surrounding chain is correct.

Reuse current scrolling, lazy loading, filtering, highlighting, copy and export
behavior. Existing payload-search conditions apply to one atomic session;
chain opening continues to retain the other members as context. Do not expand
export's supported encodings or framing as part of this task.

## Verification and acceptance

Use the user's competition observations as the representative workload:
approximately 99.9% of sessions were below 1 MB, and responses above 10 MB were
estimated to occur around once per 10,000 requests. These are observed estimates,
not measured capacity guarantees. Include ordinary traffic and larger sessions
or chains around 5–10 MB; oversized-message fixtures verify the explicit limits.

Required checks:

1. Several exchanges in one keep-alive connection produce distinct session IDs
   and one request/response per session, with no duplicated or misplaced bytes.
2. A completed exchange becomes available while its TCP connection stays open.
3. Requests sent before responses, informational responses, body framing and
   message-like text inside bodies do not shift request/response association.
4. Out-of-order packets and retransmissions can fill a gap during waiting;
   an unresolved gap does not produce guessed pairs.
5. New payload resets the 15-second timer; an unmatched direction never becomes
   a frontend row. A continuously arriving response can last over 15 seconds.
6. Separate connections and collectors cannot supply each other's missing halves.
7. Oversized S2C retains its prefix without a dedicated frontend indication and
   permits a later valid pair. Oversized C2S drops its whole pair without shifting
   the next response.
8. Standalone and chain cards contain exactly one request or corresponding
   response in each directional window; chain assembly uses the existing rules.
9. Newly generated ordinary and 5–10 MB traffic retains the expected payloads,
   flags, search results and supported exports. Repeated exchanges on a long
   connection release completed payloads rather than accumulating their history.
10. Raw TCP and WebSocket retain their existing session model. Existing saved
    records are not transformed into atomic HTTP sessions.

Use available native/unit/API/frontend checks. Follow AGENTS.md's restriction:
agents must not run Docker, Docker Compose or interact with OrbStack. Any
container-dependent verification must be run by the user.
