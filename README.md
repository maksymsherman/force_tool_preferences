# force_uv

`force_uv` is a small Rust policy binary plus agent instruction files that push coding agents toward `uv` for Python work.

It does two things:

- gives Codex, Claude Code, and Gemini a shared "use `uv`, not bare `python` or `pip`" policy
- provides a fast hook binary for agents that support pre-execution shell interception

The binary blocks:

- bare `python`, `python3`, `pip`, and `pip3`
- `uv init` in an existing repository unless the user explicitly asks for project creation or conversion

The binary allows:

- `uv run ...`
- `uv add ...`
- `uv add --dev ...`
- `uv remove ...`
- `uv sync`
- `uvx ...`

## Compatibility

| Agent | Repo-local instructions | Mechanical command blocking | Status |
| --- | --- | --- | --- |
| Claude Code | `CLAUDE.md` | Yes | Supported in this repo |
| Codex | `AGENTS.md` and `SKILL.md` | No | Instruction-level only |
| Gemini CLI | `GEMINI.md` | Not shipped here | Context-first only |

Codex does not currently expose a pre-execution shell-hook surface in the environment this repo targets, so its installation path is skill/context based rather than hook based.

Gemini CLI support in this repo is intentionally conservative. `GEMINI.md` is included, but this repository does not currently ship a maintained Gemini hook parser because Gemini's hook payload and configuration surface have been less stable than Claude Code's.

## Prerequisites

Before installing `force_uv`, make sure you have:

- Rust and Cargo available on your `PATH`
- `uv` installed and available on your `PATH`
- the target agent already installed

Install `uv` using Astral's current instructions:

- Main docs: [docs.astral.sh/uv](https://docs.astral.sh/uv/)
- Installation guide: [docs.astral.sh/uv/getting-started/installation](https://docs.astral.sh/uv/getting-started/installation/)
- Agent-friendly docs index: [docs.astral.sh/uv/llms.txt](https://docs.astral.sh/uv/llms.txt)

## Quick Start

Clone the repository and build the release binary:

```bash
git clone https://github.com/maksymsherman/force_uv.git
cd force_uv
cargo build --release
cargo test
```

The release artifact is:

```bash
./target/release/enforce-uv-command
```

Run a quick smoke test:

```bash
./target/release/enforce-uv-command --command 'uv run pytest'
echo $?

./target/release/enforce-uv-command --command 'python -m pytest'
echo $?
```

Expected behavior:

- `uv run pytest` exits `0`
- `python -m pytest` prints a block message and exits `2`

You can also test the Claude-style JSON path directly:

```bash
printf '{"tool_input":{"command":"python -m pytest"}}' \
  | ./target/release/enforce-uv-command --claude-hook-json
```

## Installation Modes

There are two main ways to use this repository:

1. Build the Rust binary and wire it into an agent that supports command hooks.
2. Use the instruction files directly so the agent sees the uv-first policy even when no hook API exists.

For most users:

- Claude Code: use both the binary and `CLAUDE.md`
- Codex: use `SKILL.md` and `AGENTS.md`
- Gemini CLI: use `GEMINI.md`

## Claude Code Installation

Claude Code is the primary mechanical-enforcement target in this repository.

### 1. Build the binary

```bash
git clone https://github.com/maksymsherman/force_uv.git
cd force_uv
cargo build --release
```

### 2. Add a `PreToolUse` hook

You can configure hooks with Claude Code's `/hooks` flow or by editing `~/.claude/settings.json` manually.

Use an absolute path to the compiled binary:

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          {
            "type": "command",
            "command": "/absolute/path/to/force_uv/target/release/enforce-uv-command --claude-hook-json"
          }
        ]
      }
    ]
  }
}
```

Notes:

- use an absolute path, not a relative path
- rebuild after updates with `cargo build --release`
- restart Claude Code after changing hook configuration

Current Claude Code hook docs:

- Hooks reference: [docs.anthropic.com/en/docs/claude-code/hooks](https://docs.anthropic.com/en/docs/claude-code/hooks)
- Hook setup guide: [docs.anthropic.com/en/docs/claude-code/hooks-guide](https://docs.anthropic.com/en/docs/claude-code/hooks-guide)

### 3. Add repo instructions

If the project already has a `CLAUDE.md`, merge the `force_uv` policy into it. If not, copy the policy from [CLAUDE.md](./CLAUDE.md).

At minimum, make sure the target repository tells Claude Code:

- never run bare `python`, `python3`, `pip`, or `pip3`
- prefer `uv run`, `uv add`, `uv add --dev`, `uv remove`, and `uv sync`
- never run `uv init` in an existing repo unless the user explicitly asks

### 4. Verify the install

Ask Claude Code to run a bare Python command in a test repository, for example:

```bash
python -m pytest
```

The hook should block it and tell Claude Code to use `uv` instead.

## Codex Installation

Codex support in this repository is skill-first, not hook-first.

### 1. Install as a reusable skill

If your Codex environment loads skills from `$CODEX_HOME/skills`, install this repository there:

```bash
mkdir -p "${CODEX_HOME:-$HOME/.codex}/skills"
git clone https://github.com/maksymsherman/force_uv.git "${CODEX_HOME:-$HOME/.codex}/skills/force-uv"
cd "${CODEX_HOME:-$HOME/.codex}/skills/force-uv"
cargo build --release
```

That gives Codex:

- [SKILL.md](./SKILL.md) as the reusable skill body
- [agents/openai.yaml](./agents/openai.yaml) as OpenAI skill metadata
- [AGENTS.md](./AGENTS.md) as repo-local guidance

### 2. Use repo-local instructions

If you want the policy in a specific project, merge the contents of [AGENTS.md](./AGENTS.md) into that project's own `AGENTS.md`.

The mechanical blocker binary can still be built in the skill repo, but this Codex environment does not expose a pre-execution command hook that this repository can wire automatically today.

### 3. Verify the install

In a Codex session, ask the agent to run a Python command in a uv-managed repository. The expected behavior is that Codex should choose a `uv`-based equivalent because of the repo instructions or installed skill.

## Gemini CLI Installation

This repository currently ships Gemini support as project context rather than a maintained command hook.

### 1. Add the policy context

If your target project already has a `GEMINI.md`, merge the `force_uv` policy into it. If it does not, start from [GEMINI.md](./GEMINI.md).

You can also keep this repo as a separate reference checkout and copy the policy into project-specific Gemini context files as needed.

### 2. Optional: keep the extension metadata nearby

This repository includes [gemini-extension.json](./gemini-extension.json) so the context files can travel together if you package them as a Gemini extension or local convention.

### 3. Verify the install

Start Gemini CLI in a project that has the merged `GEMINI.md` policy and ask it to run a Python command. It should prefer a `uv` equivalent.

Current Gemini CLI reference:

- Gemini CLI repository and docs hub: [github.com/google-gemini/gemini-cli](https://github.com/google-gemini/gemini-cli)

## Binary Usage

The compiled binary supports three practical modes.

### Evaluate a raw shell command

```bash
./target/release/enforce-uv-command --command 'python -m pytest'
```

### Evaluate a command from stdin

```bash
printf 'python -m pytest' \
  | ./target/release/enforce-uv-command --stdin-command
