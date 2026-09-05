Auto-removal: (flush when)
rows >= 5000
       OR
query time >= 100 ms (maybe)


Data digestion schema
NIC
 │
 │ kernel BPF:
 │ tcp and (port 8080 or port 8081 or port 9000)
 │
 ▼
AF_PACKET / PACKET_MMAP
 │
 ▼
TCP flow tracking
 │
 ▼
TCP reassembly
 │
 ├── streaming flag detection
 │
 ├── HTTP parsing
 │
 └── WebSocket parsing
 │
 ▼
completed / expired SESSION
 │
 ├── metadata ────────────────→ SQLite
 │
 └── reconstructed streams ──→ append-only segment

 Also - if a server replies with FLAG in payload - this TCP reply should be stored in sqlite with a pointer to a customer's payload, so that we can track exactly what player have sent to get a flag

SEGMENT_DURATION=30m
SEGMENT_RETENTION_COUNT=3
-> Total retention: 30 min × 3 = 90 min