# Bugfix Workflow

Шаблон работы над исправлением ошибки.

## Шаги

1. Воспроизвести проблему или зафиксировать минимальное описание, если live capture недоступен.
2. Определить слой: capture/BPF, flow tracking, reassembly, protocol parsing, flag detection, storage, API, UI, compose/runtime.
3. Проверить, не конфликтует ли исправление с `docs/agent-decisions.md`.
4. Исправить самое узкое место.
5. Добавить regression test или ручную проверку, подходящую для текущего scaffold.
6. Обновить docs/agent memory, если баг выявил важное архитектурное правило.

## Особое Внимание

- Flags split across TCP segments.
- Out-of-order, duplicate and retransmitted TCP data.
- Partial/incomplete sessions and idle timeouts.
- Segment rotation and SQLite metadata consistency.
- UI filters returning unbounded result sets.
