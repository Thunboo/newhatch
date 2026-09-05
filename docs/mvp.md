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

## Primary Screens

### 1. Sources / Services

Packmate-like source management.

Minimum UI actions:

- add source
- name source
- assign monitored TCP port
- enable/disable source
- delete source

Example:

```text
Name            Port    Enabled
--------------------------------
web             8080    yes
auth            9000    yes
legacy          31337   no
```

Changes to enabled monitored ports should update the capture filter.

Implementation details for live filter replacement may be deferred if necessary, but the product model must support it.

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

## Suricata UX

Suricata is enrichment, not a prerequisite.

Minimum MVP integration target:

- indicate if a session has one or more correlated alerts
- expose alert count
- optionally show alert names/signatures in session detail

If full correlation is not stable enough for the first coding milestone, the core analyzer must remain usable without it.

## Configuration

Expected environment/configuration values include:

```text
CAPTURE_INTERFACE=<interface>
FLAG_REGEX=<regex>
SEGMENT_DURATION=30m
SEGMENT_RETENTION_COUNT=3
DATA_DIR=/data
```

Source/service port configuration may live in SQLite and be edited from the UI/API.

If early boot requires at least one source, a simple bootstrap mechanism may be added without replacing runtime source management.

## Docker Compose

Expected services:

```text
analyzer
frontend
suricata
```

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
- authentication/RBAC
- SQLite compaction/vacuum automation
