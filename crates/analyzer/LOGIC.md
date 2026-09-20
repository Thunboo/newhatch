# Analyzer Logic

This document is a code-oriented map of `crates/analyzer`. Product and architecture decisions remain in `PROJECT.md` and `docs/`.

## Runtime

```text
AuthConfig::from_env + Config::from_env
                  |
                  v
        Catalog::open -> SQLite migration
                  |
                  +-> storage writer thread
                  +-> flow worker tasks
                  +-> local capture OR remote collector receiver
                  `-> authenticated Axum API
```

`main.rs` creates the catalog, bounded storage queue, flow workers and collector registry. It then selects one ingress path from `ANALYZER`:

- `local` starts live capture inside analyzer;
- `remote` listens for classified packets from standalone collectors.

Source mutations increment a watch-channel revision. Local capture reloads Sources and rebuilds its BPF filter; remote analyzer pushes the complete active Source snapshot to connected collectors.

## Data Paths

Local mode:

```text
Linux interface
  -> capture.rs: cooked AF_PACKET/SOCK_DGRAM + classic BPF + timestamp
  -> shared L3 IPv4/IPv6 + TCP parser/classifier
  -> IngressPacket { collector_id: "local", ClassifiedPacket }
  -> collector-aware flow-worker shard
```

Remote mode:

```text
standalone collector
  -> cooked AF_PACKET/BPF + shared parser/classifier
  -> bounded queue + versioned length-prefixed protobuf/TCP
  -> collector.rs receiver + optional peer-IP admission
  -> IngressPacket { collector_id, ClassifiedPacket }
  -> collector-aware flow-worker shard
```

Both paths then use the same analyzer pipeline:

```text
flow.rs
  -> TCP reassembly + streaming flag scan
protocol.rs
  -> raw TCP / HTTP / WebSocket-upgrade classification
storage/
  -> append-only payload segment + SQLite metadata transaction
api.rs
  -> authenticated browsing, filtering and payload retrieval
```

## Modules

### `config.rs`

Loads and validates analyzer mode, capture/API/storage limits, queue and flow limits, collector listener/admission configuration and `FLAG_REGEX`. The API listener must remain loopback-only. `FLAG_REGEX` is compiled as a byte regex and must not match an empty string.

### `capture.rs`

Implements analyzer-local Linux capture. It opens a nonblocking cooked `AF_PACKET/SOCK_DGRAM` socket, which strips interface-specific L2 headers and presents the same L3 layout for Ethernet, WireGuard/TUN, loopback and Docker bridge interfaces. It enables kernel receive timestamps, installs a generated classic BPF filter for enabled Source ports and rebuilds the socket when Sources change. Capture pauses when no Source is enabled.

The current path uses `recvmsg`; `PACKET_MMAP` is not implemented yet. Saturated worker queues drop and count new packets instead of growing without bound.

### `packet.rs`

Re-exports the shared collector parser/classifier. Input begins at an IPv4 or IPv6 header, not an Ethernet header. The parser supports IPv4/TCP and IPv6/TCP without extension-header walking, rejects unsupported/invalid layouts and extracts TCP sequence, SYN/FIN/RST and payload. Classification matches an enabled Source port and normalizes C2S/S2C endpoints.

IPv4 fragments and IPv6 extension headers remain unsupported. Link-layer VLAN parsing is unnecessary in cooked mode; target-host VLAN behavior still requires deployment validation.

### `collector.rs`

Implements analyzer-side remote ingress. It accepts persistent TCP connections, optionally restricts exact peer IPs resolved from `ALLOWED_COLLECTORS`, validates `CollectorHello`, sends `HelloAck` and current Sources, decodes versioned protobuf frames and routes packets using `CollectorId + FlowKey`. It tracks connection, traffic, queue/drop and error state for the Collectors API/UI.

The transport is neither encrypted nor application-authenticated. Per-collector PSK hardening remains backlog work.

### `flow.rs`

Hashes `CollectorId + FlowKey` into worker-local flow tables so equal endpoint tuples from different collectors cannot merge. `StreamAssembler` orders payload by TCP sequence number, trims retransmitted overlap and buffers out-of-order ranges within configured limits.

Flags are scanned incrementally with overlap and counted exactly again at finalization. A flow is finalized on RST, both-direction FIN, idle timeout or worker shutdown. Timed-out, truncated or not-fully-closed sessions are marked incomplete.

### `protocol.rs`

Classifies finalized streams as `raw_tcp`, `http` or `websocket`. It extracts HTTP method, host, path, response status and content type with `httparse`. WebSocket Upgrade/101 is recognized, but frames are not decoded.

### `storage/`

One blocking writer thread drains batches of up to 500 completed sessions. It rejects empty sessions, appends versioned C2S/S2C records to rotating segment files, syncs segment data, then commits metadata to SQLite. Payload retrieval uses segment filename, byte offset and record length and validates record identity, lengths and CRC32.

### `auth.rs` and `api.rs`

`auth.rs` implements startup Argon2id credential hashing, bounded in-memory server-side sessions and local-peer/origin checks. `api.rs` provides minimal public health, login/logout/current-user endpoints, Source CRUD, cursor-based session listing, metadata filters, bounded payload substring search, directional payload retrieval, flag-match ranges and collector status. Blocking SQLite and file operations run through `spawn_blocking`.

## Related Crates

- `crates/collector` owns standalone cooked capture, bounded forwarding, reconnect/backoff and transport counters.
- `crates/protocol` owns shared packet/source/domain types and the versioned protobuf envelope/framing.

## Suricata

Suricata is not part of the analyzer processing path. It runs as an optional passive parallel Compose service. EVE ingestion and session correlation are not implemented yet.
