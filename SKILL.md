---
name: force-uv
description: Enforce uv-first Python workflows in an existing project. Use when an agent would otherwise run `python`, `python3`, `pip`, or `pip3`, create or update Python environments, install or remove dependencies, run project scripts or tests, or explain how to work in a uv-managed repository. This skill is especially important in non-empty or version-controlled directories where `uv init` would be risky or unnecessary.
---

# Force uv

## Overview

Use `uv` as the default interface for Python execution, dependency management, and one-off tooling. Replace bare Python and pip commands with the least invasive `uv` equivalent, and avoid writing project metadata unless the user explicitly wants that change.

## Core Rules

- Never run bare `python`, `python3`, `pip`, or `pip3` in this project.
- Prefer `uv run ...` for scripts, modules, tests, and ad hoc Python execution.
- Prefer `uv add` for runtime dependencies and `uv add --dev` for dev-only dependencies.
- Prefer `uv remove` to uninstall project dependencies.
- Prefer `uv sync` to realize an existing `pyproject.toml` and `uv.lock`.
- Prefer `uv run --with PKG ...` for one-off tools that should not be added to project metadata.
- Use `uvx` only for isolated CLI tools when project dependencies are irrelevant.
- Never run `uv init` inside an existing repo or non-empty directory unless the user explicitly asks to create or convert a uv project there.
- Never rewrite `pyproject.toml`, `uv.lock`, `.python-version`, or virtualenv settings just to satisfy a one-off command.

## Command Mapping

Translate common requests like this:

- `python script.py` -> `uv run python script.py`
- `python -m pytest` -> `uv run pytest`
- `python -m http.server` -> `uv run python -m http.server`
- `python -c "..."` -> `uv run python -c "..."`
- `pip install requests` -> `uv add requests`
- `pip install pytest ruff --dev` -> `uv add --dev pytest ruff`
- `pip uninstall requests` -> `uv remove requests`

If the user explicitly wants pip-compatible behavior, use `uv pip ...`, but prefer project-aware `uv` commands first.

## Decision Workflow

1. Inspect the repo before changing anything. Look for `pyproject.toml`, `uv.lock`, `.python-version`, `.venv`, and existing tooling conventions.
2. If the project is already uv-managed, stay inside that workflow with `uv run`, `uv add`, `uv remove`, and `uv sync`.
3. If the project is not uv-managed and the user only needs a one-off command, use `uv run --with` or `uvx` instead of creating new project files.
4. Consider `uv init` only when the user explicitly asks to create or convert a project, and only after confirming the target directory is safe to modify.
5. When the right replacement is not obvious, explain the chosen `uv` equivalent before running it.

## Safety Rules

- Treat `uv init` as opt-in project creation, not a default setup step.
- Prefer read-only discovery before dependency changes.
- Keep changes minimal: do not add dependencies, lockfiles, or Python version pins unless they solve the user's stated task.
- If a command would change project metadata, verify that the benefit is worth the write.
- If a repo already uses another environment manager, acknowledge the conflict instead of silently converting it.

## Resources

- Use Astral's live documentation as the source of truth when command syntax, defaults, or guidance may have changed. Start with the agent-friendly index at <https://docs.astral.sh/uv/llms.txt>, then use the main docs at <https://docs.astral.sh/uv/> as needed.
- Read `references/uv-quick-reference.md` for concise `uv` guidance and safe command selection.
- Read `references/agent-integration.md` when wiring this policy into Codex, Claude Code, or Gemini.
- Use `scripts/enforce-uv-command.sh` when an agent supports shell hooks or command interception.
