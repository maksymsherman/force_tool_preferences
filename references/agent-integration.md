# Agent Integration

Use this file when installing the skill into a specific agent environment.

## Shared design

- Keep `SKILL.md` as the canonical workflow.
- Use agent-specific context files only to make the same policy discoverable in the format that agent expects.
- Build `target/release/enforce-uv-command` with `cargo build --release`, then use that binary when the agent supports shell-command interception.

## Codex / OpenAI

- `SKILL.md` contains the skill instructions.
- `agents/openai.yaml` provides UI metadata for OpenAI-compatible skill discovery.
- `AGENTS.md` gives repo-local instructions to agents that honor project context files.

## Claude Code

- `CLAUDE.md` imports the shared guidance.
- Anthropic documents both project memory files (`CLAUDE.md`) and `PreToolUse` hooks for the Bash tool.
- Example project-level hook snippet:

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          {
            "type": "command",
            "command": "\"$CLAUDE_PROJECT_DIR/target/release/enforce-uv-command\" --claude-hook-json"
          }
        ]
      }
    ]
  }
}
```

- The compiled policy binary blocks bare `python`, `python3`, `pip`, `pip3`, and `uv init`, then returns a machine-readable reason that tells Claude what to do instead.

## Gemini

- `GEMINI.md` provides repo-local instructions for Gemini CLI.
- `gemini-extension.json` packages the context files in a format that matches Gemini CLI extensions built around `GEMINI.md`.
- This repo does not ship a Gemini hook config because Gemini's hook surface is less stable across releases than Claude's documented `PreToolUse` hooks. The same policy binary is still reusable if you standardize on a Gemini hook contract later.

## Source material

- Anthropic `CLAUDE.md` memory docs: <https://docs.anthropic.com/en/docs/claude-code/memory>
- Anthropic hooks reference: <https://docs.anthropic.com/en/docs/claude-code/hooks>
- Gemini CLI repository and `GEMINI.md` context convention: <https://github.com/google-gemini/gemini-cli>
- Astral `uv` docs: <https://docs.astral.sh/uv/>
