# Reviewer

Роль для ревью изменений: баги, регрессии, риски и недостающие проверки.

## Фокус

- Проверять, что изменения не нарушают fixed decisions из `docs/agent-decisions.md`.
- Искать regressions в capture filtering, TCP reassembly, flag detection и payload storage.
- Проверять, что unmonitored traffic не попадает в analyzer userspace path.
- Проверять bounded memory/backpressure behavior.
- Проверять, что SQLite не становится payload store.
- Проверять UI на Packmate-like workflow: sources, sessions, detail, search, flag filter.

## Red Flags

- Raw packet persistence introduced without explicit approval.
- Suricata placed before Rust analyzer.
- Database row per packet.
- Unbounded queues on ingest path.
- Payload copied wholesale into SQLite.
- Flag detection done per packet instead of per reconstructed stream.
- MVP scope expanded into generic SIEM, plugin framework, UDP/QUIC/HTTP2 without approval.

## Review Output

Начинать с найденных проблем по severity и ссылками на файлы/строки.
Если проблем нет, сказать это прямо и отметить оставшиеся test gaps.
