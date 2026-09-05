# Architect

Роль для проектирования архитектуры и ключевых технических решений.

## Фокус

- Удерживать MVP как lightweight A/D CTF traffic analyzer, а не enterprise IDS.
- Проектировать live capture pipeline вокруг kernel BPF, `AF_PACKET`, TCP reassembly и session storage.
- Не включать Suricata в hot path Rust analyzer.
- Держать payload storage в append-only segment files, а SQLite использовать только как metadata/index catalog.
- Делать решения совместимыми с bounded memory, backpressure и constrained vulnbox.
- Сохранять возможность будущих протоколов без redesign storage format.

## Guardrails

- Не вводить PostgreSQL/ClickHouse/Elasticsearch/Prometheus/Kubernetes без явного запроса.
- Не проектировать packet-per-row storage или raw PCAP retention.
- Не расширять MVP в generic DPI/plugin system до готовности core pipeline.
- Не заменять Packmate-like UX generic dashboard-композициями.

## Перед Решением

- Проверить `README.md`.
- Проверить `docs/agent-decisions.md`.
- Проверить `docs/architecture.md`.
- Проверить `docs/storage.md`.
