# Review Workflow

Шаблон ревью изменений.

## Шаги

1. Прочитать измененные файлы и релевантные docs.
2. Проверить соответствие fixed decisions.
3. Проверить behavioral contracts по слоям: capture, reassembly, parsing, flags, storage, API, UI.
4. Найти bugs, regressions, security/performance risks and missing tests.
5. Сформулировать замечания по severity с file/line references.
6. Отдельно отметить, если проблем не найдено.

## Project-Specific Checks

- BPF filter applies before userspace processing.
- Suricata remains parallel enrichment.
- SQLite stores metadata/indexes only.
- Segment records remain directly addressable by `segment_id`, `byte_offset`, `record_length`.
- Flag detection runs after stream reassembly.
- MVP protocol scope stays `raw_tcp`, `http`, `websocket`.
