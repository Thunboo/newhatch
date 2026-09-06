# Analyzer Logic

This document is a code-oriented map of `crates/analyzer`. Product and architecture decisions remain in `PROJECT.md` and `docs/`.

## Runtime

```text
Config::from_env
      |
      v
Catalog::open -> SQLite migration
      |
      +-> storage writer thread
      +-> flow worker tasks
      +-> capture task
      `-> Axum API
```

`main.rs` creates the catalog, bounded storage queue and flow workers. Capture runs as a Tokio task while the API serves requests. Source mutations notify capture through a watch channel.

## Data Path

```text
Linux NIC
   |
   v
capture.rs
AF_PACKET + classic kernel BPF + receive timestamp
   |
   v
packet.rs
Ethernet/IP/TCP parse + source match + C2S/S2C normalization
   |
   v
flow.rs
consistent worker shard + TCP stream assembly + flag scan
   |
   v
protocol.rs
raw TCP / HTTP / WebSocket-upgrade classification
   |
   v
storage/
payload segment append + SQLite metadata transaction
```

## Modules

### `config.rs`

Loads and validates capture, API, storage, queue and flow limits from environment variables. `FLAG_REGEX` is compiled as a byte regex and must not match an empty string.

### `capture.rs`

On Linux, opens a nonblocking `AF_PACKET/SOCK_RAW` socket bound to `CAPTURE_INTERFACE`. It installs a generated classic BPF program for enabled source ports and rebuilds the socket when Sources change. Queue saturation drops packets and emits logarithmically throttled warnings.

The current path uses `recvmsg`; `PACKET_MMAP` is not implemented yet.

### `packet.rs`

Parses untagged Ethernet frames carrying IPv4/TCP or basic IPv6/TCP. It does not reassemble TCP. It matches source or destination TCP ports against enabled Sources, normalizes client/server endpoints and assigns C2S or S2C direction.

VLAN tags, IPv4 fragments and IPv6 extension headers are not handled yet.

### `flow.rs`

Hashes normalized flow keys into worker-local flow tables. `StreamAssembler` orders payload by TCP sequence number, trims retransmitted overlap and buffers out-of-order ranges within configured limits.

Flags are scanned incrementally with overlap and counted exactly again at finalization. A flow is finalized on RST, both-direction FIN, idle timeout or worker shutdown. Timed-out, truncated or not-fully-closed sessions are marked incomplete.

### `protocol.rs`

Classifies finalized streams as `raw_tcp`, `http` or `websocket`. It extracts HTTP method, host, path, response status and content type with `httparse`. WebSocket Upgrade/101 is recognized, but frames are not decoded.

### `storage/`

One blocking writer thread drains batches of up to 500 completed sessions. It appends versioned C2S/S2C records to rotating segment files, syncs segment data, then commits metadata to SQLite. Payload retrieval uses the recorded segment filename, byte offset and record length and validates record identity, lengths and CRC32.

### `api.rs`

The Axum API provides health, Source CRUD, cursor-based session listing, metadata filtering, bounded payload substring search, directional payload retrieval and flag-match ranges. Blocking SQLite and file operations run through `spawn_blocking`.

## Suricata

Suricata is not part of this crate's processing path. It runs as an optional passive parallel Compose service. EVE ingestion and session correlation are not implemented yet.
