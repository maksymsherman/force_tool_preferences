# Force rg

Use `rg` (ripgrep) instead of `grep` for all text searching in this repo.

## Rules

- Never run `grep`, `egrep`, or `fgrep`.
- Use `rg` for text search, pattern matching, and file filtering.
- Prefer the least invasive exact `rg` rewrite when translating a `grep` command.
- If a `grep` flag does not have a guaranteed direct `rg` translation, check `rg --help` and translate it manually instead of guessing.
- Use `rg -F` for fixed-string searches.

## Hook Reality

The matcher itself is extremely fast, but the Claude hook path is not measured in microseconds. On a warm machine, the realistic cost of invoking the hook as a subprocess is roughly:

- `3.7 ms` for `--command 'rg TODO .'`
- `3.7 ms` for `--command 'grep -rn TODO .'`
- `4.0 ms` for `--stdin-command`
- `4.8 ms` for `--claude-hook-json`

That overhead comes mostly from process startup and hook plumbing, not from the grep matcher.

## If You Need To Make It Faster

- Do not spend much time micro-optimizing `evaluate_command()` until you have addressed per-invocation process startup.
- Extract the core matcher and rewrite logic into a library crate so the CLI and any future daemon share one implementation.
- Add a resident daemon that listens on a Unix socket and returns allow/block decisions, then keep the current CLI as a fallback shim.
- If Claude integration can ever call into a library directly instead of spawning a process, prefer that over a daemon because it removes IPC too.
- Keep JSON parsing and formatting thin. The expensive part should be policy evaluation, not hook protocol glue.
- Benchmark matcher-only cost with `--benchmark-command`.
- Benchmark end-to-end hook wall time with repeated real invocations.

## Suggested Direction

The most credible path is:

1. Move evaluation code into `src/lib.rs`.
2. Keep `enforce-rg-command` as the compatibility binary.
3. Add an optional long-lived daemon process for low-latency hook use.
4. Fall back to direct CLI evaluation when the daemon is unavailable.

That keeps installation simple while giving a real path to sub-millisecond decisions later.
