# User Notes and Open Requirements

These notes preserve unresolved product requirements. Do not turn tentative values into implementation decisions without clarification.

## Auto-Removal / Flush Threshold

Original note:

```text
rows >= 5000
OR
query time >= 100 ms (maybe)
```

It is not yet clear whether this describes retention cleanup, a query/search budget, write batching or another subsystem. The implemented storage writer independently batches up to 500 completed sessions; that does not resolve this note.

## Data Digestion Pipeline

```text
NIC
 |
 | kernel BPF:
 | tcp and (port 8080 or port 8081 or port 9000)
 |
 v
AF_PACKET / PACKET_MMAP
 |
 v
TCP flow tracking
 |
 v
TCP reassembly
 |
 +-- streaming flag detection
 |
 +-- HTTP parsing
 |
 `-- WebSocket parsing
 |
 v
completed / expired SESSION
 |
 +-- metadata -----------------> SQLite
 |
 `-- reconstructed streams ---> append-only segment
```

## Flag Response Linkage

When a server response contains a flag, the session must preserve enough metadata or direct payload-range pointers to inspect the client/player request that triggered the response. Payload bytes must remain in append-only segment storage, not SQLite.

The exact request/reply linkage representation is still unresolved.

## Default Retention

```text
SEGMENT_DURATION=30m
SEGMENT_RETENTION_COUNT=3
```

Nominal retention is approximately 90 minutes. Rotation currently occurs when a completed session is appended after the active segment duration has elapsed.
