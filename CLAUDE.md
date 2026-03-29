# force_tool_preferences

Use `rg` for grep-family searches and `uv` for Python and pip workflows.

## Rules

- Never run `grep`, `egrep`, or `fgrep`.
- Never run bare `python`, `python3`, `pip`, or `pip3`.
- Never run `uv init` in an existing repo unless the user explicitly asks for it.

## Notes

- Prefer the least invasive exact rewrite when the mapping is obvious.
- If a rewrite is semantically unclear, block and translate it manually instead of guessing.
