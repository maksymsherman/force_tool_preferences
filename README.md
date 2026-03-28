# force_rg

```text
grep      egrep      fgrep
  \         |         /
   \        |        /
    +---- force_rg ----+
             |
             v
             rg
```

[![Rust 2021](https://img.shields.io/badge/Rust-2021-orange?logo=rust)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)
[![Agents](https://img.shields.io/badge/Agents-Claude%20Code%20%7C%20Gemini%20%7C%20Codex-blue)](#installation)

Conservative grep-to-rg enforcement for coding agents. `force_rg` blocks `grep`, `egrep`, and `fgrep`, suggests the least invasive exact `rg` rewrite when the mapping is clear, and refuses to guess when it is not.

Quick install:

```sh
curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_rg/main/install.sh | bash
```

This installer builds `enforce-rg-command`, installs it to `~/.local/bin/`, configures Claude Code and Gemini hooks when possible, and copies the Codex skill to `${CODEX_HOME:-~/.codex}/skills/force-rg`. It requires `cargo` and `rg`.

## TL;DR

### The Problem

Telling agents "use `rg` instead of `grep`" works until it does not. They fall back to `grep`, use `egrep` out of habit, or copy old shell snippets with flags that do not map cleanly to `rg`.

### The Solution

`force_rg` sits in front of shell execution and turns grep-family usage into an explicit decision point:

- `rg` and unrelated commands pass through.
- `grep`/`egrep`/`fgrep` get blocked.
- Clear flag mappings get an exact `rg` suggestion.
- Unclear mappings get blocked with a "translate manually" message.

### Why Use `force_rg`?

| Feature | What it does | Why it matters |
|---|---|---|
| Exact rewrites | Drops only redundant flags like `-r`, `-n`, and `-E` when `rg` already defaults to them | Keeps suggestions minimal and predictable |
| Conservative blocking | Refuses to translate uncertain flags like `-s`, `-h`, `-L`, or `--include` | Prevents subtle behavior drift |
| Wrapper-aware detection | Catches `grep` behind `sudo`, `env`, `time`, `nohup`, shell assignments, pipes, and chained commands | Works in real shell usage, not just toy examples |
| Hook integration | Consumes Claude/Gemini hook JSON, emits JSON for Claude, and uses exit-status blocking for Gemini | Fits agent workflows directly |
| Policy file distribution | Ships `AGENTS.md`, `GEMINI.md`, and `SKILL.md` alongside the binary | Covers both hard enforcement and soft guidance |

Common outcomes:

| Input | Result |
|---|---|
| `grep -rn pattern .` | `rg pattern .` |
| `grep -E 'foo|bar' file.txt` | `rg 'foo|bar' file.txt` |
| `fgrep literal file.txt` | `rg -F literal file.txt` |
| `sudo grep -rl TODO /var/log` | `sudo rg -l TODO /var/log` |
| `grep -s pattern file.txt` | blocked for manual translation |

## Quick Example

```sh
# Inspect the installer without executing anything.
curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_rg/main/install.sh | bash -s -- --dry-run

# Install the binary, hooks, and Codex skill.
curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_rg/main/install.sh | bash

# Exact rewrite with a redundant grep command.
enforce-rg-command --command 'grep -rn TODO .'

# Fixed-string rewrite from fgrep.
enforce-rg-command --command 'fgrep literal README.md'

# Unsafe flag: blocked without guessing.
enforce-rg-command --command 'grep -s TODO README.md'

# Claude-style hook JSON input.
printf '%s' '{"tool_input":{"command":"grep -rn TODO ."}}' \
  | enforce-rg-command --claude-hook-json

# Benchmark matcher overhead locally.
enforce-rg-command --benchmark-command 'grep -rn TODO .' --iterations 100000
```

Representative outputs:

```text
Suggested replacement:
  rg TODO .
```

```text
Flags requiring manual translation before switching to rg:
  -s
```

```json
{"decision":"block","reason":"Use rg (ripgrep) instead of grep in this project. Replace blocked grep commands with the least invasive exact rg rewrite when the flag mapping is clear. If a flag does not have a guaranteed direct rg translation, translate it manually instead of guessing.\nSuggested replacement:\n  rg TODO ."}
```

## Design Philosophy

### 1. Smallest Correct Rewrite

If `rg` already implies a grep flag, `force_rg` drops it instead of carrying redundant noise forward. `grep -rn pattern .` becomes `rg pattern .`, not `rg -rn pattern .`.

### 2. Never Guess on Semantics

The tool only rewrites flags with a high-confidence mapping. Anything uncertain is blocked and surfaced explicitly so the human or agent can check `rg --help`.

### 3. Work With Real Shell Commands

Detection is not limited to a single bare command. The matcher handles full paths like `/bin/grep`, wrappers like `sudo` and `env`, and grep usage inside pipes or chained commands.

### 4. Separate Enforcement From Guidance

The Rust binary handles hard blocking and JSON hook integration. The shipped `AGENTS.md`, `GEMINI.md`, and `SKILL.md` files explain the broader "use `rg` unless structure matters, then use `ast-grep`" policy.

## Performance

`force_rg` is meant to stay enabled in the hook path, so there are two different numbers worth understanding:

1. The matcher cost inside an already-running process
2. The actual wall time of invoking the hook as a subprocess

### Matcher Cost

Sample measurements from the built-in benchmark mode on one machine with a release build of `enforce-rg-command`:

| Case | Example input | Average per evaluation |
|---|---|---:|
| Allowed command | `rg TODO .` | `0.0562 us` (`0.0000000562 s`) |
| Blocked command | `grep -rn TODO .` | `0.1438 us` (`0.0000001438 s`) |

These numbers measure only the command evaluator inside the binary.

Reproduce locally:

```sh
cargo build --release
./target/release/enforce-rg-command \
  --benchmark-command 'rg TODO .' \
  --iterations 5000000

./target/release/enforce-rg-command \
  --benchmark-command 'grep -rn TODO .' \
  --iterations 5000000
```

### Practical Hook Wall Time

Warm-cache sample measurements from one machine, estimated by batching 200 invocations and dividing the total wall time back down to a per-hook figure:

| Path | Example input | Approximate wall time per hook |
|---|---|---:|
| Direct CLI allow | `--command 'rg TODO .'` | `3.7 ms` |
| Direct CLI block | `--command 'grep -rn TODO .'` | `3.7 ms` |
| Stdin command block | `--stdin-command` with `grep -rn TODO .` | `4.0 ms` |
| Claude hook JSON block | `--claude-hook-json` with `tool_input.command` | `4.8 ms` |

That practical number is what matters in agent use. The gap between the microbenchmark and the real hook cost comes from shell startup, process startup, CLI parsing, stdin reads, JSON parsing, output formatting, and pipe plumbing. Results will vary across machines, but the overall shape is the same: the evaluator itself is effectively free, while the subprocess hook architecture costs low single-digit milliseconds per invocation.

## Comparison

| Approach | Enforces at shell boundary | Suggests exact rewrites | Blocks unsafe flag guesses | Agent-aware docs included |
|---|---|---|---|---|
| `force_rg` | Yes | Yes | Yes | Yes |
| `AGENTS.md` only | No | No | No | Depends on agent compliance |
| Shell alias like `alias grep='rg'` | Partially | No | No | No |
| Manual review after the fact | No | Maybe | Maybe | No |

Notes:

- A shell alias can silently change behavior instead of explaining the translation.
- Plain repo instructions are useful, but they are advisory unless your agent actually respects them.
- `force_rg` is opinionated about safety: blocked is better than a wrong rewrite.

## Installation

### Option 1: Automated installer

```sh
curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_rg/main/install.sh | bash
```

Useful variants:

```sh
curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_rg/main/install.sh | bash -s -- --dry-run
curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_rg/main/install.sh | bash -s -- --check-binary-hash
curl -fsSL https://raw.githubusercontent.com/maksymsherman/force_rg/main/install.sh | bash -s -- --check-binary-hash --overwrite-binary
```

What the installer does:

1. Clones the repo into a temporary directory.
2. Builds `enforce-rg-command` in release mode.
3. Compares SHA-256 hashes with any installed binary.
4. Installs or updates `~/.local/bin/enforce-rg-command`.
5. Configures Claude Code and Gemini hooks when their config directories are present.
6. Copies `SKILL.md` into `${CODEX_HOME:-~/.codex}/skills/force-rg`.

### Option 2: `cargo install`

```sh
git clone https://github.com/maksymsherman/force_rg.git
cd force_rg
cargo install --path . --bin enforce-rg-command
```

This installs the binary but does not configure hooks or copy the Codex skill automatically.

### Option 3: Build and copy manually

```sh
git clone https://github.com/maksymsherman/force_rg.git
cd force_rg
cargo build --release
mkdir -p ~/.local/bin
cp target/release/enforce-rg-command ~/.local/bin/
```

Then configure hooks yourself:

```sh
mkdir -p ~/.gemini
[ -f ~/.gemini/settings.json ] || printf '{}\n' > ~/.gemini/settings.json
enforce-rg-command --configure-gemini-hook ~/.gemini/settings.json enforce-rg-command
```

For Claude Code, either create `~/.claude/settings.json` first or add the hook snippet manually from the configuration section below.

### Option 4: Policy files only

If you do not want the binary yet, you can still distribute the agent guidance files:

```sh
git clone https://github.com/maksymsherman/force_rg.git ~/.codex/skills/force-rg
```

Or copy the repo-local guidance files into another project:

```sh
cp AGENTS.md /path/to/project/AGENTS.md
cp GEMINI.md /path/to/project/GEMINI.md
```

This gives agents the policy text, but it is not equivalent to shell-hook enforcement.

## Quick Start

1. Run the installer dry-run if you want to inspect the exact plan first.
2. Install the tool with the one-line installer or build it from source.
3. Restart any running Claude, Gemini, or Codex sessions so new hooks and skills load.
4. Verify that `rg` passes and `grep` gets blocked:

```sh
enforce-rg-command --command 'rg TODO .'
enforce-rg-command --command 'grep -rn TODO .'
enforce-rg-command --command 'grep -s TODO .'
```

5. Add `AGENTS.md` or `GEMINI.md` to projects where you also want repo-local written guidance.

## Command Reference

The binary is named `enforce-rg-command`.

| Command | Purpose | Example |
|---|---|---|
| `--command <text>` | Evaluate one shell command string directly | `enforce-rg-command --command 'grep -rn TODO .'` |
| `--stdin-command` | Read the command string from stdin | `printf '%s\n' 'grep -rn TODO .' \| enforce-rg-command --stdin-command` |
| `--claude-hook-json` | Read Claude hook JSON from stdin and emit JSON block decisions | `printf '%s' '{"tool_input":{"command":"grep -rn TODO ."}}' \| enforce-rg-command --claude-hook-json` |
| `--gemini-hook-json` | Read Gemini hook JSON from stdin and emit text or success exit status | `printf '%s' '{"tool_input":{"command":"grep -rn TODO ."}}' \| enforce-rg-command --gemini-hook-json` |
| `--benchmark-command <text>` | Run the matcher repeatedly and print timing stats | `enforce-rg-command --benchmark-command 'grep -rn TODO .' --iterations 100000` |
| `--configure-claude-hook <settings> <binary>` | Add the Claude hook entry to a settings file | `enforce-rg-command --configure-claude-hook ~/.claude/settings.json enforce-rg-command` |
| `--configure-gemini-hook <settings> <binary>` | Add the Gemini hook entry to a settings file | `enforce-rg-command --configure-gemini-hook ~/.gemini/settings.json enforce-rg-command` |

Exit codes:

| Code | Meaning |
|---|---|
| `0` | Command allowed, benchmark/configuration succeeded, or JSON block output was emitted successfully |
| `2` | Grep-family command blocked in plain-text mode |
| `1` | Invalid arguments, JSON parse failure, or file/config error |

What gets rewritten:

- `grep`, `egrep`, and `fgrep`
- full-path forms like `/usr/bin/grep`
- wrapper forms like `sudo grep ...` and `env FOO=1 grep ...`
- grep inside pipes and chained commands

What gets preserved when the mapping is clear:

- common matching flags like `-i`, `-v`, `-w`, `-x`, `-l`, `-c`, `-o`, `-q`
- context flags like `-A`, `-B`, `-C`
- pattern and file flags like `-e`, `-f`, `-m`
- color flags like `--color=auto` and `--colour=always` (normalized to `--color=...`)

What gets dropped because `rg` already does it:

- `-r`
- `-n`
- `-E`
- `--recursive`
- `--line-number`
- `--extended-regexp`

What gets blocked for manual translation:

- unsupported or uncertain flags such as `-s`, `-h`, `-L`, and `--include=...`

## Configuration

### Claude Code

Claude uses `PreToolUse` hooks with the `Bash` matcher.

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
            "command": "enforce-rg-command --claude-hook-json"
          }
        ]
      }
    ]
  }
}
```

Automatic configuration:

```sh
enforce-rg-command --configure-claude-hook ~/.claude/settings.json enforce-rg-command
```

### Gemini CLI

Gemini uses `BeforeTool` hooks with the `run_shell_command` matcher.

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
            "command": "enforce-rg-command --gemini-hook-json"
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
enforce-rg-command --configure-gemini-hook ~/.gemini/settings.json enforce-rg-command
```

