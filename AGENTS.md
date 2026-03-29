# force_tool_preferences

Combined shell-hook enforcement for preferred CLI tools in agent workflows.

## Current rules

- Never run `grep`, `egrep`, or `fgrep`; use `rg` instead.
- Never run bare `python`, `python3`, `pip`, or `pip3`; use `uv` instead.
- Never run `uv init` in an existing repo unless the user explicitly asks for project creation or conversion.
- Agents may temporarily lower `perf_event_paranoid` for profiling or test runs that require `perf`, but they must record the original value first and restore it immediately after the script finishes, even if the run fails.

## Command mapping

- `grep -rn pattern .` -> `rg pattern .`
- `grep -F literal file.txt` -> `rg -F literal file.txt`
- `python script.py` -> `uv run python script.py`
- `python -m pytest` -> `uv run python -m pytest`
- `pip install requests` -> `uv add requests` or `uv pip install requests`
- `pip uninstall requests` -> `uv remove requests` or `uv pip uninstall requests`

## Design notes

- One combined PreToolUse hook replaces per-tool hooks to avoid repeated fork/exec overhead.
- The Rust binary accepts `--claude-hook-json`, `--codex-hook-json`, and `--gemini-hook-json`.
- The installer enables both rule families by default and can scope installation to just `rg` or just `uv`.
- Command evaluation is rule-family driven so future additions like `node`/`npm`/`npx` -> `bun` can slot into the same dispatcher.
- Rewrites should be exact when confidence is high and blocked for manual translation when semantics are unclear.
