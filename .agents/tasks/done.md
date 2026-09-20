# Done

Completed project work.

## Completed

- [x] 2026-09-20: Completed GitHub issue #7 defense in depth: protected-route and no-side-effect auth tests now cover collectors and mutations; nginx explicitly denies dotfile/database/segment paths; frontend data-volume isolation and real IPv4/IPv6 network behavior are regression-tested.
- [x] 2026-09-20 (consolidated record): Implemented the Rust analyzer/backend workspace, TypeScript frontend and Docker Compose scaffolds plus validated runtime configuration for capture, storage, queues and flow limits.
- [x] 2026-09-20 (consolidated record): Fixed the initial implementation choices for direct Linux capture, SQLite, HTTP parsing, React/Vite, capture capabilities/networking, flow idle timeout and the binary segment record format.
- [x] 2026-09-20 (consolidated record): Completed the core session pipeline: Source CRUD and kernel BPF rebuilds, live capture, flow identity, TCP reassembly, split-safe flag scanning and HTTP metadata parsing.
- [x] 2026-09-20 (consolidated record): Completed append-only segment writing, SQLite metadata/indexes, cursor session API, direct payload retrieval and bounded payload search.
- [x] 2026-09-20 (consolidated record): Completed the Packmate-like Sources/Sessions/detail/search/flag UI and rejection/removal of empty zero-byte `raw_tcp` sessions.
- [x] 2026-09-20: Implemented the merged live Sessions feed with paused history polling, anchored catch-up and automatic cursor pagination.
- [x] 2026-09-20: Implemented collector/analyzer local and remote modes, versioned protobuf forwarding, IP/FQDN endpoints, optional peer allowlist, Source synchronization, reconnect/drop counters and collector status UI.
- [x] 2026-09-20: Improved session detail with decoded text/path display, compact/pretty JSON modes, bounded nested JSON expansion, separate C2S/S2C copy actions and an `Esc` close hint.
- [x] 2026-09-20: Audited Markdown documentation, updated the analyzer code map, removed completed standalone task files and added a synchronization map for public docs and agent context.

- [x] 2026-09-07: Implemented single-user login with startup Argon2id hashing, bounded server-side sessions, team-CIDR nginx ingress and loopback backend authorization. Added `test/auth/` suites and updated runbooks. Actual target-host ingress validation remains in `active.md`.
- [x] 2026-09-07: Replaced Ethernet-only raw packet capture with cooked L3 capture after validating that NetBird `wt0` provides no Ethernet header. Added IPv4/IPv6 kernel-filter and L3 parser tests.

- [x] 2026-09-04: Created initial agent/Codex skeleton files and folders.
- [x] 2026-09-05: Synced agent/Codex context with `README.md` and `docs/*`.
- [x] 2026-09-05: Built the first executable analyzer/API/storage/frontend/Compose vertical slice.
- [x] 2026-09-05: Added the opt-in nginx flag-capture fixture on TCP port 18080.
- [x] 2026-09-05: Branded the UI as `Нюхач`, added the project logo, and added Sources search/sorting.
- [x] 2026-09-06: Added the canonical logo, now stored in `frontend/src/assets/logo.png` and bundled by Vite.
- [x] 2026-09-06: Audited implementation state and refreshed project/agent/Codex documentation.
