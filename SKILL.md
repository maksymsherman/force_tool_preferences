---
name: force-uv
description: >
  Enforce uv-first Python workflows. Triggers when the task involves python,
  python3, pip, pip3, pip install, venv, virtualenv, pyenv, conda,
  poetry, pipenv, requirements.txt, setup.py, or any Python dependency
  management. Replaces bare Python and pip commands with uv equivalents.
---

# Force uv

Use `uv` as the default interface for Python execution and dependency management.

## Rules

- Never run bare `python`, `python3`, `pip`, or `pip3`.
- Prefer `uv run ...` for scripts, modules, tests, and ad hoc Python execution.
- Prefer `uv add` / `uv add --dev` for dependencies and `uv remove` to uninstall.
- Prefer `uv sync` to realize an existing `pyproject.toml` and `uv.lock`.
- Prefer `uv run --with PKG ...` for one-off tools that should not touch project metadata.
- Use `uvx` only for isolated CLI tools when project dependencies are irrelevant.
- Never run `uv init` in an existing repo unless the user explicitly asks for project creation or conversion. If needed, use `uv init --no-readme --no-workspace`.
- Never rewrite `pyproject.toml`, `uv.lock`, `.python-version`, or virtualenv settings just to satisfy a one-off command.

## Command mapping

- `python script.py` -> `uv run python script.py`
- `python -m pytest` -> `uv run pytest`
- `python -m http.server` -> `uv run python -m http.server`
- `python -c "..."` -> `uv run python -c "..."`
- `pip install requests` -> `uv add requests`
- `pip install pytest ruff --dev` -> `uv add --dev pytest ruff`
- `pip uninstall requests` -> `uv remove requests`

## Workflow

1. Inspect the repo first. Look for `pyproject.toml`, `uv.lock`, `.python-version`, `.venv`.
2. If the project is uv-managed, use `uv run`, `uv add`, `uv remove`, `uv sync`.
3. If not uv-managed and the task is one-off, use `uv run --with` or `uvx`.
4. Only consider `uv init` when the user explicitly asks, after confirming the directory is safe.
