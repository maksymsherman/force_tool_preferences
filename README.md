# force_tool_preferences

```text
grep / egrep / fgrep   python / pip / uv init   npm / npx   mypy / pyright / basedpyright
          \                      |                    |                    /
           \                     |                    |                   /
            +-------------------- force_tool_preferences ----------------+
                                       |
                                       v
                        enforce-tool-preferences-command
                                       |
                            allow or block with rewrite
```

<div align="center">

[![CI](https://github.com/maksymsherman/force_tool_preferences/actions/workflows/ci.yml/badge.svg)](https://github.com/maksymsherman/force_tool_preferences/actions/workflows/ci.yml)
[![Rust 2021](https://img.shields.io/badge/Rust-2021-orange?logo=rust)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)
[![ripgrep First](https://img.shields.io/badge/search-rg%20first-cc5500)](https://github.com/BurntSushi/ripgrep)
[![uv First](https://img.shields.io/badge/python-uv%20first-4B8BBE)](https://docs.astral.sh/uv/)
[![bun First](https://img.shields.io/badge/js-bun%20first-f4a261)](https://bun.sh/)
[![ty First](https://img.shields.io/badge/typecheck-ty%20first-2a9d8f)](https://docs.astral.sh/ty/)
[![Agents](https://img.shields.io/badge/Agents-Claude%20Code%20%7C%20Gemini%20%7C%20Codex-blue)](#installation)

One Rust hook that blocks the wrong CLI before it runs and tells agents what to use instead. `force_tool_preferences` combines the old `force_rg` and `force_uv` flows into one binary, one installer, and one shared rule catalog for Codex, Claude Code, and Gemini while now also enforcing `bun`-first JavaScript package workflows and `ty`-first Python type checking.

</div>

**Quick Install**

```sh
curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_tool_preferences/main/install.sh | bash
```

The default install builds `enforce-tool-preferences-command`, installs it to `~/.local/bin/`, enables every shipped rule family, and configures supported agent hooks where possible.

At a glance:

| Need | Use |
|---|---|
| Discover the shipped rule families | `--list-rules` |
| Install only `grep` -> `rg` enforcement | `--enable-rule rg` |
| Install only `python`/`pip` -> `uv` enforcement | `--enable-rule uv` |
| Install only `npm`/`npx` -> `bun` enforcement | `--enable-rule bun` |
| Install only `mypy`/`pyright` -> `ty` enforcement | `--enable-rule ty` |
| Keep a reproducible exact set in dotfiles or CI | `--rules rg,uv,bun,ty` |
| Check a command directly | `enforce-tool-preferences-command --command 'grep -rn TODO .'` |

## TL;DR

**The Problem:** Prompt-only tool preferences drift. Agents still fall back to `grep`, `python`, `pip`, `npm`, `npx`, `mypy`, or `pyright`, and separate per-tool hooks multiply startup cost, config churn, and places where policy can get out of sync.

**The Solution:** `force_tool_preferences` evaluates shell commands at the hook boundary and enforces all current rule families in one process:

- `grep` / `egrep` / `fgrep` -> `rg`
- bare `python*` / `pip*` -> `uv`
- `uv init` in an existing repo workflow -> blocked with safer guidance
- `npm` / `npx` -> `bun` / `bunx`
- `mypy` / `pyright` / `basedpyright` -> `ty check`

When the mapping is obvious, it suggests the least invasive exact rewrite. When semantics are unclear, it blocks and tells you to translate manually instead of guessing.

### Why Use `force_tool_preferences`?

| Feature | What it does | Why it matters |
|---|---|---|
| One hook process | Replaces separate `rg`, `uv`, `bun`, and future rule-family hooks with one evaluator | Reduces repeated subprocess overhead and config drift |
| Exact rewrites | `grep -rn TODO .` becomes `rg TODO .`; `python -m pytest` becomes `uv run python -m pytest`; `npm run dev` becomes `bun run dev`; `mypy .` becomes `ty check .` | Keeps suggestions predictable and minimal |
| Honest ambiguity handling | `pip install requests` suggests both `uv add requests` and `uv pip install requests`; unsupported npm/npx or type-checker flags are blocked for manual translation | Avoids pretending nearby tools always have identical semantics |
| Wrapper-aware parsing | Preserves `sudo`, `env`, `command`, `time`, `nohup`, `builtin`, assignments, pipes, and chained commands | Works on real shell commands instead of just toy examples |
| Shared rule catalog | The installer and Rust CLI read the same catalog for rule ids, aliases, descriptions, and prerequisites | Reduces drift as the rule list grows from a couple of families to many |
| Scalable rule selection | Installer supports `--list-rules`, repeated `--enable-rule`, repeated `--disable-rule`, and exact `--rules <rule[,rule...]>` | Scales to many rule families without adding a new `--only-foo` flag every time |
| Rule-aware hook updates | Reinstalling with a different rule selection updates the existing hook entry instead of appending duplicates | Keeps agent config clean over time |

Common outcomes:

| Input | Result |
|---|---|
| `grep -rn TODO .` | suggest `rg TODO .` |
| `fgrep literal README.md` | suggest `rg -F literal README.md` |
| `python -m pytest` | suggest `uv run python -m pytest` |
| `pip install requests` | suggest `uv add requests` and `uv pip install requests` |
| `pip uninstall requests` | suggest `uv remove requests` and `uv pip uninstall requests` |
| `uv init` | blocked with safer `uv` guidance |
| `npm run dev` | suggest `bun run dev` |
| `npm ci` | suggest `bun install --frozen-lockfile` |
| `npm test` | suggest `bun run test` |
| `npm start` | suggest `bun run start` |
| `npm init` | suggest `bun init` |
| `npx prettier .` | suggest `bunx prettier .` |
| `mypy .` | suggest `ty check .` |
| `pyright src` | suggest `ty check src` |
| `uv run mypy .` | suggest `uv run ty check .` |
| `grep -s TODO file.txt` | blocked for manual translation |

## Quick Example

```sh
# See the current rule catalog before enabling anything.
curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_tool_preferences/main/install.sh \
  | bash -s -- --list-rules

# Inspect the installer without cloning, building, or writing files.
curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_tool_preferences/main/install.sh \
  | bash -s -- --dry-run

# Default install: enable every shipped rule family.
curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_tool_preferences/main/install.sh \
  | bash

# Install just grep-family -> rg enforcement.
curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_tool_preferences/main/install.sh \
  | bash -s -- --enable-rule rg

# Start from the default all-rules install and subtract one family.
curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_tool_preferences/main/install.sh \
  | bash -s -- --disable-rule uv

# Direct CLI evaluation.
enforce-tool-preferences-command --list-rules
enforce-tool-preferences-command --command 'grep -rn TODO .'
enforce-tool-preferences-command --command 'python -m pytest'
enforce-tool-preferences-command --command 'pip install requests'
enforce-tool-preferences-command --command 'npm run dev'
enforce-tool-preferences-command --command 'mypy .'

# Limit evaluation to one rule family.
enforce-tool-preferences-command --command 'grep -rn TODO .' --rules rg
enforce-tool-preferences-command --command 'python -m pytest' --rules uv

# Hook payload input.
printf '%s' '{"tool_input":{"command":"grep -rn TODO ."}}' \
  | enforce-tool-preferences-command --codex-hook-json --rules rg,uv,bun,ty

# Benchmark the evaluator locally.
enforce-tool-preferences-command \
  --benchmark-command 'python -m pytest' \
  --iterations 100000 \
  --rules rg,uv,bun,ty
```

Representative outputs:

```text
Use rg (ripgrep) instead of grep in this project. Replace blocked grep commands with the least invasive exact rg rewrite when the flag mapping is clear. If a flag does not have a guaranteed direct rg translation, translate it manually instead of guessing.
Suggested replacement:
  rg TODO .
```

```text
Use uv instead of bare Python or pip commands in this project. Replace the blocked command with 'uv run ...', 'uv add ...', 'uv add --dev ...', 'uv remove ...', or 'uv run --with ...' as appropriate.
Likely alternatives:
  uv add requests
  uv pip install requests
Choose `uv add` for project dependencies; choose `uv pip` to keep pip-style behavior.
```

```text
Use bun instead of npm or npx in this project. Replace blocked commands with 'bun install', 'bun add', 'bun remove', 'bun run', 'bunx', 'bun create', 'bun publish', 'bun update', or 'bun outdated' when the mapping is exact. If an npm or npx flag does not have a guaranteed Bun equivalent, translate it manually instead of guessing.
Suggested replacement:
  bun run dev
```

```text
Use ty for Python type checking in this project. Replace blocked type-checker commands with 'ty check ...' when the mapping is exact, preserving uv or uvx wrappers when they define the execution environment. If a flag is tool-specific or changes semantics, translate it manually after checking 'ty check --help' instead of guessing.
Suggested replacement:
  ty check .
```

```json
{"decision":"block","reason":"Use rg (ripgrep) instead of grep in this project. Replace blocked grep commands with the least invasive exact rg rewrite when the flag mapping is clear. If a flag does not have a guaranteed direct rg translation, translate it manually instead of guessing.\nSuggested replacement:\n  rg TODO ."}
```

## Design Philosophy

### 1. One Hook Boundary, Multiple Policies

The hook boundary is where enforcement actually matters, so the combined binary owns all current rule families. Adding another family later should extend the dispatcher, not add another long-lived pile of hooks.

### 2. One Shared Rule Catalog

The installer and Rust CLI should not each carry their own copy of rule ids, aliases, descriptions, and prerequisites. This repo now keeps one shared catalog in the installer source and has the Rust binary read it directly, so discovery output and exact-set parsing stay aligned.

### 3. Stable Rule IDs, Not One-Off Installer Flags

The installer surface should scale by combining stable rule ids, not by minting `--only-foo` flags forever. Humans get discovery and additive/subtractive selection; scripts get an exact `--rules` value that stays easy to diff and automate.

### 4. Smallest Correct Rewrite

If the preferred tool already implies a flag, the suggestion drops the redundant noise. `grep -rn pattern .` becomes `rg pattern .`, not `rg -rn pattern .`.

### 5. Preserve the Original Shell Shape

The evaluator rewrites only the command portion. Wrapper commands and shell context stay intact so the suggestion still matches the original execution environment.

### 6. Block on Uncertainty

If `rg` flag semantics are unclear or a `pip` command could mean two different things, the tool does not invent certainty. Blocking is better than a subtly wrong rewrite.

### 7. Guidance and Enforcement Are Separate

`AGENTS.md`, `CLAUDE.md`, and `GEMINI.md` can teach preferred tool choices before a command runs. The binary enforces those choices when a shell command is actually about to execute.

## Comparison

| Approach | One hook process | Exact rewrites | Honest ambiguity handling | Selective rule install |
|---|---|---|---|---|
| `force_tool_preferences` | Yes | Yes | Yes | Yes |
| Separate hook repos (`force_rg` + `force_uv`) | No | Yes | Yes | Yes, but with extra processes |
| Repo policy files only (`AGENTS.md`, prompts, docs) | No | No | No | N/A |
| Shell aliases or wrapper scripts | Partially | No | No | Partially |
| Manual review after the fact | No | Maybe | Maybe | N/A |

When to use `force_tool_preferences`:

- You want hard enforcement at the shell boundary instead of relying on prompt compliance.
- You want one install path and one hook command for all current rule families.
- You want a stable installer surface that can keep growing from a few rule families to many without minting a new install flag per tool.

When `force_tool_preferences` might not be ideal:

- You only want soft written guidance and do not want blocking hooks.
- Your team does not use `rg`, `uv`, `bun`, or `ty` as the default workflow.
- You need policy that varies per directory, per command source, or by richer project context than the current rule selector exposes.

## Installation

### 1. Installer script

This is the default path and the fastest way to get hooks installed:

```sh
curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_tool_preferences/main/install.sh | bash
```

What it does:

1. Clones the repo to a temporary directory.
2. Builds `enforce-tool-preferences-command` with Cargo.
3. Installs or updates `~/.local/bin/enforce-tool-preferences-command`.
4. Configures Claude Code automatically if `~/.claude/settings.json` already exists.
5. Configures Gemini CLI automatically if `~/.gemini/` already exists.
6. Ensures Codex has `codex_hooks = true` in `${CODEX_HOME:-~/.codex}/config.toml`.
7. Updates the Claude, Gemini, and Codex hook commands with the selected `--rules` value.

Why this path exists:

- `install.sh --list-rules` and `enforce-tool-preferences-command --list-rules` read the same catalog.
- `--rules` parsing in the Rust CLI accepts the same ids and aliases the installer documents.
- Adding a new family now means extending the enforcement logic and updating one catalog entry, instead of hand-syncing multiple docs and option tables.

Rule selection model:

| Need | Recommended flags |
|---|---|
| Discover what exists | `--list-rules` |
| Pick a subset interactively | repeat `--enable-rule <name>` |
| Start from all defaults and subtract exceptions | repeat `--disable-rule <name>` |
| Keep a reproducible exact set in scripts, CI, or dotfiles | `--rules <rule[,rule...]>` |
| Preserve old one-off commands during migration | `--only-rg`, `--only-uv` (supported, but deprecated) |

Useful installer variants:

```sh
# Show the current rule catalog, aliases, and prerequisites.
curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_tool_preferences/main/install.sh \
  | bash -s -- --list-rules

# Print planned actions without cloning, building, or writing files.
curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_tool_preferences/main/install.sh \
  | bash -s -- --dry-run

# Print SHA-256 hashes for the built and installed binary.
curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_tool_preferences/main/install.sh \
  | bash -s -- --check-binary-hash

# Force overwrite even if the installed hash already matches.
curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_tool_preferences/main/install.sh \
  | bash -s -- --overwrite-binary

# Install only grep-family -> rg enforcement.
curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_tool_preferences/main/install.sh \
  | bash -s -- --enable-rule rg

# Install only python/pip-family -> uv enforcement.
curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_tool_preferences/main/install.sh \
  | bash -s -- --enable-rule uv

# Install only npm/npx-family -> bun enforcement.
curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_tool_preferences/main/install.sh \
  | bash -s -- --enable-rule bun

# Install every currently supported family except uv.
curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_tool_preferences/main/install.sh \
  | bash -s -- --disable-rule uv

# Exact rule selection for automation.
curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_tool_preferences/main/install.sh \
  | bash -s -- --rules rg,uv,bun,ty
```

Prerequisites:

| Selected rules | Required tools |
|---|---|
| `rg` | `cargo`, `rg` |
| `uv` | `cargo`, `uv` |
| `bun` | `cargo`, `bun` |
| `ty` | `cargo`, `ty` |
| `rg,uv,bun,ty` | `cargo`, `rg`, `uv`, `bun`, `ty` |

### 2. Build from source

Use this when you want the binary locally but do not want the remote installer path:

```sh
git clone https://github.com/maksymsherman/force_tool_preferences.git
cd force_tool_preferences
cargo build --release
mkdir -p ~/.local/bin
cp target/release/enforce-tool-preferences-command ~/.local/bin/
```

Then configure hooks manually for the agent runtimes you use:

```sh
mkdir -p ~/.claude ~/.gemini ~/.codex
[ -f ~/.claude/settings.json ] || printf '{}\n' > ~/.claude/settings.json
[ -f ~/.gemini/settings.json ] || printf '{}\n' > ~/.gemini/settings.json
[ -f ~/.codex/hooks.json ] || printf '{}\n' > ~/.codex/hooks.json

~/.local/bin/enforce-tool-preferences-command \
  --configure-claude-hook ~/.claude/settings.json ~/.local/bin/enforce-tool-preferences-command \
  --rules rg,uv,bun,ty

~/.local/bin/enforce-tool-preferences-command \
  --configure-gemini-hook ~/.gemini/settings.json ~/.local/bin/enforce-tool-preferences-command \
  --rules rg,uv,bun,ty

~/.local/bin/enforce-tool-preferences-command \
  --configure-codex-hook ~/.codex/hooks.json ~/.local/bin/enforce-tool-preferences-command \
  --rules rg,uv,bun,ty
```

For Codex, also ensure `~/.codex/config.toml` contains:

```toml
[features]
codex_hooks = true
```

### 3. Local development checkout

Use this when you are working on the repo itself and want to test behavior before installing anything to `~/.local/bin`:

```sh
git clone https://github.com/maksymsherman/force_tool_preferences.git
cd force_tool_preferences
cargo run -- --list-rules
cargo run -- --command 'grep -rn TODO .'
cargo run -- --command 'python -m pytest'
cargo run -- --command 'npm run dev'
cargo run -- --command 'mypy .'
```

This path is useful for development and verification, but it is not the recommended long-term hook target because agent configs should point to a stable binary path.

### Package managers and prebuilt binaries

There is no Homebrew, Scoop, npm, Cargo install, or prebuilt binary release path documented in this repo today. The supported install methods are:

- the remote `install.sh` flow
- a local source build copied into `~/.local/bin`
- a development checkout used directly with `cargo run`

## Quick Start

### 1. Inspect the installer and rule catalog

```sh
curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_tool_preferences/main/install.sh \
  | bash -s -- --list-rules

curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_tool_preferences/main/install.sh \
  | bash -s -- --dry-run
```

### 2. Install the rule families you want

```sh
# Default: all shipped families
curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_tool_preferences/main/install.sh \
  | bash

# Only rg
curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_tool_preferences/main/install.sh \
  | bash -s -- --enable-rule rg

# Only uv
curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_tool_preferences/main/install.sh \
  | bash -s -- --enable-rule uv

# Only bun
curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_tool_preferences/main/install.sh \
  | bash -s -- --enable-rule bun

# Only ty
curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_tool_preferences/main/install.sh \
  | bash -s -- --enable-rule ty

# Everything except uv
curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_tool_preferences/main/install.sh \
  | bash -s -- --disable-rule uv

# Exact set for automation
curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_tool_preferences/main/install.sh \
  | bash -s -- --rules rg,uv,bun,ty
```

### 3. Restart any running agent sessions

Existing Claude Code, Gemini CLI, and Codex sessions usually keep old hook state until they restart.

### 4. Validate the behavior directly

```sh
enforce-tool-preferences-command --command 'grep -rn TODO .'
enforce-tool-preferences-command --command 'python -m pytest'
enforce-tool-preferences-command --command 'pip install requests'
enforce-tool-preferences-command --command 'npm run dev'
enforce-tool-preferences-command --command 'mypy .'
```

Expected behavior:

- blocked direct CLI calls exit with status `2`
- hook JSON modes emit the block payload expected by the runtime
- allowed commands exit `0` without output

## Current Shipped Rules

### grep-family -> `rg`

Blocked commands:

- `grep`
- `egrep`
- `fgrep`

High-confidence rewrites stay small:

- `grep -rn pattern .` -> `rg pattern .`
- `grep -E 'foo|bar' file.txt` -> `rg 'foo|bar' file.txt`
- `fgrep literal file.txt` -> `rg -F literal file.txt`
- `sudo grep -rl TODO /var/log` -> `sudo rg -l TODO /var/log`

If a flag does not have a guaranteed direct `rg` translation, the command is blocked and the flagged options are listed explicitly.

### python-family -> `uv`

Blocked commands:

- bare `python`
- bare `python3`, `python3.11`, and similar versioned `python*` executables
- bare `pip`
- bare `pip3`, `pip3.12`, and similar versioned `pip*` executables
- `uv init` in an existing-project workflow

Typical outcomes:

- `python script.py` -> `uv run python script.py`
- `python -m pytest` -> `uv run python -m pytest`
- `pip list` -> `uv pip list`
- `pip install requests` -> `uv add requests` or `uv pip install requests`
- `pip uninstall requests` -> `uv remove requests` or `uv pip uninstall requests`
- `uv init` -> blocked with guidance toward `uv run`, `uv add`, `uv sync`, or `uv run --with`

### npm/npx-family -> `bun`

Blocked commands:

- `npm`
- `npx`

Typical outcomes:

- `npm install` -> `bun install`
- `npm install react` -> `bun add react`
- `npm install --save-dev typescript` -> `bun add -d typescript`
- `npm ci` -> `bun install --frozen-lockfile`
- `npm uninstall react` -> `bun remove react`
- `npm run dev` -> `bun run dev`
- `npm test` -> `bun run test`
- `npm start` -> `bun run start`
- `npm init` -> `bun init`
- `npm link` -> `bun link`
- `npm exec vite -- --host` -> `bun vite -- --host`
- `npm create vite@latest app` -> `bun create vite@latest app`
- `npm publish dist` -> `bun publish dist`
- `npm update --latest vite` -> `bun update --latest vite`
- `npm pack` -> `bun pm pack`
- `npx prettier .` -> `bunx prettier .`

If the npm or npx command shape depends on npm-only flags or on a package-vs-binary distinction that is not obvious, the command is blocked and you are told to translate it manually instead of guessing.

### type-checker-family -> `ty`

Blocked commands:

- `mypy`
- `pyright`
- `basedpyright`

Typical outcomes:

- `mypy .` -> `ty check .`
- `pyright src` -> `ty check src`
- `basedpyright packages/api` -> `ty check packages/api`
- `python -m mypy .` -> `ty check .`
- `uv run mypy .` -> `uv run ty check .`
- `uvx mypy .` -> `uvx ty check .`

If the original type-checker command depends on tool-specific flags, the command is blocked and the flagged options are listed explicitly so you can translate them manually.

## Command Reference

### `--list-rules`

Print the shared rule catalog that both the installer and Rust CLI use.

```sh
enforce-tool-preferences-command --list-rules
```

### `--command '<shell command>'`

Evaluate a shell command passed directly on the command line.

```sh
enforce-tool-preferences-command --command 'grep -rn TODO .'
enforce-tool-preferences-command --command 'python -m pytest'
enforce-tool-preferences-command --command 'npm run dev'
enforce-tool-preferences-command --command 'mypy .'
```

### `--stdin-command`

Read the shell command from stdin instead of an argument.

```sh
printf '%s' 'grep -rn TODO .' \
  | enforce-tool-preferences-command --stdin-command
```

### `--claude-hook-json`

Read hook JSON from stdin, extract `tool_input.command`, and emit Claude-style JSON block output when the command is rejected.

```sh
printf '%s' '{"tool_input":{"command":"grep -rn TODO ."}}' \
  | enforce-tool-preferences-command --claude-hook-json --rules rg,uv,bun,ty
```

### `--codex-hook-json`

Read hook JSON from stdin, extract `tool_input.command`, and emit Codex-style JSON block output when the command is rejected.

```sh
printf '%s' '{"tool_input":{"command":"python -m pytest"}}' \
  | enforce-tool-preferences-command --codex-hook-json --rules rg,uv,bun,ty
```

### `--gemini-hook-json`

Read hook JSON from stdin and evaluate `tool_input.command` for Gemini hook integration.

```sh
printf '%s' '{"tool_input":{"command":"pip install requests"}}' \
  | enforce-tool-preferences-command --gemini-hook-json --rules rg,uv,bun,ty
```

### `--claude-json`

When used with `--command` or `--stdin-command`, emit JSON block output instead of plain text.

```sh
enforce-tool-preferences-command \
  --command 'grep -rn TODO .' \
  --claude-json \
  --rules rg,uv,bun,ty
```

### `--rules <rule[,rule...]>`

Restrict which rule families are active for this invocation or configured hook. This is the same exact-set model the installer uses for automation.

```sh
enforce-tool-preferences-command --command 'grep -rn TODO .' --rules rg
enforce-tool-preferences-command --command 'python -m pytest' --rules uv
enforce-tool-preferences-command --command 'npm run dev' --rules bun
enforce-tool-preferences-command --command 'mypy .' --rules ty
enforce-tool-preferences-command --command 'pip install requests' --rules rg,uv,bun,ty
```

### `--benchmark-command '<shell command>'`

Benchmark the evaluator on the same input repeatedly.

```sh
enforce-tool-preferences-command \
  --benchmark-command 'python -m pytest' \
  --iterations 1000000 \
  --rules rg,uv,bun,ty
```

### `--iterations <n>`

Set the loop count for benchmark mode. The value must be greater than `0`.

### `--configure-claude-hook <settings-path> <binary-name>`

Update Claude Code hook settings in place.

```sh
enforce-tool-preferences-command \
  --configure-claude-hook ~/.claude/settings.json ~/.local/bin/enforce-tool-preferences-command \
  --rules rg,uv,bun,ty
```

### `--configure-gemini-hook <settings-path> <binary-name>`

Update Gemini CLI hook settings in place.

```sh
enforce-tool-preferences-command \
  --configure-gemini-hook ~/.gemini/settings.json ~/.local/bin/enforce-tool-preferences-command \
  --rules rg,uv,bun,ty
```

### `--configure-codex-hook <hooks-path> <binary-name>`

Update Codex hook settings in place.

```sh
enforce-tool-preferences-command \
  --configure-codex-hook ~/.codex/hooks.json ~/.local/bin/enforce-tool-preferences-command \
  --rules rg,uv,bun,ty
```

### `--help`

Print usage text.

Exit behavior:

| Mode | Allowed command | Blocked command | Invalid usage |
|---|---|---|---|
| Direct CLI (`--command`, `--stdin-command`) | `0` | `2` | `1` |
| Claude/Codex hook JSON | `0` | `0` with block JSON | `1` |
| Gemini hook JSON | `0` | `2` | `1` |

## Configuration

### Claude Code

Claude uses a `PreToolUse` hook with the `Bash` matcher.

`~/.claude/settings.json`

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          {
            "type": "command",
            "command": "~/.local/bin/enforce-tool-preferences-command --claude-hook-json --rules rg,uv,bun,ty"
          }
        ]
      }
    ]
  }
}
```

Automatic configuration:

```sh
enforce-tool-preferences-command \
  --configure-claude-hook ~/.claude/settings.json ~/.local/bin/enforce-tool-preferences-command \
  --rules rg,uv,bun,ty
```

Installer note: automatic Claude setup only runs when `~/.claude/settings.json` already exists.

### Gemini CLI

Gemini uses a `BeforeTool` hook with the `run_shell_command` matcher.

`~/.gemini/settings.json`

```json
{
  "hooks": {
    "BeforeTool": [
      {
        "matcher": "run_shell_command",
        "hooks": [
          {
            "type": "command",
            "command": "~/.local/bin/enforce-tool-preferences-command --gemini-hook-json --rules rg,uv,bun,ty"
          }
        ]
      }
    ]
  }
}
```

Automatic configuration:

```sh
mkdir -p ~/.gemini
[ -f ~/.gemini/settings.json ] || printf '{}\n' > ~/.gemini/settings.json
enforce-tool-preferences-command \
  --configure-gemini-hook ~/.gemini/settings.json ~/.local/bin/enforce-tool-preferences-command \
  --rules rg,uv,bun,ty
```

Installer note: automatic Gemini setup runs only when `~/.gemini/` already exists.

### Codex

Codex uses a `PreToolUse` hook with the `Bash` matcher.

`~/.codex/config.toml`

```toml
[features]
codex_hooks = true
```

`~/.codex/hooks.json`

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          {
            "type": "command",
            "command": "~/.local/bin/enforce-tool-preferences-command --codex-hook-json --rules rg,uv,bun,ty"
          }
        ]
      }
    ]
  }
}
```

Automatic configuration:

```sh
mkdir -p ~/.codex
[ -f ~/.codex/hooks.json ] || printf '{}\n' > ~/.codex/hooks.json
enforce-tool-preferences-command \
  --configure-codex-hook ~/.codex/hooks.json ~/.local/bin/enforce-tool-preferences-command \
  --rules rg,uv,bun,ty
```

The installer also ensures `${CODEX_HOME:-~/.codex}/config.toml` contains `codex_hooks = true`.

### Project-local guidance files

This repo also ships:

- `AGENTS.md`
- `CLAUDE.md`
- `GEMINI.md`

Those files are optional prompt-level guidance. The binary and hooks are the actual shell boundary enforcement layer.

## Architecture

```text
                     shared rule catalog in install.sh
                                   |
                    +--------------+--------------+
                    |                             |
                    v                             v
               installer UI              Rust CLI rule metadata
                    |                             |
                    v                             v
               install.sh             enforce-tool-preferences-command
                    |                             |
       +------------+------------+                |
       |            |            |                |
       v            v            v                v
 build binary  update hooks  enable Codex   parse commands + enforce rules
                    feature flag
       |
       v
 ~/.local/bin/enforce-tool-preferences-command   ~/.claude / ~/.gemini / ~/.codex


 Agent shell command or hook payload
                 |
                 v
  enforce-tool-preferences-command
                 |
       tokenize + preserve wrappers
                 |
                 v
          rule-family dispatcher
         /           |           |             \
        v            v           v              v
 grep-family -> rg  python-family -> uv  npm/npx-family -> bun  type-checker-family -> ty
        |            |           |              |
        +------------+-----------+--------------+
                     |
                     v
         allow command or block with
         exact rewrite / alternatives /
         manual-translation guidance
```

Data flow summary:

1. The shared catalog in `install.sh` defines the shipped rule ids, aliases, descriptions, and prerequisites.
2. The installer uses that catalog for listing, validation, prerequisites, and exact-set resolution.
3. `enforce-tool-preferences-command` reads the same catalog for `--list-rules`, aliases, and `--rules` parsing.
4. When an agent invokes a shell command or sends a hook payload, the binary tokenizes the command while preserving wrapper context.
5. The rule dispatcher evaluates only the enabled families from `--rules`.
6. Safe rewrites are rendered directly; ambiguous cases are blocked with guidance.
7. Allowed commands exit cleanly; hook modes emit the format their runtime expects.

## Performance

`force_tool_preferences` is intended to stay enabled in the hook path, so there are two numbers that matter:

1. The evaluator cost inside an already-running process
2. The actual wall time of invoking the hook as a subprocess

### Evaluator Cost

Sample measurements from this repo on one machine with a release build of `enforce-tool-preferences-command`:

| Case | Example input | Average per evaluation |
|---|---|---:|
| Allowed `rg` command | `rg TODO .` | `0.0831 us` |
| Allowed `uv` command | `uv run pytest` | `0.1223 us` |
| Blocked grep-family command | `grep -rn TODO .` | `0.4861 us` |
| Blocked python-family command | `python -m pytest` | `0.4750 us` |
| Blocked type-checker-family command | `mypy .` | `0.3378 us` |

Reproduce locally:

```sh
cargo build --release

./target/release/enforce-tool-preferences-command \
  --benchmark-command 'rg TODO .' \
  --iterations 5000000 \
  --rules rg,uv,bun,ty

./target/release/enforce-tool-preferences-command \
  --benchmark-command 'python -m pytest' \
  --iterations 5000000 \
  --rules rg,uv,bun,ty

./target/release/enforce-tool-preferences-command \
  --benchmark-command 'mypy .' \
  --iterations 5000000 \
  --rules rg,uv,bun,ty
```

### Practical Hook Wall Time

Warm-cache sample measurements from this repo on one machine, estimated by batching 200 invocations and dividing total wall time back down to a per-hook figure:

| Path | Example input | Approximate wall time per hook |
|---|---|---:|
| Direct CLI allow | `--command 'rg TODO .'` | `6.533 ms` |
| Direct CLI block | `--command 'grep -rn TODO .'` | `6.642 ms` |
| Stdin command block | `--stdin-command` with `grep -rn TODO .` | `7.044 ms` |
| Codex hook JSON block | `--codex-hook-json` with `python -m pytest` | `7.013 ms` |

The overall pattern matches the split repos: evaluator work is effectively free, while subprocess startup, CLI parsing, stdin handling, and JSON handling dominate the real hook cost.

## Troubleshooting

### `cargo not found`

The installer builds from source. Install Rust first:

```sh
curl https://sh.rustup.rs -sSf | sh
```

### `rg not found`

If `rg` rules are enabled, ripgrep must already be installed.

Check:

```sh
rg --version
```

Then install ripgrep with your system package manager and rerun the installer.

### `uv not found`

If `uv` rules are enabled, `uv` must already be installed.

Check:

```sh
uv --version
```

Then install `uv` and rerun the installer.

### `bun not found`

If `bun` rules are enabled, `bun` must already be installed.

Check:

```sh
bun --version
```

Then install `bun` and rerun the installer.

### `ty not found`

If `ty` rules are enabled, `ty` must already be installed.

Check:

```sh
ty version
```

Then install `ty` and rerun the installer.

### Claude Code was detected, but hooks were not configured

Automatic Claude setup only runs when `~/.claude/settings.json` already exists. Create the file, run the `--configure-claude-hook` command from this README, then restart Claude Code.

### The hook exists but does not fire

Most often, the agent session was already running. Restart Claude Code, Gemini CLI, or Codex after installation so the updated hook state loads.

### A grep-family command was blocked without a replacement

That is expected for uncertain flags. Translate the flagged options manually after checking `rg --help` instead of assuming they behave the same.

### `pip install` suggested `uv pip`, but I wanted `uv add`

That usually means the original command was ambiguous. Use `uv add` when you want to change project metadata. Use `uv pip` when you want pip-style environment mutation.

### `uv init` was blocked, but I really do want project creation

This tool assumes an existing-repo workflow by default. If the user explicitly wants project creation or conversion, rerun with that intent in mind and use `uv init --no-readme --no-workspace` to reduce the risk of overwriting existing files.

### An `npm` or `npx` command was blocked for manual translation

That is expected when the command uses npm-only flags or when the tool cannot tell whether Bun should run a local binary, a one-off package, or a different package-manager subcommand. Translate it manually after checking `bun --help` or `bunx --help` instead of guessing.

### A type-checker command was blocked for manual translation

That is expected when the original `mypy`, `pyright`, or `basedpyright` command used tool-specific flags. Translate those flags manually after checking `ty check --help` instead of assuming they map one-to-one.

## Limitations

- This tool evaluates shell commands, not prose instructions or file edits.
- It does not auto-rewrite and re-run the blocked command; it blocks and explains instead.
- Shell parsing is deliberately practical rather than a full shell AST.
- Rule selection is hook-level via `--rules`, not per directory or per richer project context.
- Only the current `rg`, `uv`, `bun`, and `ty` families are implemented today.
- Many grep flags are intentionally blocked for manual translation instead of guessed.
- `pip install` and `pip uninstall` cannot always distinguish project metadata changes from environment mutation.
- Many npm and npx flags are intentionally blocked for manual translation instead of guessed.
- The binary evaluates the command string it receives. If a runtime only passes `bash script.sh`, it does not inspect the script body.
- The documented install path is a Bash script that builds from source. There are no package-manager or prebuilt-binary releases documented in this repo today.

## FAQ

### Does `force_tool_preferences` rewrite the command automatically?

No. It blocks and prints the exact rewrite or likely alternatives. That keeps the change visible and avoids silently altering shell behavior.

### Can I enable only one rule family?

Yes. The preferred installer form is `--enable-rule rg`, `--enable-rule uv`, `--enable-rule bun`, or `--enable-rule ty`. For automation or manual hook configuration, use `--rules rg`, `--rules uv`, `--rules bun`, or `--rules ty`. The older `--only-rg` and `--only-uv` aliases still work, but they are kept only for compatibility.

### How do I add another rule family cleanly?

Add the enforcement logic in Rust, then add one catalog row in `install.sh` for the new rule id, aliases, description, and prerequisites. That keeps installer discovery and CLI parsing aligned without maintaining separate rule tables.

### Why does `pip install requests` return two answers?

Because the command is ambiguous. It could mean "add this dependency to the project" or "mutate this environment right now." The tool shows both likely `uv` paths instead of forcing the wrong interpretation.

### Does it catch wrappers, full paths, and versioned binaries?

Yes. The evaluator understands wrappers such as `sudo`, `env`, `command`, `time`, `nohup`, and `builtin`, and it normalizes paths and names like `/usr/bin/grep`, `python3.11`, and `pip3.12`.

### Will it allow `rg`, `uv`, `uvx`, `bun`, `bunx`, and `ty`?

`rg`, `bun`, `bunx`, and `ty` pass through unchanged. `uvx` generally passes through too, except when it launches a blocked type checker such as `uvx mypy`. `uv` generally passes through too, except for `uv init` and `uv` invocations that directly launch a blocked type checker such as `uv run mypy`.

### Do I still need `AGENTS.md` if hooks are enabled?

Not for shell enforcement. Keep the policy files only if you also want prompt-level written guidance before a shell command is attempted.

### What happened to `force_rg` and `force_uv`?

Their behavior and history were folded into this combined repo. The import history is preserved on the `import/force-rg` and `import/force-uv` branches.

## License

MIT. See [LICENSE](LICENSE).
