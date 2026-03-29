# force_uv

```text
python -m pytest
       |
       v
+--------------------+
| enforce-uv-command |
+--------------------+
   | allow      | block
   |            v
   |   uv run python -m pytest
   v
continue
```

<div align="center">

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust 2021](https://img.shields.io/badge/rust-2021-black?logo=rust)](https://www.rust-lang.org/)
[![uv First](https://img.shields.io/badge/python-workflows-uv%20first-4B8BBE)](https://docs.astral.sh/uv/)
[![Agents](https://img.shields.io/badge/agents-Claude%20Code%20%7C%20Gemini%20%7C%20Codex-0A66C2)](#installation)

Shell-hook enforcement for `uv`-first Python workflows across Codex, Claude Code, and Gemini CLI.

</div>

Quick install:

```sh
curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_uv/main/install.sh | bash
```

## TL;DR

### The Problem

Agent workflows drift back to `python`, `python3`, `pip`, and `pip3` even when the repo is supposed to be `uv`-first. Prompt-only instructions help, but they are easy to miss and they do not enforce anything at the moment a shell command is about to run.

### The Solution

`force_uv` adds a small Rust binary, `enforce-uv-command`, that evaluates shell commands before execution. When it sees a bare Python or pip invocation, it blocks the command and prints an exact rewrite or a short list of likely `uv` alternatives.

It also blocks `uv init` in existing-project workflows and points the agent toward safer patterns such as `uv run`, `uv add`, `uv sync`, and `uv run --with`.

### Why Use force_uv?

| Capability | What it does |
|---|---|
| Exact rewrites when confidence is high | `python -m pytest` becomes `uv run python -m pytest` |
| Conservative ambiguity handling | `pip install requests` returns both `uv add requests` and `uv pip install requests` |
| Wrapper-aware parsing | Preserves `sudo`, `env`, `command`, `nohup`, `time`, and `builtin` prefixes |
| Hook-friendly output modes | Supports direct command mode, stdin mode, and Codex/Claude/Gemini hook JSON modes |
| Safer repo initialization policy | Blocks `uv init` unless the user explicitly wants project creation/conversion |
| Low-friction deployment | Installer builds the binary, configures Claude/Gemini hooks, and enables Codex hooks |

## Quick Example

```sh
# Install the binary and configure detected agent integrations
curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_uv/main/install.sh | bash

# Exact rewrite for a bare Python command
enforce-uv-command --command 'python -m pytest'

# Ambiguous pip install: project dependency vs pip-style environment change
enforce-uv-command --command 'pip install requests'

# Wrapper commands are preserved
enforce-uv-command --command 'sudo -u root python script.py'

# Safe uv usage passes through
enforce-uv-command --command 'uv run pytest'

# Codex/Claude-style hook JSON input
printf '%s' '{"tool_input":{"command":"python -m pytest"}}' \
  | enforce-uv-command --codex-hook-json
```

Expected behavior:

- Allowed commands exit `0`
- Blocked commands exit `2`
- Invalid usage exits `1`

## Design Philosophy

### 1. Block confidently, suggest conservatively

When the rewrite is obvious, `force_uv` gives one exact replacement. When the intent is ambiguous, it refuses to pretend otherwise and shows a minimal set of likely `uv` alternatives.

### 2. Preserve the original shell shape

The tool inserts or replaces only the command portion. Wrapper commands such as `sudo -u root`, `env FOO=1`, or `nohup` remain intact so the suggested command still matches the original execution context.

### 3. Distinguish project metadata changes from environment mutations

`pip install requests` can mean "add this to the project" or "modify this environment right now." `force_uv` surfaces both `uv add` and `uv pip install` when needed instead of forcing the wrong interpretation.

### 4. Make enforcement cheap enough to leave on

The implementation is a single Rust binary with no runtime service. It is designed to sit in the hook path, parse fast, print a clear reason, and get out of the way.

### 5. Bias away from destructive initialization

`uv init` is useful for project creation, but risky inside an existing repo. `force_uv` blocks it by default and points users toward the safer `uv` commands they usually wanted in the first place.

## Performance

`force_uv` is meant to stay enabled in the hook path, so there are two different numbers worth understanding:

1. The evaluator cost inside an already-running process
2. The actual wall time of invoking the hook as a subprocess

### Evaluator Cost

Sample measurements from repeated built-in benchmark runs on one machine with a release build of `enforce-uv-command`:

| Case | Example input | Average per evaluation |
|---|---|---:|
| Allowed command | `uv run pytest` | `0.2765 us` (`0.0000002765 s`) |
| Blocked command | `python -m pytest` | `0.3729 us` (`0.0000003729 s`) |

These numbers measure only the command evaluator inside the binary.

Reproduce locally:

```sh
cargo build --release

./target/release/enforce-uv-command \
  --benchmark-command 'uv run pytest' \
  --iterations 5000000

./target/release/enforce-uv-command \
  --benchmark-command 'python -m pytest' \
  --iterations 5000000
```

### Practical Hook Wall Time

Warm-cache sample measurements from one machine, estimated by batching 200 invocations and dividing the total wall time back down to a per-hook figure:

| Path | Example input | Approximate wall time per hook |
|---|---|---:|
| Direct CLI allow | `--command 'uv run pytest'` | `3.4 ms` |
| Direct CLI block | `--command 'python -m pytest'` | `3.2 ms` |
| Stdin command block | `--stdin-command` with `python -m pytest` | `4.6 ms` |
| Codex/Claude hook JSON block | `--codex-hook-json` with `tool_input.command` | `4.4 ms` |

That practical number is what matters in agent use. The gap between the microbenchmark and the real hook cost comes from shell startup, process startup, CLI parsing, stdin reads, JSON parsing, output formatting, and pipe plumbing. Results will vary across machines, but the overall shape is the same: the evaluator itself is effectively free, while the subprocess hook architecture costs low single-digit milliseconds per invocation.

## Comparison

| Approach | Blocks bare `python`/`pip` | Suggests concrete rewrites | Works across Codex/Claude/Gemini | Handles ambiguity honestly |
|---|---|---|---|---|
| `force_uv` hook enforcement | Yes | Yes | Yes | Yes |
| Repo instructions only (`AGENTS.md`, prompts, docs) | No | Sometimes, manually | Depends on the agent | Usually not |
| Shell aliases or wrapper scripts | Partially | No | No | No |
| Manual review after the fact | No | Maybe | No | Maybe |

When to use `force_uv`:

- You want hard enforcement at the shell boundary instead of relying on prompt compliance.
- You want `python` and `pip` commands blocked with exact or confidence-graded `uv` rewrites.
- You need one policy that behaves consistently across Codex, Claude Code, and Gemini CLI.

When `force_uv` might not be ideal:

- You only want soft guidance and do not want blocking hooks.
- Your team does not use `uv` as the default Python workflow.
- You need Windows support for Codex hooks today.

## Installation

### 1. Installer script

This is the default path:

```sh
curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_uv/main/install.sh | bash
```

What it does:

1. Clones the repo to a temporary directory.
2. Builds `enforce-uv-command` with Cargo.
3. Installs or updates `~/.local/bin/enforce-uv-command`.
4. Configures Claude Code automatically if `~/.claude/settings.json` exists.
5. Configures Gemini CLI automatically if `~/.gemini/` exists.
6. Enables Codex hooks in `${CODEX_HOME:-~/.codex}/config.toml` and `${CODEX_HOME:-~/.codex}/hooks.json`.

Prerequisites:

- `cargo`
- `uv`

Useful installer variants:

```sh
# Print the exact repo files and planned actions without executing anything
curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_uv/main/install.sh \
  | bash -s -- --dry-run

# Show SHA-256 hashes for the built and installed binary
curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_uv/main/install.sh \
  | bash -s -- --check-binary-hash

# Overwrite the installed binary even if the hashes already match
curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_uv/main/install.sh \
  | bash -s -- --check-binary-hash --overwrite-binary
```

### 2. Build from source

```sh
git clone https://github.com/maksymsherman/force_uv.git
cd force_uv
cargo build --release
mkdir -p ~/.local/bin
cp target/release/enforce-uv-command ~/.local/bin/
chmod +x ~/.local/bin/enforce-uv-command
```

### 3. Build and configure manually

```sh
git clone https://github.com/maksymsherman/force_uv.git
cd force_uv
cargo build --release
mkdir -p ~/.local/bin
cp target/release/enforce-uv-command ~/.local/bin/
chmod +x ~/.local/bin/enforce-uv-command
```

Then configure hooks yourself:

```sh
mkdir -p ~/.gemini
[ -f ~/.gemini/settings.json ] || printf '{}\n' > ~/.gemini/settings.json
enforce-uv-command --configure-gemini-hook ~/.gemini/settings.json ~/.local/bin/enforce-uv-command
mkdir -p ~/.codex
[ -f ~/.codex/hooks.json ] || printf '{}\n' > ~/.codex/hooks.json
enforce-uv-command --configure-codex-hook ~/.codex/hooks.json ~/.local/bin/enforce-uv-command
```

For Claude Code, either create `~/.claude/settings.json` first or add the hook snippet manually from the configuration section below. For Codex, also set `codex_hooks = true` under `[features]` in `~/.codex/config.toml`.

### 4. Policy files only

If you do not want the binary yet, you can still copy the repo-local guidance file into another project:

```sh
curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_uv/main/AGENTS.md -o AGENTS.md
```

## Quick Start

1. Install the tool with the one-line installer or build it from source.
2. Restart any running Codex, Claude Code, or Gemini CLI sessions so the new hooks load.
3. Verify that safe `uv` commands pass and bare Python commands get blocked:

```sh
enforce-uv-command --command 'uv run pytest'
enforce-uv-command --command 'python -m pytest'
enforce-uv-command --command 'pip install requests'
```

4. Add `AGENTS.md` to repos where you also want prompt-level written guidance before a shell command is attempted.

### Claude Code

Run the installer, then restart Claude Code sessions so the hook becomes active. If automatic configuration is not possible, add this to `~/.claude/settings.json`:

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

### Gemini CLI

Run the installer, then restart Gemini CLI sessions. Manual configuration:

```json
{
  "hooks": {
    "BeforeTool": [
      {
        "matcher": "run_shell_command",
        "hooks": [
          {
            "type": "command",
            "command": "enforce-uv-command --gemini-hook-json"
          }
        ]
      }
    ]
  }
}
```

### Codex

Codex uses a `PreToolUse` hook with the `Bash` matcher. The installer enables the `codex_hooks` feature flag in `~/.codex/config.toml`, adds the hook entry to `~/.codex/hooks.json`, and leaves `AGENTS.md` as an optional repo-local guidance file.

Codex hooks are experimental, and the current Codex docs note that Windows support is temporarily disabled.

Important caveat: Codex currently applies this policy at the `PreToolUse` Bash interception point. That means `force_uv` sees and can block direct Bash commands such as `python -m pytest`, but it does not inspect the contents of a script that Codex writes to disk and later runs with `bash script.sh`. Treat this as a workflow guardrail, not a sandbox or hard security boundary.

If you configure Codex manually, use this shape:

```toml
[features]
codex_hooks = true
```

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          {
            "type": "command",
            "command": "~/.local/bin/enforce-uv-command --codex-hook-json"
          }
        ]
      }
    ]
  }
}
```

Then restart Codex so the new hook state loads.

## Command Reference

| Command | Purpose | Example |
|---|---|---|
| `--command <text>` | Evaluate one shell command directly | `enforce-uv-command --command 'python -m pytest'` |
| `--stdin-command` | Read the command from stdin | `printf '%s' 'python -m pytest' \| enforce-uv-command --stdin-command` |
| `--claude-hook-json` | Read Claude hook JSON and emit JSON block output | `printf '%s' '{"tool_input":{"command":"python -m pytest"}}' \| enforce-uv-command --claude-hook-json` |
| `--codex-hook-json` | Read Codex hook JSON and emit JSON block output | `printf '%s' '{"tool_input":{"command":"python -m pytest"}}' \| enforce-uv-command --codex-hook-json` |
| `--gemini-hook-json` | Read Gemini hook JSON and emit plain-text blocking output | `printf '%s' '{"tool_input":{"command":"python -m pytest"}}' \| enforce-uv-command --gemini-hook-json` |
| `--configure-claude-hook <settings> <binary>` | Add the Claude hook entry to a settings file | `enforce-uv-command --configure-claude-hook ~/.claude/settings.json ~/.local/bin/enforce-uv-command` |
| `--configure-gemini-hook <settings> <binary>` | Add the Gemini hook entry to a settings file | `enforce-uv-command --configure-gemini-hook ~/.gemini/settings.json ~/.local/bin/enforce-uv-command` |
| `--configure-codex-hook <hooks> <binary>` | Add the Codex hook entry to a hooks file | `enforce-uv-command --configure-codex-hook ~/.codex/hooks.json ~/.local/bin/enforce-uv-command` |
| `--benchmark-command <text>` | Benchmark the evaluator on one command string | `enforce-uv-command --benchmark-command 'python -m pytest'` |

### `--command '<shell command>'`

Evaluate a single command string.

```sh
enforce-uv-command --command 'python -m pytest'
enforce-uv-command --command 'pip list'
enforce-uv-command --command 'sudo -u root python script.py'
```

### `--stdin-command`

Read the command string from stdin.

```sh
printf '%s' 'python -m pytest' | enforce-uv-command --stdin-command
```

### `--claude-hook-json`

Read Claude Code hook JSON from stdin, extract `tool_input.command`, and emit Claude-compatible block JSON when needed.

```sh
printf '%s' '{"tool_input":{"command":"python -m pytest"}}' \
  | enforce-uv-command --claude-hook-json
