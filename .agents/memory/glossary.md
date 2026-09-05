# Glossary

Термины проекта и их значения.

## Термины

- A/D CTF - Attack/Defence CTF format where teams attack services and defend their own vulnbox.
- vulnbox - constrained machine running team services during an A/D game.
- Packmate-like - workflow similar to Packmate: sources, sessions, reconstructed traffic inspection, search and flag visibility.
- source/service - user-visible monitored service definition, minimally `id`, `name`, `port`, `enabled`.
- monitored port - TCP port explicitly selected by the user for capture.
- kernel BPF - capture filter applied before packets enter userspace.
- `AF_PACKET` - Linux packet socket interface used for live capture.
- `PACKET_MMAP` / RX ring - Linux packet capture mechanism to reduce syscall overhead.
- Rust analyzer - core Rust service that captures, tracks flows, reassembles streams, detects flags, parses protocols and serves API.
- Suricata - parallel IDS/enrichment source; not in the Rust analyzer hot path.
- session - reconstructed bidirectional TCP conversation visible to the user.
- C2S - client to server reconstructed byte stream.
- S2C - server to client reconstructed byte stream.
- TCP reassembly - reconstruction of logical byte streams from TCP segments.
- `raw_tcp` - fallback protocol value when application parsing is unavailable or fails.
- HTTP/1.x - MVP HTTP protocol support.
- WebSocket - MVP protocol support including upgrade detection and frame extraction where practical.
- `FLAG_REGEX` - global regex used to detect flags in reconstructed streams.
- `contains_flag` - SQLite session metadata boolean for flag presence.
- `flag_direction` - session metadata enum: `none`, `c2s`, `s2c`, `both`.
- `flag_count` - count of matched flags in a session.
- segment file - append-only file storing reconstructed session payload bytes.
- `segment_id` - SQLite identifier for a segment file.
- `segment_offset` - byte offset of a session record inside a segment.
- `record_length` - length of a session record in a segment.
- retention - rotation/removal policy for old segments and their metadata.
- Community ID - possible Suricata correlation identifier to evaluate.
- backpressure - explicit behavior when bounded queues fill.