### Codex

Codex uses a skill file, not a shell hook entry.

Global install:

```sh
git clone https://github.com/maksymsherman/force_rg.git ~/.codex/skills/force-rg
```

Installer target:

```text
${CODEX_HOME:-~/.codex}/skills/force-rg/SKILL.md
```

Project-local fallback:

```sh
cp AGENTS.md /path/to/project/AGENTS.md
```

## Architecture

```text
                         +-------------------+
                         |  AGENTS.md        |
                         |  GEMINI.md        |
                         |  SKILL.md         |
                         +---------+---------+
                                   |
                                   v
+-------------+         +----------+-----------+         +-------------------+
| Agent shell  +--------> enforce-rg-command   +---------> allow command      |
| invocation   | JSON    | tokenizes shell text |  none   | exit 0            |
| or hook      | or text | finds grep-family    |         +-------------------+
+------+------+         | usage in context      |
       |                +----------+-----------+
       |                            |
       | grep-family found          |
       v                            v
+------+------+         +-----------+----------+
| exact mapping |-------> suggested `rg` rewrite|
| available     | text    | plain text or JSON  |
+-------------+-+         | block response      |
              |           +-----------+---------+
              |                       |
              | no safe mapping       v
              +-----------------> manual translation
                                   required
```

Data flow summary:

