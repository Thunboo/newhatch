## App Logic:

```
                        ┌─────────────────────┐
                        │       api.rs        │
                        │      Axum API       │
                        │                     │
                        │ sources / sessions  │
                        │ payload / flags     │
                        └─────────┬───────────┘
                                  │
                           читает / меняет
                                  │
                                  ▼
┌─────────────┐           ┌──────────────────┐
│ Linux NIC   │           │      SQLite      │
│ eth0 / etc. │           │   index.sqlite   │
└──────┬──────┘           │                  │
       │                  │ sources          │
       │ Ethernet frames  │ session metadata │
       ▼                  └────────▲─────────┘
┌──────────────────┐               │
│    capture.rs    │               │ metadata
│                  │               │
│ AF_PACKET socket │               │
│ kernel BPF       │               │
└────────┬─────────┘               │
         │                         │
         │ raw frame               │
         ▼                         │
┌──────────────────┐               │
│    packet.rs     │               │
│                  │               │
│ Ethernet         │               │
│ IPv4 / IPv6      │               │
│ TCP parsing      │               │
│ C2S / S2C        │               │
└────────┬─────────┘               │
         │ ClassifiedPacket        │
         ▼                         │
┌─────────────────────────────┐    │
│          flow.rs            │    │
│                             │    │
│ worker #0 ─┐                │    │
│ worker #1 ─┤ FlowKey hash   │    │
│ worker #2 ─┤                │    │
│ worker #N ─┘                │    │
│                             │    │
│ TCP reassembly              │    │
│ C2S stream                  │    │
│ S2C stream                  │    │
│ FLAG regex                  │    │
└─────────────┬───────────────┘    │
              │                    │
              │ finished flow      │
              ▼                    │
┌─────────────────────────────┐    │
│       protocol.rs           │    │
│                             │    │
│ Raw TCP / HTTP / WebSocket  │    │
│ HTTP metadata extraction    │    │
└─────────────┬───────────────┘    │
              │ CompletedSession   │
              ▼                    │
┌─────────────────────────────┐    │
│        storage/             │────┘
│                             │
│ storage writer thread       │
│ batches ≤500 sessions       │
│                             │
│ metadata ───────► SQLite    │
│ payload  ───────► segments  │
└─────────────────────────────┘
```

## Now more distinctivly:
### `main.rs`
```
Config::from_env()
        │
        ▼
Catalog::open()
        │
        ├────► SQLite
        │
        ▼
storage::start_writer()
        │
        ▼
flow::start_workers()
        │
        ▼
capture::run()   ← Tokio task

In parallel:    api::serve()
```

### `capture.rs`

Uses Linux abstractions (similar to what is used in tcpdump) with BPF Filtering on Kernel level
```
libc::socket(
    libc::AF_PACKET,
    libc::SOCK_RAW | libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC,
    ...
)
```

### `domain.rs`

It declares `Source` struct - basically, it's all our defined sources to be filtered from traffic. Sources list is editable via api calls:
```
UI
 │
 │ POST /api/sources
 ▼
api.rs
 │
 ▼
SQLite
 │
 └───► watch channel ──► capture.rs ──► rebuild BPF filter
```
Then these sources are being captured by `capture.rs`

### `packet.rs`

It's a TCP packet rebuilder. It gathers raw Ethernet frames, then checks the IPv4 / v6, and other data and then outputs a proper TCP Packet.
It is also responsible for C2S / S2C direction recognition logic.

### `flow.rs`

There is a StreamAssembler, that checks TCP seq number to make sure that we reconstruct TCP Packet in right order

### `protocol.rs`

Upon `flow.rs` built C2S / S2C Packets being gathered - this part of code classifies TCP Connection with top-level protocols, like HTTP / WS (or other)
