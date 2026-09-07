# NewHatch Collector / Analyzer Split — Agent Summary

## Goal

Refactor the current NewHatch backend so traffic capture can run as a lightweight collector on the vulnbox, while the main analyzer runs on a separate host.

Target architecture:

```text
VULNBOX

┌─────────────────────┐
│ newhatch-collector  │
│                     │
│ AF_PACKET capture   │
│ BPF filtering       │
│ packet parsing      │
│ source classification│
└──────────┬──────────┘
           │
           │ persistent network stream
           ▼

ANALYZER HOST

┌─────────────────────┐
│ newhatch-analyzer   │
│                     │
│ flow workers        │
│ TCP reassembly      │
│ FLAG detection      │
│ protocol detection  │
│ storage             │
│ API/auth/frontend   │
└─────────────────────┘
```

The vulnbox-side component should only capture, minimally parse/classify, and forward traffic.
All heavy processing and persistence should remain on the analyzer host.

---

# 1. Recommended split point

The natural split point in the current codebase is between:

```text
capture.rs / packet.rs
        ↓
ClassifiedPacket
        ↓
flow.rs
```

Today `capture.rs` obtains packets, parses/classifies them, computes the target worker, and sends `ClassifiedPacket` to a local Tokio MPSC queue.

Replace this local-only handoff with a transport abstraction.

Current conceptual flow:

```text
ClassifiedPacket
    ↓
mpsc::Sender<ClassifiedPacket>
    ↓
FlowWorker
```

Target flow:

```text
VULNBOX

ClassifiedPacket
    ↓
serialize
    ↓
persistent transport

ANALYZER HOST

deserialize
    ↓
worker_index(flow_key)
    ↓
FlowWorker
```

This lets most of `flow.rs`, `protocol.rs`, storage, API, and frontend remain unchanged.

---

# 2. What stays on the collector

Collector should contain only the minimum required to observe traffic:

```text
capture.rs
packet parsing/classification
minimal config
network transport
collector stats/health
```

Responsibilities:

1. Open/bind AF_PACKET capture socket.
2. Apply/rebuild BPF filter.
3. Parse Ethernet/IP/TCP where applicable.
4. Determine Source and C2S/S2C direction.
5. Build `ClassifiedPacket`.
6. Send packets to analyzer.
7. Maintain bounded in-memory queue.
8. Reconnect if analyzer connection drops.
9. Report packet/drop/connection statistics.

Do not keep on collector:

```text
SQLite
session storage
payload segment storage
TCP reassembly
FLAG regex
HTTP/WebSocket classification
REST API
frontend
auth UI
```

---

# 3. What stays on the analyzer

Analyzer keeps:

```text
flow workers
FlowState
TCP stream reassembly
FLAG detection
protocol detection
CompletedSession
SQLite catalog
segment files
API
auth
frontend integration
```

Analyzer receives `ClassifiedPacket` values from collectors and injects them into the existing flow-worker pipeline.

---

# 4. Shared protocol crate

Create a shared crate for wire/domain types used by both collector and analyzer.

Suggested workspace layout:

```text
crates/
├── collector/
│   ├── src/main.rs
│   ├── src/capture.rs
│   └── src/transport.rs
│
├── analyzer/
│   ├── src/main.rs
│   ├── src/flow.rs
│   ├── src/protocol.rs
│   ├── src/storage/
│   ├── src/api.rs
│   └── src/auth.rs
│
└── protocol/
    ├── src/lib.rs
    ├── src/packet.rs
    └── src/messages.rs
```

Move/share types such as:

```text
FlowKey
TcpPacket
ClassifiedPacket
Direction
Source configuration DTOs
collector/analyzer transport messages
```

Do not duplicate equivalent structs in collector and analyzer.

---

# 5. Prefer sending `ClassifiedPacket`, not raw Ethernet frames

Recommended wire payload: `ClassifiedPacket`.

Include enough information for analyzer-side flow processing:

```text
timestamp
source_id
direction
client IP
client port
server IP
server port
TCP sequence
SYN/FIN/RST
payload bytes
```

Advantages over raw-frame forwarding:

- less bandwidth;
- analyzer does not need link-layer-specific parsing;
- avoids tying analyzer to Ethernet/VLAN framing;
- avoids carrying irrelevant L2 headers;
- current `flow.rs` can remain mostly unchanged;
- easier future support for L3 tunnel interfaces.

Do not make raw Ethernet frame forwarding the default architecture unless there is a strong reason.

---

# 6. Move worker selection to analyzer

Currently capture code computes:

