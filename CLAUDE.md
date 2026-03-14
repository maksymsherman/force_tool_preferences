# Force uv

@SKILL.md
@references/uv-quick-reference.md

## Claude-specific note

- If shell hooks are enabled, wire `target/release/enforce-uv-command --claude-hook-json` into a `PreToolUse` hook for the Bash tool so bare `python`, `python3`, `pip`, and `pip3` commands are blocked before execution.