```

### Evaluate a Claude hook payload from stdin

```bash
printf '{"tool_input":{"command":"python -m pytest"}}' \
  | ./target/release/enforce-uv-command --claude-hook-json
```

### Run the built-in benchmark

```bash
./target/release/enforce-uv-command \
  --benchmark-command 'uv run pytest' \
  --iterations 5000000
```

This benchmark measures the matcher inside the process. It does not include shell startup or external hook-host overhead.

## Verification Checklist

After installation, verify all of the following:

- `cargo test` passes
- `cargo build --release` succeeds
- `./target/release/enforce-uv-command --command 'uv run pytest'` exits `0`
- `./target/release/enforce-uv-command --command 'python -m pytest'` exits `2`
- Claude Code blocks a bare Python command if you installed the hook
- the target agent still allows normal `uv` commands

## Updating

To update an existing installation:

```bash
cd /path/to/force_uv
git pull --ff-only
cargo build --release
cargo test
```

If you use Claude Code hooks, no config change is needed as long as the binary path stays the same.

When updating `uv` guidance, prefer the live Astral docs over stale screenshots or copied examples:

- [docs.astral.sh/uv/llms.txt](https://docs.astral.sh/uv/llms.txt)
- [docs.astral.sh/uv](https://docs.astral.sh/uv/)

## Uninstall

### Claude Code

Remove the hook entry from `~/.claude/settings.json`, then delete the checkout if you no longer need it:

```bash
rm -rf /path/to/force_uv
```

### Codex

Remove the installed skill directory or delete the merged instructions from the target project's `AGENTS.md`.

### Gemini CLI

Remove the merged `GEMINI.md` policy or delete the copied files from the target project.

## Repository Layout

```text
force_uv/
├── SKILL.md                      # reusable Codex skill
├── AGENTS.md                     # repo-local instructions for agents that read AGENTS.md
├── CLAUDE.md                     # Claude Code instructions
├── GEMINI.md                     # Gemini CLI instructions
├── agents/openai.yaml            # OpenAI/Codex skill metadata
├── references/                   # deeper guidance and references
├── src/main.rs                   # Rust command-policy binary
└── target/release/enforce-uv-command
```

## Development Notes

The matcher is intentionally small and fast.

- The in-process benchmark is microsecond-scale and typically sub-microsecond on this machine.
- End-to-end hook time is higher because process startup still costs milliseconds.
- The binary is meant to be deterministic and cheap enough for interactive command interception.

If you change the policy surface, update:

- [SKILL.md](./SKILL.md)
- [AGENTS.md](./AGENTS.md)
- [CLAUDE.md](./CLAUDE.md)
- [GEMINI.md](./GEMINI.md)
- [references/agent-integration.md](./references/agent-integration.md)

## License

MIT. See [LICENSE](./LICENSE).
