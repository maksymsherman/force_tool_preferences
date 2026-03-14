# force_uv

A policy tool that redirects coding agents away from bare `python`/`pip` commands toward [`uv`](https://docs.astral.sh/uv/).

## What gets blocked

- `python`, `python3`, `pip`, `pip3` (bare invocations)
- `uv init` in existing projects (suggests safe alternatives)

## Quick install

```sh
curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_uv/main/install.sh | bash
```

This builds the binary, installs it to `~/.local/bin/`, and auto-configures hooks for any detected agents (Claude Code, Gemini CLI). Requires Rust/Cargo.

For Codex, copy the context file into your project:

```sh
curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_uv/main/AGENTS.md -o AGENTS.md
```

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

Install as a global skill (triggers automatically on Python/pip tasks):

```sh
git clone https://github.com/maksymsherman/force_uv.git ~/.codex/skills/force-uv
```

Or copy [`AGENTS.md`](./AGENTS.md) into a specific project root for per-project enforcement.

## Verify

```sh
enforce-uv-command --command 'uv run pytest'    # exits 0
enforce-uv-command --command 'pip install foo'   # exits 2, prints block message
```

## License

MIT
