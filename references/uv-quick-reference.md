# uv Quick Reference

Use this file when deciding which `uv` command is the least invasive and safest choice.

## Default replacements

- Run a script: `uv run python path/to/script.py`
- Run a module: `uv run python -m package.module`
- Run tests: `uv run pytest`
- Add a runtime dependency: `uv add PACKAGE`
- Add a dev dependency: `uv add --dev PACKAGE`
- Remove a dependency: `uv remove PACKAGE`
- Materialize the environment from project metadata: `uv sync`
- Run a one-off tool without editing the project: `uv run --with PACKAGE COMMAND`
- Run an isolated CLI tool: `uvx TOOL`

## Safety rules

- Do not start with `uv init` in an existing repo. `uv init` is for creating or converting a project, so treat it as a deliberate write operation.
- Inspect the repo before adding dependencies or syncing. Existing `pyproject.toml`, `uv.lock`, `.python-version`, and `.venv` files should shape the choice.
- Prefer `uv run` over changing project files when the task is only to execute something once.
- Prefer `uv add` and `uv remove` over `uv pip install` in normal project workflows.
- Use `uv pip ...` only when the user explicitly wants pip-compatible behavior or the project context makes that the least surprising interface.

## Official Astral docs

- Start with the LLM index when available: <https://docs.astral.sh/uv/llms.txt>
- Overview: <https://docs.astral.sh/uv/>
- Running commands and scripts: <https://docs.astral.sh/uv/guides/scripts/>
- Project management: <https://docs.astral.sh/uv/guides/projects/>
- `uv add`: <https://docs.astral.sh/uv/reference/cli/#uv-add>
- `uv run`: <https://docs.astral.sh/uv/reference/cli/#uv-run>
- `uv init`: <https://docs.astral.sh/uv/reference/cli/#uv-init>
