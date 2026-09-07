# MVP Scope and UX

## Product Goal

The MVP should feel familiar to a Packmate user.

The player should be able to:

1. start the stack
2. select a network interface
3. define monitored sources/services by port
4. browse captured sessions
5. inspect reconstructed client/server traffic
6. search payloads
7. immediately identify sessions containing flags
8. filter to only stolen-flag traffic

The player should not need to manually create PCAP files.

The internal project/service name is `newhatch`. The current browser title and sidebar branding use the display name `Нюхач` and the root `logo.png` asset.

## Primary Screens

### 1. Sources / Services

Packmate-like source management.

Minimum UI actions:

- add source
- name source
- assign monitored TCP port
- enable/disable source
- delete source
- search sources by name or TCP port
- sort sources by name or TCP port

Example:

```text
Name            Port    Enabled
--------------------------------
web             8080    yes
auth            9000    yes
legacy          31337   no
```

Changes to enabled monitored ports should update the capture filter.

Current status: source CRUD, enable/disable, delete, local search/sorting and live analyzer BPF replacement are implemented. TCP ports are unique. Suricata's separate filter is not synchronized yet.

## 2. Session List

Primary table should show recent reconstructed sessions.

Suggested columns:

```text
time
source
src
dst
protocol
size
flag indicator
Suricata alert indicator
```

Example:

```text
12:03:21  web   10.0.4.17:51321 -> :8080  HTTP       1.2 KB
12:03:22  web   10.0.7.13:54018 -> :8080  HTTP       8.9 KB  FLAG
12:03:22  auth  10.0.8.91:60112 -> :9000  WebSocket  421 B
```

Sessions with stolen flags must be visually prominent.

## 3. Session Detail

The detail view should expose both reconstructed directions.

Conceptually:

```text
CLIENT -> SERVER

GET /api/object/123 HTTP/1.1
Host: service
...

----------------------------

SERVER -> CLIENT

HTTP/1.1 200 OK
Content-Type: application/json
...

{"flag":"..."}
```

Required representations:

- human-readable text
- raw bytes / hex-oriented fallback

HTTP and WebSocket parsing should improve readability without destroying access to raw reconstructed bytes.

## Session Protocol Values

MVP values:

```text
raw_tcp
http
websocket
```

A session must still be inspectable if application parsing fails.

## Flag UX

Global configuration:

```text
FLAG_REGEX=<regex>
```

Required session metadata:

```text
contains_flag
flag_direction
flag_count
```

Required UI:

- highlighted row when `contains_flag = true`
- clear flag marker
- filter:
  - `Show only traffic containing flags`
- session detail should make matched flag occurrences easy to locate

Do not make the UI depend on rescanning all payloads just to determine whether a row contains a flag. That information must already exist in SQLite metadata.

## Search and Filtering

Required MVP filters:

- source/service
- recent time range
- source IP
- destination IP
- source port
- destination port
- protocol
- contains flag
- payload substring/search expression

Payload search flow:

```text
metadata filters in SQLite
       |
       v
candidate sessions
       |
       v
Rust payload scan
       |
       v
matching sessions
```

The UI should not request or render unbounded result sets.

Pagination or cursor-based loading is required.

Current API coverage includes every metadata filter listed above except an upper/end-time bound; it provides `started_after` for the recent-time constraint. The current UI exposes source, protocol, contains-flag and payload filters. Endpoint IP/port and time controls remain API-only. Session API pages are capped at 200 rows, and one payload-search request scans at most 2,000 metadata-prefiltered candidates.

## Suricata UX

Suricata is enrichment, not a prerequisite.

Minimum MVP integration target:

- indicate if a session has one or more correlated alerts
- expose alert count
- optionally show alert names/signatures in session detail

If full correlation is not stable enough for the first coding milestone, the core analyzer must remain usable without it.

Current status: `suricata_alerts` exists in session metadata and the UI can display a non-zero count, but no EVE ingestion or correlation path populates it. The Compose service is passive and cannot block traffic.

## Configuration

Expected environment/configuration values include:

```text
CAPTURE_INTERFACE=<interface>
LISTEN_ADDR=127.0.0.1:3000
AUTH_USERNAME=<installation user>
AUTH_PASSWORD_HASH=<Argon2id PHC hash>
AUTH_SESSION_TTL_SECONDS=86400
AUTH_ALLOWED_SUBNETS=<comma-separated team CIDRs; empty means loopback-only>
AUTH_COOKIE_SECURE=false
FLAG_REGEX=<regex>
SEGMENT_DURATION=30m
SEGMENT_RETENTION_COUNT=3
DATA_DIR=/data
FLOW_WORKERS=<available CPUs clamped to 1..8>
PACKET_QUEUE_CAPACITY=8192
STORAGE_QUEUE_CAPACITY=2048
FLOW_IDLE_TIMEOUT=30s
MAX_ACTIVE_FLOWS_PER_WORKER=16384
MAX_STREAM_BYTES=4MiB
FRONTEND_PORT=8080
SURICATA_BPF_FILTER=<separate passive IDS filter>
```

Source/service port configuration may live in SQLite and be edited from the UI/API.

Current status: `CAPTURE_INTERFACE` is configured through the environment. There is no interface selector in the UI yet.

If early boot requires at least one source, a simple bootstrap mechanism may be added without replacing runtime source management.

## Docker Compose

Expected services:

```text
analyzer
frontend
suricata
```

An opt-in `test-flag` service is available under the `test` Compose profile. It exposes `GET /flag` on TCP port `18080` for local end-to-end capture checks and is not part of normal runtime.

SQLite and segment files are local files mounted into the analyzer container.

Do not create separate containers for infrastructure that can remain embedded/local.

Example conceptual volumes:

```text
./data:/data
./suricata:/etc/suricata
```

Exact compose networking/capabilities must be designed around packet capture requirements.

## Linux Capabilities

The analyzer and Suricata may require elevated packet capture capabilities.

Prefer the minimum required capability set rather than generic privileged containers where practical.

The exact Docker capability configuration should be validated during implementation.

## Performance Acceptance Direction

No formal benchmark suite is required in MVP.

However, all design/code decisions should be compatible with the intended outcome:

- no worse than Packmate in practical Attack/Defence usage
- lower or comparable resource use
- responsive UI during sustained traffic
- bounded memory behavior
- no application work for unmonitored ports

Benchmarking Packmate/Tulip against this tool is explicitly outside MVP, but should remain possible later.

## Out of MVP

- performance comparison framework
- generic plugin system
- arbitrary UDP protocol reconstruction
- historical PCAP export
- distributed capture
- multi-vulnbox aggregation
- Kubernetes
- Prometheus/Grafana integration
- database replication
- RBAC, registration and external identity providers (single-user login was explicitly added; see `authentication.md`)
- SQLite compaction/vacuum automation
