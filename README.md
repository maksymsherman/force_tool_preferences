# force_uv

A policy tool that redirects coding agents away from bare `python`/`pip` commands toward [`uv`](https://docs.astral.sh/uv/), with confidence-graded replacement suggestions.

## What gets blocked

- `python`, `python3`, `pip`, `pip3` (bare invocations)
- `uv init` in existing projects (suggests safe alternatives)

## What gets suggested

- Exact rewrites for high-confidence Python cases:
  - `python script.py` -> `uv run python script.py`
  - `python -m pytest` -> `uv run python -m pytest`
  - `sudo -u root python script.py` -> `sudo -u root uv run python script.py`
- Exact `uv pip` rewrites for pip-compatible commands such as `pip list`
- Multiple likely alternatives for ambiguous commands such as `pip install ...`:
  - `uv add ...` when the intent is to modify project dependencies
  - `uv pip install ...` when the intent is pip-style environment management

The tool stays conservative: if a rewrite is not high confidence, it blocks and suggests the smallest safe set of likely `uv` alternatives instead of pretending there is one canonical conversion.

## Quick install

```sh
curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_uv/main/install.sh | bash
```

This builds the binary, installs it to `~/.local/bin/`, auto-configures hooks for any detected agents (Claude Code, Gemini CLI), and installs the Codex skill at `~/.codex/skills/force-uv` even if Codex has not been launched yet. Requires Rust/Cargo and `uv`.

## Manual install

### Claude Code

```sh
git clone https://github.com/maksymsherman/force_uv.git
cd force_uv && cargo build --release
cp target/release/enforce-uv-command ~/.local/bin/
```

Add to `~/.claude/settings.json`:

```json
{
  "hooks": {
    "PreToolUse": [{
      "matcher": "Bash",
      "hooks": [{"type": "command", "command": "enforce-uv-command --claude-hook-json"}]
    }]
  }
}
```

### Gemini CLI

Same binary, different hook. Add to `~/.gemini/settings.json`:

```json
{
  "hooks": {
    "BeforeTool": [{
      "matcher": "run_shell_command",
      "hooks": [{"type": "command", "command": "enforce-uv-command --gemini-hook-json"}]
    }]
  }
}
```

### Codex

Install as a global skill (recommended; triggers automatically on Python/pip tasks):

```sh
git clone https://github.com/maksymsherman/force_uv.git ~/.codex/skills/force-uv
```

Project-local fallback only:

```sh
curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_uv/main/AGENTS.md -o AGENTS.md
```

## Verify

```sh
enforce-uv-command --command 'uv run pytest'    # exits 0
enforce-uv-command --command 'python -m pytest'  # exits 2, prints exact uv rewrite
enforce-uv-command --command 'pip install foo'   # exits 2, prints likely uv alternatives
```

## License

MIT
