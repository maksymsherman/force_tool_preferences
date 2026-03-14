# Force uv

This repository enforces uv-first Python workflows.

- Do not run `python`, `python3`, `pip`, or `pip3` directly.
- Replace script and module execution with `uv run ...`.
- Replace dependency installs with `uv add ...` or `uv add --dev ...`.
- Replace dependency removals with `uv remove ...`.
- Do not run `uv init` in this repository unless the user explicitly asks to create or convert a uv project here.
- Keep project changes minimal; prefer `uv run --with ...` or `uvx ...` for one-off tooling.

Read `SKILL.md` for the full workflow. Read `references/uv-quick-reference.md` before choosing a `uv` command that writes project metadata.
