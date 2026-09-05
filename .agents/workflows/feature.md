# Feature Workflow

Шаблон работы над новой функцией.

## Шаги

1. Прочитать `README.md`, `docs/agent-decisions.md` и релевантный документ из `docs/`.
2. Сопоставить feature с MVP scope: sources, sessions, detail, search, flag UX, Suricata enrichment или storage/capture foundation.
3. Найти затронутые boundaries: capture, flow/reassembly, protocol parsing, flag scanning, storage, API, UI.
4. Проверить, что feature не вводит запрещенную инфраструктуру или raw packet persistence.
5. Внести минимальные связанные изменения.
6. Добавить проверки на поведение и regressions.
7. Обновить `docs/` или `.agents/memory/decisions.md`, если появилось новое проектное решение.

## Acceptance Bias

- UI remains responsive and paginated/cursor-based for large result sets.
- Ingest path remains bounded.
- Flag metadata is stored in SQLite and does not require UI rescanning.
- Payload retrieval uses segment offsets, not scanning whole files.
