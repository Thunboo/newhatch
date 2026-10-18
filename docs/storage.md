# Storage

Persist reconstructed sessions, not packets. SQLite holds metadata/indexes;
append-only segments hold C2S/S2C bytes. Do not add SQLite FTS or another payload
database. Discard empty raw_tcp sessions and remove legacy zero-payload raw_tcp rows.

## Files, addressing and rotation

DATA_DIR contains index.sqlite and segments/<creation-time-in-Unix-microseconds>.seg.
A session points directly to segment_id, segment_offset and record_length.
Retrieval seeks to the offset and reads exactly that record; do not scan preceding
records or address by ordinal position.

SEGMENT_DURATION defaults to 30m and SEGMENT_RETENTION_COUNT to 3.
Rotation is checked only while appending a completed session. Retention is by
segment count, not a continuous wall-clock age limit: downtime alone expires nothing.
Three busy 30-minute segments retain roughly 90 minutes, not a guaranteed time window.

## Binary record

Version 1 has a fixed 52-byte little-endian header:

```text
8 bytes   magic: NHSEGREC
u16       version
u16       header length
u64       total record length
i64       session id
u64       C2S length
u64       S2C length
u32       CRC32 of C2S || S2C
u32       reserved
```

The header is followed by C2S, then S2C bytes. Keep version/length/checksum
validation and direct offsets. No hot-path JSON or one-file-per-session layout.

## Implemented Schema

The current schema is created idempotently at startup. SQLite uses WAL mode, `synchronous=NORMAL`, foreign keys and a five-second busy timeout.

### `sources`

```sql
CREATE TABLE sources (
    id          INTEGER PRIMARY KEY,
    name        TEXT NOT NULL,
    port        INTEGER NOT NULL UNIQUE CHECK (port BETWEEN 1 AND 65535),
    enabled     INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    deleted     INTEGER NOT NULL DEFAULT 0 CHECK (deleted IN (0, 1))
);
```

Source deletion is soft. Creating a source on a previously deleted port restores that row; active TCP ports remain unique.

### `segments`

```sql
CREATE TABLE segments (
    id          INTEGER PRIMARY KEY,
    filename    TEXT NOT NULL UNIQUE,
    created_at  INTEGER NOT NULL,
    closed_at   INTEGER,
    size_bytes  INTEGER NOT NULL DEFAULT 0
);
```

### `sessions`

```sql
CREATE TABLE sessions (
    id                  INTEGER PRIMARY KEY,
    segment_id          INTEGER NOT NULL REFERENCES segments(id),
    segment_offset      INTEGER NOT NULL,
    record_length       INTEGER NOT NULL,
    started_at          INTEGER NOT NULL,
    ended_at            INTEGER NOT NULL,
    source_id           INTEGER NOT NULL REFERENCES sources(id),
    client_ip           BLOB NOT NULL,
    client_port         INTEGER NOT NULL,
    server_ip           BLOB NOT NULL,
    server_port         INTEGER NOT NULL,
    protocol            INTEGER NOT NULL,
    bytes_c2s           INTEGER NOT NULL,
    bytes_s2c           INTEGER NOT NULL,
    contains_flag       INTEGER NOT NULL DEFAULT 0,
    flag_direction      INTEGER NOT NULL DEFAULT 0,
    flag_count          INTEGER NOT NULL DEFAULT 0,
    suricata_alerts     INTEGER NOT NULL DEFAULT 0,
    incomplete          INTEGER NOT NULL DEFAULT 0,
    http_method         TEXT,
    http_host           TEXT,
    http_path           TEXT,
    http_status         INTEGER,
    http_content_type   TEXT,
    collector_id        TEXT,
    first_payload_c2s_at INTEGER,
    first_payload_s2c_at INTEGER
);
```

IPv4 addresses are stored as 4-byte blobs and IPv6 addresses as 16-byte blobs. Protocol and flag direction are compact integer enums. `suricata_alerts` is reserved but is not populated until correlation is implemented.

An idempotent startup migration adds the three nullable chain fields to existing
catalogs. New sessions preserve their originating collector (`local` for local
capture); directional timestamps are the earliest observed non-empty payload
capture times in Unix microseconds. Legacy NULL values remain unknown, and
collector-less records stay singleton chains. Migration does not rewrite segment
files or their direct record pointers.

### Rule and representation metadata

`attack_rules(id, owner, source_id, definition)` stores bounded, validated rule
definitions and an owner/ID index. IDs are AUTOINCREMENT so deleting a rule cannot
reuse its identity. Source deletion stays soft and does not remove retained rules.
Buckets are derived by the bounded readers; sessions have no global category.
See [rule semantics and limits](session-buckets.md).

`catalog_settings(key, value)` records the completed canonical flag-text version
and FLAG_REGEX. The [startup flag rebuild](session-filtering-v2.md#canonical-payload-text)
updates only session metadata in 200-row batches when that revision changes.
There are no stored decoded payloads or per-event body rows.

## Indexes

Keep indexes justified by query patterns:

```sql
CREATE INDEX idx_sessions_started_at
ON sessions(started_at DESC);

CREATE INDEX idx_sessions_source_time
ON sessions(source_id, started_at DESC);

CREATE INDEX idx_sessions_flag_time
ON sessions(contains_flag, started_at DESC);

CREATE INDEX idx_sessions_segment
ON sessions(segment_id);

CREATE INDEX idx_sessions_chain_key_time
ON sessions(collector_id, client_ip, source_id, started_at, id);
```

Additional indexes should be added only after measured query patterns justify them.

## Writer and crash consistency

One logical writer receives completed sessions through a bounded channel.
It waits for the first, drains up to 499 queued sessions, appends the batch's
payloads, flushes and sync_data's the segment, then commits SQLite rows in one
transaction. Batch size is 1..500; there is no time-based batch flush.

Segments and SQLite are separate persistence systems, not one atomic transaction.
Data sync precedes metadata commit so durable rows do not point to unsynced payloads.
Tail scanning/truncation and SQLite/file reconciliation at startup remain
unimplemented. The format must allow recovery of valid records and detection
of an incomplete tail.

The unassigned rows >= 5000 OR query time >= 100 ms note is tracked in
[open questions](../.agents/memory/open-questions.md); it is not this writer limit.

## Retention

On rotation beyond the count limit, remove expired sessions and segment rows in
one SQLite transaction, commit, then remove the file. Log a file-removal failure;
an orphan file is preferable to a dangling catalog pointer.
Full crash reconciliation remains pending.

Do not VACUUM each rotation. Deleted pages may be reused; automatic compaction is
outside MVP. Immediate disk reclamation beyond normal retention is an open question.

## Read queries

Payload search prefilters metadata in SQLite, then reads selected segment ranges.
It scans at most 2,000 candidates in pages/chunks of at most 200 and loads each
candidate once for all terms. Matching/API limits are defined in
[session filtering](session-filtering-v2.md).

Chains are computed on demand without a chain table or copied payloads.
Each request uses one SQLite transaction; pagination pins insertion IDs while
retention remains active. The complete correlation, filtering, reader admission,
SQL deadlines/cache and continuation contract is in
[session chains](session-chains.md#api-and-work-limits).