1. The agent invokes a shell command or hook.
2. `enforce-rg-command` tokenizes the command and detects grep-family usage.
3. If the rewrite is safe, it blocks with an exact `rg` suggestion.
4. If the flags are uncertain, it blocks and names the flags that need manual translation.
5. If the command already uses `rg` or is unrelated, it returns success.

## Troubleshooting

### `cargo not found`

The installer builds from source. Install Rust first:

```sh
curl https://sh.rustup.rs -sSf | sh
```

### `rg not found`

The installer expects ripgrep to exist already.

Check:

```sh
rg --version
```

Then install ripgrep using your system package manager before re-running the installer.

### Claude Code was detected, but hooks were not configured

The installer only auto-updates Claude when `~/.claude/settings.json` already exists. Create the file and run:

```sh
enforce-rg-command --configure-claude-hook ~/.claude/settings.json enforce-rg-command
```

### Gemini hook exists but does not fire

Make sure you restarted the running Gemini session after editing `~/.gemini/settings.json`. Existing sessions will usually keep old hook state.

### A grep command was blocked without a replacement

That is expected for uncertain flags. Re-run the command after translating the flagged options manually against `rg --help`.

Example:

```sh
enforce-rg-command --command 'grep -s pattern file.txt'
rg --help
```

## Limitations

