# Storage Design

## Goals

The storage layer must:

- be lightweight enough for a vulnbox
- keep ingest writes cheap
- avoid a heavyweight database
- support recent-session browsing
- support filtering by source/time/flag status
- allow direct random access to reconstructed payloads
- rotate old data predictably
- avoid persisting raw packets
- avoid storing payload bodies in SQLite

## Storage Components

```text
data/
├── index.sqlite
└── segments/
    ├── 1788580800000000.seg
    ├── 1788582600000000.seg
    └── 1788584400000000.seg
```

SQLite stores metadata and indexes.

Segment files store reconstructed session payloads. Current filenames are the segment creation time in Unix microseconds followed by `.seg`.

## Persistence Unit

The persistence unit is a reconstructed session, not a packet.

```text
SESSION
  ├── metadata
  ├── client -> server reconstructed bytes
  └── server -> client reconstructed bytes
```

Packets can be discarded after their relevant information has been incorporated into active flow/reassembly state.

## Append-Only Segment Files

Create a new segment periodically. The current writer checks rotation when it appends a completed session; it does not run a separate wall-clock rotation task.

Configuration:

```text
SEGMENT_DURATION=30m
SEGMENT_RETENTION_COUNT=3
```

Default example:

```text
3 segments * 30 minutes = approximately 90 minutes retained
```

Rotation:

```text
12:00 segment
12:30 segment
13:00 segment

13:30:
  create 13:30 segment
  expire 12:00 segment
```

The duration and retained segment count must be configurable.

## Record Addressing

Do not address a record by ordinal position.

Use:

```text
segment_id
byte_offset
record_length
```

Example:

```text
segment_id     = 42
byte_offset    = 81293812
record_length  = 5291
```

Payload retrieval:

```text
seek(segment, byte_offset) + read_exact(record_length)
```

This allows direct access without scanning preceding records.

## Segment Record Format

The exact binary format is implementation-defined, but it should be simple, versioned, and self-describing.

Conceptually:

```text
+-----------------------------+
| magic/version               |
| total_record_length         |
| session_id                  |
| c2s_length                  |
| s2c_length                  |
| optional metadata length    |
+-----------------------------+
| optional compact metadata   |
+-----------------------------+
| client -> server bytes      |
+-----------------------------+
| server -> client bytes      |
+-----------------------------+
```

Important requirements:

- deterministic binary format
- explicit version
- enough length information to skip/validate records
- safe detection of truncated tail records
- no JSON in the hot-path segment format
- no one-file-per-session design

## SQLite Role

SQLite is a metadata catalog and query index.

It is not the payload store.

It should contain enough information to:

- list sessions
- filter by monitored source
- filter by time
- filter flag-containing sessions
- locate payload record in segment storage
- correlate Suricata alerts
- support UI pagination

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
    http_content_type   TEXT
);
```

IPv4 addresses are stored as 4-byte blobs and IPv6 addresses as 16-byte blobs. Protocol and flag direction are compact integer enums. `suricata_alerts` is reserved but is not populated until correlation is implemented.

## Initial Indexes

Avoid over-indexing.

Start with only indexes justified by MVP query patterns:

```sql
CREATE INDEX idx_sessions_started_at
ON sessions(started_at DESC);

CREATE INDEX idx_sessions_source_time
ON sessions(source_id, started_at DESC);

CREATE INDEX idx_sessions_flag_time
ON sessions(contains_flag, started_at DESC);

CREATE INDEX idx_sessions_segment
ON sessions(segment_id);
```

Additional indexes should be added only after measured query patterns justify them.

## SQLite Write Model

Use one logical storage writer.

Conceptually:

```text
capture/reassembly workers
          |
          v
    bounded channel
          |
          v
    storage writer
       /       \
      v         v
 segment append SQLite batch
```

The current writer blocks for the first completed session, drains up to 499 additional queued sessions, appends all payload records, flushes and syncs the active segment, then commits the corresponding SQLite rows in one transaction:

```text
batch size: 1..500 sessions
```

There is no time-based batch flush yet. The `rows >= 5000 OR query time >= 100 ms` note in `docs/todo.md` remains unresolved and must not be conflated with this 500-session writer batch.

## Retention

When creating a new segment beyond the retention count:

1. identify the oldest expired segment
2. remove its session metadata using `segment_id`
3. remove the segment record from SQLite
4. remove the segment file
5. keep processing

The current implementation removes session and segment metadata in one SQLite transaction, commits it, and then removes the segment file. A file-removal failure is logged and leaves an orphan file rather than dangling SQLite pointers. Full crash reconciliation is still pending.

Example metadata removal:

```sql
DELETE FROM sessions
WHERE segment_id = ?;

DELETE FROM segments
WHERE id = ?;
```

## SQLite Vacuum Policy

Automatic aggressive SQLite compaction is outside MVP.

Do **not** run `VACUUM` every rotation.

Deleted SQLite pages may be reused by future inserts.

The MVP should focus on predictable ingest and query behavior rather than continuously shrinking the SQLite file on disk.

Future maintenance may add optional incremental cleanup if real-world testing shows it is useful.

## Crash Consistency

SQLite and segment files are separate persistence systems, so they are not one atomic transaction.

Preferred write order:

```text
1. serialize session record
2. append session record to segment
3. obtain byte offset and record length
4. persist SQLite metadata
```

The segment format must make recovery possible.

On startup, the implementation should eventually support:

```text
open latest segment
  |
  v
scan record headers
  |
  v
detect truncated/incomplete tail
  |
  v
truncate invalid tail if necessary
  |
  v
reconcile recoverable records with SQLite
```

Full recovery behavior can be implemented incrementally, but the storage format must not prevent it.

Current status: the write order above is implemented, including segment `sync_data` before the SQLite session transaction. Startup tail scanning, truncation and reconciliation are not implemented yet.

## Initial Binary Record Layout

Version 1 uses a fixed 52-byte little-endian header:

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

The header is followed by C2S bytes and then S2C bytes. The writer appends and syncs segment data before committing the corresponding SQLite metadata batch.

## Payload Search

Payload search is required, but no full-text engine should be introduced in MVP.

Implemented design:

```text
user query
   |
   v
SQLite metadata prefilter
   |
   v
candidate session ids/ranges
   |
   v
Rust scans only selected payload ranges
   |
   v
byte-substring matches
```

Example:

```text
source = service-a
time = last 10 minutes
payload contains = "FLAG{"
```

First reduce candidates through SQLite, then scan only the corresponding segment ranges.

The current API scans no more than 2,000 metadata-prefiltered candidate sessions per request, in pages of at most 200. Search is a literal byte-substring match, not a regular expression.

Do not duplicate all payload text into SQLite FTS for MVP.

## PostgreSQL Decision

PostgreSQL is intentionally not used in MVP.

Although PostgreSQL can be tuned for low connection counts and controlled autovacuum, it introduces machinery that is unnecessary for this workload:

- server processes
- shared buffers
- WAL
- checkpoints
- autovacuum
- background maintenance
- connection management

The expected workload is a better fit for:

```text
single local writer
local readers
metadata indexing
append-only payload files
short retention
```

Therefore:

```text
SQLite + append-only segments
```

is the selected architecture.
