# .codex

Local context and reference material for Codex work in this repository.

## Назначение

This folder restores working context between sessions. Product direction remains in `PROJECT.md`, startup instructions in `README.md`, and technical decisions in `docs/`.

## Structure

- `context.md` - compact project state.
- `commands.md` - development and validation commands.
- `preferences.md` - collaboration and implementation preferences.
- `prompts/` - reusable project prompts.
- `checklists/` - before-work and before-finish checks.

## Read First

- Start with `AGENTS.md`; it owns canonical-context routing.
- Use `.codex/context.md` for a fast state summary, then open only the focused canonical documents required by `AGENTS.md` and the current task.
- Use `.agents/memory/documentation-sync.md` before saving refreshed context.
