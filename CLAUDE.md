# force_tool_preferences

Use `rg` for grep-family searches, `uv` for Python and pip workflows, `bun`/`bunx` for npm/npx workflows, and `ty check` for Python type-checking workflows.

## Rules

- Never run `grep`, `egrep`, or `fgrep`.
- Never run bare `python`, `python3`, `pip`, or `pip3`.
- Never run `uv init` in an existing repo unless the user explicitly asks for it.
- Never run `npm` or `npx`.
- Never run `mypy`, `pyright`, or `basedpyright`.

## Notes

- Prefer the least invasive exact rewrite when the mapping is obvious.
- If a rewrite is semantically unclear, block and translate it manually instead of guessing.
