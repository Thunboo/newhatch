# AGENTS.md

Базовые правила для Codex и других AI-агентов в этом проекте.

## Project Snapshot

- Internal name: `newhatch`; UI/display name: `Нюхач`.
- Продукт: lightweight high-performance A/D CTF traffic analyzer.
- Цель: дать Packmate-like workflow и UI при меньшем расходе CPU/RAM на vulnbox.
- Нормальный режим: live capture с интерфейса, без ручного создания PCAP.
- Основная сущность: reconstructed bidirectional TCP session.
- Язык общения с пользователем: русский, если пользователь явно не просит иначе.
- Документация проекта сейчас ведется на английском.

## Canonical Context

Перед архитектурой или production-кодом читать:

1. `PROJECT.md` - product goal, stack, constraints and current implementation status.
2. `README.md` - quick-start runbook.
3. `docs/agent-decisions.md` - фиксированные решения и запреты.
4. `docs/architecture.md` - pipeline, concurrency, reassembly, Suricata model.
5. `docs/storage.md` - SQLite + append-only segment storage.
6. `docs/mvp.md` - MVP UX, screens, filters, configuration.
7. `docs/todo.md` - текущие заметки пользователя.

Служебные файлы:

- `.agents/memory/project.md` - короткая память по проекту.
- `.agents/memory/decisions.md` - сжатый журнал решений.
- `.agents/memory/glossary.md` - терминология.
- `.agents/memory/open-questions.md` - вопросы, которые требуют уточнения перед реализацией.
- `.agents/tasks/backlog.md` - следующие задачи.
- `.codex/context.md` - быстрый контекст для новых сессий.
- `.codex/commands.md` - команды разработки и проверок.

## Fixed Technical Direction

Использовать:

- Rust, Tokio.
- Linux `AF_PACKET`.
- `PACKET_MMAP` / RX ring where useful.
- kernel BPF capture filters.
- Suricata as parallel enrichment.
- SQLite for metadata and indexes only.
- custom append-only segment files for reconstructed payloads.
- TypeScript frontend.
- Docker Compose.

Не вводить в MVP без явного решения пользователя:

- PostgreSQL, TimescaleDB, ClickHouse, Elasticsearch, Redis.
- Prometheus/Grafana, Kubernetes, HA/database replication.
- permanent PCAP storage or raw packet persistence.
- database row per captured packet.
- generic SIEM/enterprise IDS product scope.

## Architecture Rules

- Reject irrelevant traffic in the kernel before userspace.
- Do not capture all traffic and filter it later in Rust.
- Keep Suricata out of the Rust analyzer hot path.
- Rust analyzer is the source of truth for reconstructed sessions.
- Persist sessions, not packets.
- Store payload bytes in append-only segment files, not SQLite.
- Store direct payload location as `segment_id`, `segment_offset`, `record_length`.
- Run flag detection on reconstructed streams, not raw packets.
- For server replies containing flags, preserve enough metadata/pointers to inspect the triggering client/player payload.
- Keep ingest queues bounded and backpressure explicit.
- Prefer observable drops and bounded memory over unbounded buffering.
- Keep MVP protocol support to `raw_tcp`, `http`, `websocket`.

## Working Rules

- Read local context before changing files.
- Treat `docs/agent-decisions.md` as fixed unless the user explicitly changes direction.
- Do not silently replace fixed stack decisions.
- Use TODO/TBD for unresolved implementation details instead of inventing incompatible architecture.
- Keep changes scoped to the current task.
- Do not revert user changes unless explicitly requested.
- Update docs or agent memory when behavior, architecture, or project decisions change.
- Run relevant checks when the codebase has commands for them.

## Current Unknowns

The first implementation choices are recorded in `docs/agent-decisions.md`. Remaining unknowns include:

- exact WebSocket frame parser/representation.
- exact Suricata correlation mechanism.
- production policy for extremely long-lived TCP sessions.
- recovery and reconciliation behavior for truncated segment tails.
- handling policy for VLAN traffic, IPv6 extension headers and large reassembly gaps.
- exact representation for linking flag-containing replies to triggering client/player requests.