- This tool only targets `grep`, `egrep`, and `fgrep`. It does not enforce `rg` over `awk`, `sed`, `find`, or other search patterns.
- It does not auto-execute the suggested `rg` replacement. It blocks and explains instead.
- Only high-confidence flag mappings are rewritten. Many grep flags are intentionally left unsupported.
- Automatic hook configuration exists for Claude Code and Gemini. Codex integration is via skill files, not hook patching.
- The main install path builds locally. There are no prebuilt binaries or package-manager releases documented in this repo today.

## FAQ

### Does `force_rg` rewrite the command automatically?

No. It blocks and prints the exact replacement when safe. That keeps the decision visible and avoids silently changing shell behavior.

### Will it allow `rg` itself?

Yes. `rg` and `ripgrep` pass through unchanged.

### Does it catch `grep` inside pipes or chained commands?

Yes. Commands like `cat file.txt | grep pattern` and `cd /tmp && grep -rn TODO .` are still detected and blocked.

### Why ship `AGENTS.md`, `GEMINI.md`, and `SKILL.md` if the binary already exists?

Because enforcement and instruction solve different problems. The binary blocks shell commands. The policy files teach agents when to use `rg` and when `ast-grep` is the better tool.

### Can I use just the Codex skill without installing the binary?

Yes. Cloning the repo into `~/.codex/skills/force-rg` gives Codex the policy text, but it does not enforce shell-hook blocking.

### Does this support `ast-grep`?

The binary does not parse or enforce `ast-grep`. The shipped policy files explain when to prefer `ast-grep` for structural code matches.

### Is Windows supported?

The command matcher understands program names like `grep.exe`, but the documented installer is a Bash script and the repo currently documents Unix-like install flows.

## License

MIT. See [LICENSE](LICENSE).