```

Blocked output looks like:

```json
{"decision":"block","reason":"Use uv instead of bare Python or pip commands in this project. Replace the blocked command with 'uv run ...', 'uv add ...', 'uv add --dev ...', 'uv remove ...', or 'uv run --with ...' as appropriate.\nSuggested replacement:\n  uv run python -m pytest"}
```

### `--codex-hook-json`

Read Codex hook JSON from stdin, extract `tool_input.command`, and emit the JSON block shape Codex still accepts for `PreToolUse`.

```sh
printf '%s' '{"tool_input":{"command":"python -m pytest"}}' \
  | enforce-uv-command --codex-hook-json
```

### `--gemini-hook-json`

Reads the same hook-style JSON shape from stdin, but prints the human-readable block message expected by Gemini CLI hook execution.

```sh
printf '%s' '{"tool_input":{"command":"python -m pytest"}}' \
  | enforce-uv-command --gemini-hook-json
```

### `--claude-json`

Use JSON block output with `--command` or `--stdin-command`.

```sh
enforce-uv-command --command 'python -m pytest' --claude-json
printf '%s' 'python -m pytest' | enforce-uv-command --stdin-command --claude-json
```

### `--configure-claude-hook <settings-path> <binary-name>`

Add the Claude Code `PreToolUse` hook entry to an existing settings file.

### `--configure-gemini-hook <settings-path> <binary-name>`

Add the Gemini CLI `BeforeTool` hook entry to a settings file.

### `--configure-codex-hook <hooks-path> <binary-name>`

Add the Codex `PreToolUse` hook entry to a `hooks.json` file.

### `--benchmark-command '<shell command>'`

Benchmark the parser and decision path on the same input repeatedly.

```sh
enforce-uv-command --benchmark-command 'python -m pytest'
enforce-uv-command --benchmark-command 'python -m pytest' --iterations 1000000
```

### `--iterations <n>`

Set the loop count for benchmark mode. Must be greater than `0`.

### `--help`

Print usage text.

## What Gets Blocked

### Bare Python invocations

Examples:

```sh
python script.py
python -m pytest
python3 -m http.server
sudo -u root python script.py
env FOO=1 python script.py
```

Typical rewrite:

```sh
uv run python script.py
```

### Bare pip invocations

Examples:

```sh
pip list
pip install requests
pip uninstall requests
pip3 install -r requirements.txt
```

Typical outcomes:

```text
pip list
  -> uv pip list

