# force_tool_preferences

```text
grep / python / pip
         |
         v
+----------------------------------+
| enforce-tool-preferences-command |
+----------------------------------+
     | allow              | block
     v                    v
  continue        suggest rg / uv rewrite
```

Single-process shell-hook enforcement for preferred CLI tools in Codex, Claude Code, and Gemini.

Today it combines:

- `grep` / `egrep` / `fgrep` -> `rg`
- `python` / `python3` / `pip` / `pip3` -> `uv`

The point of this repo is consolidation. Instead of paying shell startup and process startup costs for one hook per rule family, you pay them once and evaluate all active tool-preference rules in one Rust binary.

## Quick install

```sh
curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_tool_preferences/main/install.sh | bash
```

The installer builds `enforce-tool-preferences-command`, installs it to `~/.local/bin/`, and configures hooks for Claude Code, Gemini CLI, and Codex when their config files are present.

## What it does

`force_tool_preferences` sits in front of shell execution and turns tool drift into an explicit decision point:

- allowed commands pass through
- blocked commands exit non-zero in direct CLI mode
- Codex and Claude hook mode emit JSON block responses
- exact rewrites are suggested when the mapping is high confidence
- ambiguous cases are blocked without guessing

Current examples:

| Input | Result |
|---|---|
| `grep -rn TODO .` | suggest `rg TODO .` |
| `fgrep literal README.md` | suggest `rg -F literal README.md` |
| `python -m pytest` | suggest `uv run python -m pytest` |
| `pip install requests` | suggest `uv add requests` and `uv pip install requests` |
| `uv init` | blocked with safer guidance |
| `grep -s TODO file.txt` | blocked for manual translation |

## CLI

```sh
enforce-tool-preferences-command --command 'grep -rn TODO .'
enforce-tool-preferences-command --command 'python -m pytest'
printf '%s' '{"tool_input":{"command":"pip install requests"}}' \
  | enforce-tool-preferences-command --codex-hook-json
enforce-tool-preferences-command --benchmark-command 'grep -rn TODO .' --iterations 100000
```

Behavior:

- allowed commands exit `0`
- blocked direct CLI commands exit `2`
- invalid usage exits `1`

## Installation

### Installer

```sh
curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_tool_preferences/main/install.sh | bash
```

Useful variants:

```sh
curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_tool_preferences/main/install.sh \
  | bash -s -- --dry-run

curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_tool_preferences/main/install.sh \
  | bash -s -- --check-binary-hash
```

The installer expects:

- `cargo`
- `rg`
- `uv`

### Build from source

```sh
git clone https://github.com/maksymsherman/force_tool_preferences.git
cd force_tool_preferences
cargo build --release
mkdir -p ~/.local/bin
cp target/release/enforce-tool-preferences-command ~/.local/bin/
```

Then configure hooks:

```sh
mkdir -p ~/.codex
[ -f ~/.codex/hooks.json ] || printf '{}\n' > ~/.codex/hooks.json
./target/release/enforce-tool-preferences-command \
  --configure-codex-hook ~/.codex/hooks.json ~/.local/bin/enforce-tool-preferences-command
```

For Codex, also ensure `codex_hooks = true` under `[features]` in `~/.codex/config.toml`.

## Design

- One binary owns the hook boundary and evaluates all current rule families.
- The command evaluator is centralized around an internal rule dispatcher, so future rules like `npm` / `npx` / `node` -> `bun` can be added without adding more hook processes.
- The grep-family translation remains conservative: safe rewrites are suggested, unsafe flags are blocked.
- The Python-family translation preserves wrapper commands and distinguishes likely project dependency updates from pip-style environment mutation.

## Provenance

This repo was combined from the separate `force_rg` and `force_uv` projects. In this repository, their histories are preserved on import branches so the combined implementation on `main` can be kept clean.
