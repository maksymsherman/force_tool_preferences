# force_tool_preferences

Combined Claude Code PreToolUse hook that enforces preferred CLI tools in a single process, replacing the per-tool hooks (`force_rg`, `force_uv`) with one data-driven script.

## Motivation

Each PreToolUse hook costs ~3-4ms of fork/exec overhead. With 10+ individual hooks this adds up to 40-60ms per Bash invocation. A single combined hook pays that cost once (~4ms) regardless of how many rules it checks.

## Current rules to consolidate

- `grep` -> `rg` (ripgrep) — from `force_rg`
- `pip`/`python` -> `uv` — from `force_uv`

## Future rules

- `npm`/`npx`/`node` -> `bun` (`bun install`, `bun run`, `bunx`, `bun`). This rule should be easily bypassable since bun doesn't have 100% Node.js API compatibility — some native modules or Node-specific APIs may require falling back to node/npm.

## Design

Should follow the same Rust CLI pattern as `force_rg` and `force_uv`. Accepts `--claude-hook-json` on stdin, checks the command against an internal rule map, and exits with an error message suggesting the preferred alternative when a match is found.