pip install requests
  -> uv add requests
  -> uv pip install requests

pip uninstall requests
  -> uv remove requests
  -> uv pip uninstall requests
```

### `uv init` in existing-repo workflows

Blocked guidance:

```text
Do not run 'uv init' in an existing project unless the user explicitly asks for project creation or conversion.
```

Recommended alternatives depend on intent:

- `uv run ...` to execute code
- `uv add ...` or `uv add --dev ...` to add dependencies
- `uv remove ...` to remove dependencies
- `uv sync` to realize existing project metadata
- `uv run --with ...` for one-off tools

## Configuration

### Claude Code hook config

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

Automatic configuration:

```sh
enforce-uv-command --configure-claude-hook ~/.claude/settings.json ~/.local/bin/enforce-uv-command
```

### Gemini CLI hook config

```json
{
  "hooks": {
    "BeforeTool": [
      {
        "matcher": "run_shell_command",
        "hooks": [
          {
            "type": "command",
            "command": "enforce-uv-command --gemini-hook-json"
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
enforce-uv-command --configure-gemini-hook ~/.gemini/settings.json ~/.local/bin/enforce-uv-command
```

### Codex hook config

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
            "command": "~/.local/bin/enforce-uv-command --codex-hook-json"
          }
        ]
      }
    ]
  }
}
```

Project-local prompt guidance remains optional:

```text
./AGENTS.md
```

Automatic configuration:

```sh
mkdir -p ~/.codex
[ -f ~/.codex/hooks.json ] || printf '{}\n' > ~/.codex/hooks.json
enforce-uv-command --configure-codex-hook ~/.codex/hooks.json ~/.local/bin/enforce-uv-command
```

## Architecture

```text
                    install.sh
                        |
        +---------------+----------------+
        |               |                |
        v               v                v
  cargo build     hook config     Codex hook config
        |               |                |
        v               v                v
 ~/.local/bin/   ~/.claude/ or      ~/.codex/
 enforce-uv-command   ~/.gemini/     config.toml + hooks.json


 Agent shell request / hook payload
                |
                v
      enforce-uv-command
                |
     +----------+-----------+
     |                      |
     v                      v
 known-safe uv         bare python/pip
     |                      |
   allow                block + explain
                              |
                              v
                   exact rewrite or likely alternatives
```

## Troubleshooting

### `cargo not found`

The installer builds from source. Install Rust first:

```sh
curl https://sh.rustup.rs -sSf | sh
```

### `uv not found`

`force_uv` enforces `uv`-first workflows; it does not install `uv` for you. Install `uv` first:

```sh
curl -LsSf https://astral.sh/uv/install.sh | sh
```

### The installer ran, but the hook is not firing

Most often, the agent session was already running. Restart Claude Code, Gemini CLI, or Codex after installation so the new hook state is loaded.

### Claude Code was detected, but no hook was added

Automatic Claude setup only runs when `~/.claude/settings.json` already exists. Create the file if needed, add the JSON snippet from this README, then restart the session.

### Gemini CLI settings look empty

This is expected on first install. If `~/.gemini/` exists but `settings.json` does not, the installer creates it and then adds the `BeforeTool` hook entry.

### Codex hook exists but does not fire

Make sure `~/.codex/config.toml` has `[features]` with `codex_hooks = true`, and restart the running Codex session after editing `~/.codex/hooks.json`.

### The suggestion uses `uv pip`, but I wanted `uv add`

That usually means the original command was ambiguous. For dependency changes that belong in project metadata, choose `uv add` or `uv remove`. For pip-style environment changes, choose `uv pip`.

## Limitations

- This tool evaluates shell commands, not prose instructions or file edits.
- It does not auto-rewrite and re-run the command; it blocks and explains.
- The shell parsing is deliberately narrow and practical, not a full shell AST.
- Automatic hook setup exists for Claude Code, Gemini CLI, and Codex.
- Codex hooks are experimental and currently disabled on Windows.
- In Codex, this policy currently applies to direct `Bash` tool invocations. It can block `python -m pytest`, but it does not inspect script contents that are written to disk and later executed with `bash`.
- The installer is a Bash script and assumes a Unix-like environment with Cargo available.
- There is no package-manager or prebuilt-binary distribution path yet; source build is the supported installation path.

## FAQ

### Does this replace `python -m pytest` with `uv run pytest`?

Not today. The current exact rewrite preserves the original command shape and outputs `uv run python -m pytest`.

### Why does `pip install requests` return two different answers?

Because the command is ambiguous. It might mean "add this dependency to the project" or "mutate the current environment." `force_uv` shows both reasonable `uv` paths and lets the user choose.

### Does it understand wrapper commands like `sudo`, `env`, or `nohup`?

Yes. The parser explicitly preserves common wrappers such as `sudo`, `env`, `command`, `time`, `nohup`, and `builtin`.

### Will this modify my `pyproject.toml` or `uv.lock` automatically?

No. The tool never edits project metadata. It only blocks commands and suggests safer replacements.

### What happens when the command is already `uv`-based?

Known-safe `uv` subcommands such as `run`, `add`, `remove`, `sync`, `pip`, `python`, `venv`, and related commands pass through unchanged.

### Does this stop Codex from writing a shell script that runs `python` later?

No. In Codex, the hook currently inspects the direct `Bash` command string before execution. It can block `python -m pytest`, but if Codex writes a script file and later runs `bash script.sh`, the hook sees `bash script.sh`, not the script body. That is why this README describes Codex support as a guardrail rather than a complete enforcement boundary.

### Can I inspect what the installer would do before running it?

Yes:

```sh
curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_uv/main/install.sh \
  | bash -s -- --dry-run
```

### If Codex hooks are enabled, do I still need `AGENTS.md`?

Not for shell enforcement. Keep `AGENTS.md` only if you also want prompt-level written guidance in the repo before a shell command is ever attempted.

## License

MIT. See [LICENSE](LICENSE).