```text
worker_index(flow_key, workers.len())
```

and sends directly to the chosen worker.

After the split, collector must NOT know analyzer worker count.

Target:

```text
collector
   ↓
ClassifiedPacket
   ↓
analyzer ingress
   ↓
worker_index(flow_key)
   ↓
worker N
```

Analyzer-side ingress owns worker distribution.

This preserves the current invariant:

> all packets belonging to the same FlowKey must always go to the same FlowWorker.

---

# 7. Source configuration must become remote

Current capture logic reads enabled Sources from the analyzer SQLite catalog.

After separation, collector must not require analyzer SQLite.

Analyzer remains source-of-truth for Sources and pushes the active configuration to collector.

Suggested control flow:

```text
Analyzer                         Collector
   │                                │
   │ SetSources / UpdateSources     │
   ├───────────────────────────────►│
   │                                │
   │                         rebuild BPF
   │                                │
   │◄───────────────────────────────┤
   │   Packet / Stats / Heartbeat   │
```

Collector should maintain an in-memory source configuration.

Whenever sources change in the analyzer UI/API:

1. analyzer updates SQLite;
2. analyzer sends updated source configuration to connected collector(s);
3. collector rebuilds the capture/BPF filter.

Do not copy the analyzer SQLite database to vulnbox.

---

# 8. Use a persistent bidirectional connection

Prefer one persistent collector ↔ analyzer connection.

Analyzer → collector messages:

```text
HelloAck
SetSources
UpdateConfig
Ping
Shutdown/Reload if needed
```

Collector → analyzer messages:

```text
CollectorHello
Packet
Stats
DroppedPackets
Heartbeat
Error/Status
```

This is preferable to opening one connection per packet or polling configuration independently.

---

# 9. Transport format

For an initial implementation, a simple persistent binary stream is sufficient.

Conceptually:

```text
[length][message]
[length][message]
[length][message]
...
```

Use an explicit versioned wire protocol.

Preferred serialization for a long-lived split architecture:

```text
protobuf
```

Reason:

- stable schema;
- explicit compatibility;
- easier version evolution;
- safer than directly serializing Rust structs with implementation-specific binary layouts.

A compact Rust-native serializer such as `postcard`/`bincode` may be acceptable for a prototype, but protobuf is preferred if collector/analyzer version skew is expected.

---

# 10. Backpressure and packet-loss policy

Do not create an unbounded queue on vulnbox.

The collector must use a bounded in-memory queue.

If analyzer is unavailable or too slow:

```text
capture
   ↓
bounded queue
   ↓
queue full
   ↓
drop packet
   ↓
increment metric/log counter
```

Do not allow the collector to consume unlimited RAM.

Do not spool unlimited packet traffic to vulnbox disk by default.

For an A/D CTF sensor, protecting the vulnbox is more important than guaranteeing zero packet loss.

Expose counters such as:

```text
captured_packets
sent_packets
dropped_packets
queue_depth
reconnect_count
connection_state
```

---

# 11. Reconnect behavior

Collector should reconnect automatically if analyzer becomes unavailable.

Requirements:

- exponential or bounded backoff;
- do not busy-loop;
- keep capture process alive;
- bounded queue only;
- stale queued packets may be dropped if necessary;
- after reconnect, collector re-sends Hello/status;
- analyzer re-sends active source configuration.

Do not require manual collector restart after analyzer restart.

---

# 12. Security of collector ↔ analyzer channel

The collector stream contains intercepted vulnbox traffic.

It is believed that players network - is trusted. Since that - no need for extra security layer for cryptography in collector <-> receiver channel

But at minimum:

- Analyzer ui should have new sidebar menu - info about connected collector.

It is asserted, that only one collector is connected at the same time. 

---

# 13. Multiple collectors

Design transport messages so adding more vulnboxes later does not require another major refactor.

Conceptually:

```text
collector A ─┐
collector B ─┼──► analyzer ingress ─► flow workers
collector C ─┘
```

Include collector identity in connection/session metadata.

Be careful about FlowKey collisions between different collectors.

If two vulnboxes can produce the same client/server tuple, analyzer-side flow identity should include collector identity.

Recommended future-safe flow identity:

```text
CollectorId + FlowKey
```

rather than only:

```text
FlowKey
```

This is important if multiple vulnboxes are analyzed by one backend.

In the end - app should be ready to accept data from several collectors, but the actual attack&defence workflow expects single collector

---

# 14. Preserve existing analyzer behavior

Avoid rewriting working logic unnecessarily.

