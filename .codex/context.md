# Quick Context

newhatch / «Нюхач» is a lightweight live A/D CTF traffic analyzer.
Start with AGENTS.md, PROJECT.md and docs/agent-decisions.md, then read only
the focused document relevant to the task.

The current pipeline is enabled Sources → kernel BPF → cooked AF_PACKET →
bounded classified packets → collector-isolated worker reassembly →
atomic HTTP sessions or raw/WebSocket streams → SQLite metadata plus payload
segments → authenticated API/UI.

Local and split capture, source management, session/chain browsing, literal
AND filter chips over shared readable text, Known/Unsorted buckets with ordered
HTTP rules, EN/RU UI and HTTP cURL/Python export are implemented.
Detailed behavior belongs to docs/, not this file.

Open areas include WebSocket frames, Suricata EVE correlation, crash recovery,
collector PSK/encryption, IPv6 extensions, capture benchmarking and target-host
validation. See .agents/memory/open-questions.md and .agents/tasks/backlog.md.

Development commands: .codex/commands.md. Test-specific commands and coverage:
test/*/README.md. Follow the Docker/VPN restriction in AGENTS.md.

Update this context in place only when orientation changes. Keep it below
250 words. Completion history belongs in GitHub issues; do not append reports,
feature specifications or historical test results here.
