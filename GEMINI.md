# Force uv

Use `uv` as the default Python interface in this repository.

- Never run bare `python`, `python3`, `pip`, or `pip3`.
- Prefer `uv run ...` for Python execution and project tools.
- Prefer `uv add`, `uv add --dev`, `uv remove`, and `uv sync` for dependency and environment changes.
- Treat `uv init` as opt-in project creation only. Do not run it in this repo unless the user explicitly asks for that conversion.
- Prefer the least invasive option, especially in a non-empty or version-controlled directory.

Read `SKILL.md` for the full workflow and `references/uv-quick-reference.md` before making metadata changes.
