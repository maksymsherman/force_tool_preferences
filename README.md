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

Quick install:

```sh
curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_tool_preferences/main/install.sh | bash
```

By default, the installer enables both rule families:

- `grep` / `egrep` / `fgrep` -> `rg`
- `python` / `python3` / `pip` / `pip3` -> `uv`

## TL;DR

### The Problem

Prompt-only tool preferences drift. Agents fall back to `grep`, `python`, or `pip`, and each separate hook adds its own process startup cost.

### The Solution

`force_tool_preferences` consolidates those checks into one Rust binary and one hook process. It blocks disallowed commands, suggests the least invasive rewrite when confidence is high, and lets you install either both rule families or only the subset you want.

### Why Use It?

| Capability | What it does |
|---|---|
| One hook process | Replaces separate hook binaries for `rg` and `uv` enforcement |
| Exact rewrites when obvious | `grep -rn TODO .` -> `rg TODO .`, `python -m pytest` -> `uv run python -m pytest` |
| Honest ambiguity handling | `pip install requests` suggests both `uv add` and `uv pip install` |
| Selective installation | Default is both rules, but `--only-rg` and `--only-uv` are supported |
| Rule-aware hook updates | Reinstalling with a different selection updates the existing hook instead of appending duplicates |

## Quick Example

```sh
# Default install: enable both rg and uv enforcement
curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_tool_preferences/main/install.sh \
  | bash

# Install only grep-family -> rg enforcement
curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_tool_preferences/main/install.sh \
  | bash -s -- --only-rg

# Install only python/pip-family -> uv enforcement
curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_tool_preferences/main/install.sh \
  | bash -s -- --only-uv

# Direct CLI evaluation
enforce-tool-preferences-command --command 'grep -rn TODO .'
enforce-tool-preferences-command --command 'python -m pytest'

# Limit evaluation to one rule family
enforce-tool-preferences-command --command 'python -m pytest' --rules uv
enforce-tool-preferences-command --command 'grep -rn TODO .' --rules rg
```

Representative outcomes:

| Input | Result |
|---|---|
| `grep -rn TODO .` | suggest `rg TODO .` |
| `fgrep literal README.md` | suggest `rg -F literal README.md` |
| `python -m pytest` | suggest `uv run python -m pytest` |
| `pip install requests` | suggest `uv add requests` and `uv pip install requests` |
| `uv init` | blocked with safer guidance |
| `grep -s TODO file.txt` | blocked for manual translation |

## Installer Modes

The installer always installs the same binary, `enforce-tool-preferences-command`. What changes is which rule families the configured hooks enable.

| Mode | Command | Effect |
|---|---|---|
| Default | `install.sh` | Enable both `rg` and `uv` rules |
| `rg` only | `install.sh --only-rg` | Enable only grep-family -> `rg` enforcement |
| `uv` only | `install.sh --only-uv` | Enable only python/pip-family -> `uv` enforcement |
| Explicit | `install.sh --rules rg`, `--rules uv`, `--rules rg,uv` | Same behavior, but scriptable |

The installer:

1. Clones the repo.
2. Builds the release binary.
3. Installs or updates `~/.local/bin/enforce-tool-preferences-command`.
4. Configures Claude Code, Gemini CLI, and Codex hooks when their config paths exist.
5. Writes hook commands with the selected `--rules` value.

## CLI

### Evaluate a command

```sh
enforce-tool-preferences-command --command 'grep -rn TODO .'
enforce-tool-preferences-command --command 'python -m pytest'
enforce-tool-preferences-command --stdin-command
```

### Evaluate hook JSON

```sh
printf '%s' '{"tool_input":{"command":"pip install requests"}}' \
  | enforce-tool-preferences-command --codex-hook-json
```

### Restrict active rule families

```sh
enforce-tool-preferences-command --command 'grep -rn TODO .' --rules rg
enforce-tool-preferences-command --command 'python -m pytest' --rules uv
enforce-tool-preferences-command --command 'pip install requests' --rules rg,uv
```

### Benchmark matcher cost

```sh
enforce-tool-preferences-command \
  --benchmark-command 'grep -rn TODO .' \
  --iterations 100000 \
  --rules rg,uv
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

curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_tool_preferences/main/install.sh \
  | bash -s -- --only-rg

curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_tool_preferences/main/install.sh \
  | bash -s -- --only-uv

curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_tool_preferences/main/install.sh \
  | bash -s -- --rules rg,uv
```

Prerequisites depend on the selected rules:

- always: `cargo`
- `rg` rules selected: `rg`
- `uv` rules selected: `uv`

### Build from source

```sh
git clone https://github.com/maksymsherman/force_tool_preferences.git
cd force_tool_preferences
cargo build --release
mkdir -p ~/.local/bin
cp target/release/enforce-tool-preferences-command ~/.local/bin/
```

Then configure hooks manually:

```sh
mkdir -p ~/.codex
[ -f ~/.codex/hooks.json ] || printf '{}\n' > ~/.codex/hooks.json
./target/release/enforce-tool-preferences-command \
  --configure-codex-hook ~/.codex/hooks.json ~/.local/bin/enforce-tool-preferences-command \
  --rules rg,uv
```

For Codex, also ensure `codex_hooks = true` under `[features]` in `~/.codex/config.toml`.

## Design

- One binary owns the hook boundary and evaluates all current rule families.
- The command evaluator is centralized around an internal rule dispatcher, so future rules like `npm` / `npx` / `node` -> `bun` can be added without adding more hook processes.
- Hook configuration is rule-aware, so reinstalling with `--only-rg` or `--only-uv` updates the existing hook instead of appending duplicates.
- The grep-family translation remains conservative: safe rewrites are suggested, unsafe flags are blocked.
- The Python-family translation preserves wrapper commands and distinguishes likely project dependency updates from pip-style environment mutation.

## Comparison

| Approach | One hook process | Exact rewrites | Honest ambiguity handling | Selective rule install |
|---|---|---|---|---|
| `force_tool_preferences` | Yes | Yes | Yes | Yes |
| Separate hook repos | No | Yes | Yes | Yes, but with extra processes |
| `AGENTS.md` only | No | No | No | N/A |
| Shell aliases | Partially | No | No | Partially |

## Limitations

- The installer currently targets Unix-like environments and shell-hook workflows.
- Rule selection is hook-level, not per-directory or per-command-context.
- Future families like `node` / `npm` / `npx` -> `bun` are not implemented yet.

## Provenance

This repo was combined from the separate `force_rg` and `force_uv` projects. Their histories are preserved here on the `import/force-rg` and `import/force-uv` branches so the combined implementation on `main` can stay clean.