Try to preserve:

```text
FlowWorker
StreamAssembler
FLAG scanning
protocol::classify
CompletedSession
storage writer
SQLite catalog
segment storage
REST API
frontend
auth
```

The major new analyzer component should be:

```text
network ingress
    ↓
deserialize packet
    ↓
worker_index
    ↓
existing FlowWorker senders
```

---

# 15. Suggested abstraction

Introduce an abstraction around packet output from capture.

Conceptually:

```rust
trait PacketSink {
    async fn send(&self, packet: ClassifiedPacket) -> Result<()>;
}
```

Possible implementations:

```text
LocalPacketSink
RemotePacketSink
```

`LocalPacketSink` is useful during migration/testing because it can preserve current monolithic behavior.

`RemotePacketSink` serializes and forwards packets to analyzer.

This allows a staged refactor instead of a single disruptive rewrite.

---

# 16. Migration strategy

Recommended incremental implementation:

## Phase 1

Refactor current capture pipeline to send through a `PacketSink` abstraction while keeping everything in the same process.

No network split yet.

Verify all existing tests still pass.

## Phase 2

Move shared packet/domain types into `crates/protocol`.

Keep local mode working.

## Phase 3

Implement analyzer network ingress and `RemotePacketSink`.

Run collector and analyzer on the same host first.

## Phase 4

Move collector to separate container/network namespace.

Verify:

- packet forwarding;
- source configuration updates;
- reconnect behavior;
- drop counters;
- flow reassembly correctness.

## Phase 5

Move collector to actual vulnbox and analyzer to remote host.

Just add some info to project-root/README.md on how to deploy new setup - briefly. Actual Phase 5 will be executed by developer

---

# 17. Testing requirements

Add tests for:

## Serialization

Round-trip all packet/control message types.

## Worker routing

Packets for the same `CollectorId + FlowKey` always reach the same worker.

## Reassembly across transport

Send packets out-of-order through collector transport and verify analyzer reconstructs the same stream as monolithic mode.

## Source updates

Analyzer changes Source list → collector rebuilds BPF/config.

## Disconnect

Analyzer goes down → collector does not crash or grow memory without bound.

## Reconnect

Analyzer returns → collector reconnects and resumes sending.

## Queue overflow

Simulate slow analyzer and verify packet drop counters increase instead of unbounded memory growth.

## Authentication

Unauthenticated collector connection must be rejected.
Actual connection flow should work like this:
- collector on vulnbox is set up to send data to receiver by some variable like: `RECEIVER_ADDR=IP` and `RECEIVER_PORT=PORT`
- receiver MUST have manifested such source in its config - othervise - traffic if droped, no receiver_hello message for establishing connection

This should lead to other teams to be unable to spam receiver anyhow

## Multiple collectors

Same FlowKey from two different collectors must not merge into one FlowState.

---

# 18. Important design decisions

Use these defaults unless implementation constraints prove otherwise:

1. Split after `ClassifiedPacket`.
2. Do not forward raw Ethernet frames by default.
3. Collector stays lightweight.
4. Analyzer owns flow-worker selection.
5. Analyzer remains source-of-truth for Sources.
6. Use persistent bidirectional transport.
7. Use bounded collector queues.
8. Prefer dropping packets over exhausting vulnbox resources.
9. Secure collector ↔ analyzer connection approval (both sides have manifested each other so they both send hello packets to begin trafic flow)
10. Include collector identity in analyzer-side flow identity for multi-vulnbox support.
11. Preserve current flow/storage/API implementation where possible.
12. Keep a local/monolithic transport mode during migration if practical.

---

# 19. Final target data flow

```text
VULNBOX

Linux interface
      │
      ▼
AF_PACKET
      │
      ▼
BPF filter
      │
      ▼
packet parser
      │
      ▼
ClassifiedPacket
      │
      ▼
bounded sender queue
      │
      ▼
secure persistent transport
      │
      │
      ▼

ANALYZER HOST

network ingress
      │
      ▼
deserialize
      │
      ▼
CollectorId + FlowKey
      │
      ▼
worker_index()
      │
      ▼
FlowWorker
      │
      ▼
TCP reassembly
      │
      ▼
FLAG / protocol detection
      │
      ▼
CompletedSession
      │
      ▼
storage writer
    ┌─┴────────────┐
    ▼              ▼
 SQLite        segment files
    │
    ▼
 API/auth/frontend
```

The objective is to turn the vulnbox component into a lightweight traffic sensor while keeping the existing NewHatch analyzer logic centralized and mostly intact.
