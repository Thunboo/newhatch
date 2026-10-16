# AGENTS.md

Правила работы с newhatch / «Нюхач».

## Контекст

Общаться с пользователем по-русски, коротко и по делу; проектную документацию вести на английском.
Перед существенными изменениями прочитать PROJECT.md и docs/agent-decisions.md.
Остальные документы открывать только по области текущей задачи:

| Область | Источник |
| --- | --- |
| Запуск, .env, local/split deployment | README.md и .env.example |
| Capture, reassembly, concurrency, collector, Suricata | docs/architecture.md |
| Atomic HTTP sessions (#28) | docs/http-sessions.md |
| SQLite, segments, retention, recovery | docs/storage.md |
| UI, live feed, payload display, individual HTTP export | docs/mvp.md |
| Correlation, chain cards, chain API/export preparation | docs/session-chains.md |
| Python replay, destination override, response output, cookies | docs/python-replay.md |
| Search chips, application lifecycle, payload list API/limits | docs/session-filtering-v2.md |
| Known/Unsorted buckets, ordered HTTP rules, ownership and labels | docs/session-buckets.md |
| Login, nginx ingress, sessions, team access | docs/authentication.md |
| Code map | crates/analyzer/LOGIC.md |
| Нерешённые вопросы | .agents/memory/open-questions.md |
| Незавершённые задачи и проверки на целевом хосте | .agents/tasks/backlog.md |
| Короткий контекст и команды разработки | .codex/context.md и .codex/commands.md |

## Работа

- Сохранять фиксированные решения из docs/agent-decisions.md; менять их только по явному решению пользователя.
- Не выдумывать ответы на нерешённые вопросы. Уточнять только то, что нужно для текущей задачи; иначе оставлять TODO/TBD.
- Держать изменения в рамках задачи, не откатывать пользовательские правки.
- Комментарии в GitHub пользователь публикует сам. Просьбы «фиксируй комментом» и «подготовь комментарий» означают дать текст в чате, а не отправить его в GitHub.
- Если задача предполагает изменения, выполнять её, а не останавливаться на предложении плана.
- Проверять изменённое поведение подходящими доступными проверками; сообщать о существенных пробелах проверки.
- Никогда не запускать Docker, Docker Compose, не обращаться к Docker daemon и не запускать, останавливать или перенастраивать OrbStack: это может оборвать VPN и сессию. Если нужна Docker-проверка, дать точные команды пользователю и дождаться его результата.

## Документация

- У каждой темы один подробный источник из таблицы выше. При изменении поведения обновлять его; в обзорах оставлять короткую ссылку.
- Не копировать контракты функций в PROJECT.md, AGENTS.md и служебные файлы. Исключение — короткие предупреждения, нужные для безопасного запуска или работы.
- Сначала исправлять существующий текст; добавлять новый раздел только для новой самостоятельной темы.
- .codex/context.md держать коротким (до 250 слов): ориентация и ссылки, без пересказа спецификаций.
- .agents/ хранит только нерешённые вопросы и незавершённые задачи. После выполнения удалять задачу из списка; историю ведет пользователь в GitHub issues.
- Не сохранять журналы завершённых работ, старые результаты тестов, промежуточные планы, даты согласований и названия завершённых веток.
- В test/*/README.md хранить команды, охват и ограничения проверок, а не историю запусков.
- При перемещении или удалении документа обновлять ссылки и проверять их, включая ссылки на разделы.
