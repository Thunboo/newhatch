# Documentation Synchronization Map

Use this map when behavior, architecture, configuration, verification status or open questions change. It records intentional repetition so future agents can update affected summaries without rescanning every Markdown file.

## Audience Boundary

- Files outside `.agents/` and `.codex/` are player/developer-facing project documentation, except the root `AGENTS.md`, which is agent policy and context routing.
- `.agents/` and `.codex/` are agent memory, compressed context, workflows and command references.
- Public/project docs remain authoritative for user-visible behavior and architecture. Agent memory may summarize them but must not silently diverge.

## Update Rule

When saving or refreshing context:

1. Update the authoritative project document for the changed subject.
2. Use the topic map below to update every affected compact summary.
3. Resolve discovered discrepancies in the same change; if the correct answer is unknown, record it in `.agents/memory/open-questions.md` instead of guessing.
4. Update `.agents/tasks/backlog.md` when an unresolved item is added, removed or materially redefined.
5. Do not rewrite focused test runbooks or operator instructions unless their actual commands/behavior changed.

## Task Lifecycle

- `tasks/backlog.md` contains only unfinished, not-yet-active tasks (`[ ]`).
- `tasks/active.md` contains only work currently being implemented plus concrete operational follow-ups.
- `tasks/done.md` is the dated append-only completion ledger.
- A detailed standalone task file may exist while a complex task is active. After completion, move durable behavior/decisions into authoritative docs and memory, add a concise `done.md` entry and remove the standalone task file.
- Normal movement is `backlog -> active -> done`; do not keep the same completed task in multiple registries.

## Topic Map

### Product Scope And Current Implementation

- Authoritative: `PROJECT.md`.
- Mirrors: `.agents/memory/project.md`, `.codex/context.md`.
- Task state: `.agents/tasks/active.md`, `.agents/tasks/done.md`, `.agents/tasks/backlog.md`.

### Fixed Architecture And Prohibitions

- Authoritative: `docs/agent-decisions.md`.
- Mirrors: `AGENTS.md`, `.agents/memory/decisions.md`.
- Routing pointer: `.codex/prompts/project.md`.

### Capture, Reassembly And Analyzer Code Map

- Authoritative design: `docs/architecture.md`.
- Code-oriented map: `crates/analyzer/LOGIC.md`.
- Summaries: `PROJECT.md`, `.agents/memory/project.md`, `.agents/memory/decisions.md`, `.codex/context.md`.

### Collector / Analyzer Split

- Operator workflow: `README.md`.
- Architecture/contract: `docs/architecture.md`, `docs/agent-decisions.md`.
- Test workflow: `test/collector_split/README.md`.
- Mirrors: `PROJECT.md`, `.agents/memory/project.md`, `.agents/memory/decisions.md`, `.codex/context.md`.

### Authentication And Team Access

- Authoritative: `docs/authentication.md`.
- Operator summary: `README.md`.
- Test workflow: `test/auth/README.md`.
- Mirrors: `PROJECT.md`, `docs/architecture.md`, `docs/agent-decisions.md`, `.agents/memory/project.md`, `.agents/memory/decisions.md`, `.codex/context.md`.

### Storage, Segment Format And Retention

- Authoritative: `docs/storage.md`.
- Fixed policy: `docs/agent-decisions.md`.
- UX/config references: `docs/mvp.md`, `.codex/commands.md`.
- Mirrors: `.agents/memory/project.md`, `.agents/memory/decisions.md`, `.codex/context.md`.

### UI Behavior And MVP Scope

- Authoritative: `docs/mvp.md`.
- Operator-visible live behavior when relevant: `README.md`.
- Mirrors: `.agents/memory/project.md`, `.agents/memory/decisions.md`, `.codex/context.md`.

### Runtime Configuration

- Runtime defaults: `.env.example`, `compose.yaml`, analyzer/collector config code.
- Operator workflow: `README.md`, `docs/authentication.md`.
- Reference lists: `docs/mvp.md`, `.codex/commands.md`.

### Suricata Status

- Architecture/status: `docs/architecture.md`.
- Product/operator summaries: `PROJECT.md`, `README.md`, `docs/mvp.md`, `docs/agent-decisions.md`.
- Agent mirrors: `.agents/memory/project.md`, `.agents/memory/glossary.md`, `.codex/context.md`, `.codex/commands.md`.
- Planned work: `.agents/tasks/backlog.md`, `.agents/memory/open-questions.md`.

### Open Questions

- Primary list: `.agents/memory/open-questions.md`.
- Mirrors by purpose: `AGENTS.md` for high-level unknowns, `docs/agent-decisions.md` for unresolved architectural decisions, `.agents/tasks/backlog.md` for actionable work.

### Verification Status

- Compact current record: `.agents/memory/project.md`, `.codex/context.md`.
- Task/operational state: `.agents/tasks/active.md`, `.agents/tasks/done.md`.
- Focused commands and coverage: `.codex/commands.md`, relevant `test/*/README.md`.
