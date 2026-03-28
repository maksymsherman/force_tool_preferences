# Force rg

Use `rg` (ripgrep) instead of `grep` for all text searching in this repo.

## Rules

- Never run `grep`, `egrep`, or `fgrep`.
- Use `rg` for text search, pattern matching, and file filtering.
- Prefer the least invasive exact `rg` rewrite when translating a `grep` command.
- If a `grep` flag does not have a guaranteed direct `rg` translation, check `rg --help` and translate it manually instead of guessing.
- Use `rg -F` for fixed-string searches.

## Performance

The matcher runs in nanoseconds. End-to-end hook cost is ~12-15ms per invocation, dominated by process startup — not the evaluator. Benchmark matcher-only cost with `--benchmark-command`.
