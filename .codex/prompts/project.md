# Project Prompt

Проектный промпт для повторного использования.

## Default

Work inside the existing `newhatch` architecture.
Build a lightweight high-performance A/D CTF traffic analyzer with Packmate-like UX.

Before production code, read:

- `AGENTS.md`
- `PROJECT.md`
- `README.md`
- `docs/agent-decisions.md`
- relevant files in `docs/`

Respect fixed decisions:

- Rust/Tokio analyzer.
- Linux `AF_PACKET`, `PACKET_MMAP` where useful, kernel BPF filters.
- Suricata parallel enrichment only.
- SQLite metadata only.
- Append-only segment files for reconstructed payloads.
- TypeScript frontend.
- Docker Compose runtime.

Do not introduce heavy infrastructure or raw packet persistence for MVP.
Use TODO/TBD for unresolved choices unless the current implementation step requires a concrete decision.
