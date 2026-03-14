# force_uv

A policy tool that redirects coding agents away from bare `python`/`pip` commands toward [`uv`](https://docs.astral.sh/uv/).

## What gets blocked

- `python`, `python3`, `pip`, `pip3` (bare invocations)
- `uv init` in existing projects (suggests safe alternatives)

## Install

### Claude Code (hook)

Build the binary and add a `PreToolUse` hook:

```sh
git clone https://github.com/maksymsherman/force_uv.git
cd force_uv
cargo build --release
cp target/release/enforce-uv-command ~/.local/bin/
```

Add to `~/.claude/settings.json`:

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          {
            "type": "command",
            "command": "enforce-uv-command --claude-hook-json"
          }
        ]
      }
    ]
  }
}
```

Restart Claude Code.

### Codex

Copy [`AGENTS.md`](./AGENTS.md) into your project root.

### Gemini CLI

Copy [`GEMINI.md`](./GEMINI.md) into your project root.

## Verify

```sh
# should exit 0
enforce-uv-command --command 'uv run pytest'

# should exit 2 with a block message
enforce-uv-command --command 'pip install requests'
```

## License

MIT
