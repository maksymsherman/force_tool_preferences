# force_tool_preferences

Use preferred CLI tools consistently in this repo.

## Rules

- Use `rg` instead of `grep`, `egrep`, or `fgrep`.
- Use `uv` instead of bare `python`, `python3`, `pip`, or `pip3`.
- Do not run `uv init` in an existing repo unless the user explicitly asks for project creation or conversion.

## Examples

- `grep -rn TODO .` -> `rg TODO .`
- `python -m pytest` -> `uv run python -m pytest`
- `pip install requests` -> `uv add requests` or `uv pip install requests`
