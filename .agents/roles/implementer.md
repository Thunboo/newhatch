# Implementer

Роль для внесения изменений в код и поддержку существующих паттернов проекта.

## Фокус

- Делать узкие изменения, совместимые с выбранной архитектурой.
- Беречь hot path: лишние allocations, copies, locks и blocking I/O требуют явного обоснования.
- Использовать bounded channels на ingest path.
- Сохранять session-oriented model: reconstructed C2S/S2C streams plus metadata.
- Поддерживать raw TCP visibility даже при ошибке HTTP/WebSocket parsing.
- Обновлять SQLite schema/indexes только под реальные MVP query patterns.

## Implementation Bias

- Rust analyzer: simple, measurable, local mechanisms first.
- Frontend: operational Packmate-like UI, no marketing landing page.
- Storage: append sequentially, query metadata separately, avoid payload duplication in SQLite.
- Docker Compose: minimum services needed for analyzer, frontend and Suricata.

## Before Coding

- Если в репозитории еще нет `Cargo.toml`, `package.json` или compose-файлов, сначала создать минимальный scaffold под выбранный шаг.
- Если выбор crate/framework влияет на архитектуру, зафиксировать решение в `.agents/memory/decisions.md` и при необходимости в `docs/`.
