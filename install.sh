#!/usr/bin/env bash
set -euo pipefail

REPO="https://github.com/maksymsherman/force_uv.git"
INSTALL_DIR="${HOME}/.local/bin"
BINARY_NAME="enforce-uv-command"

info()  { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
ok()    { printf '\033[1;32m==>\033[0m %s\n' "$*"; }
warn()  { printf '\033[1;33m==>\033[0m %s\n' "$*"; }
err()   { printf '\033[1;31m==>\033[0m %s\n' "$*" >&2; }

# --- check prerequisites ---

if ! command -v cargo &>/dev/null; then
  err "cargo not found. Install Rust first: https://rustup.rs"
  exit 1
fi

if ! command -v uv &>/dev/null; then
  err "uv not found. Install uv first: https://docs.astral.sh/uv/"
  exit 1
fi

# --- build ---

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

info "Cloning force_uv..."
git clone --depth 1 "$REPO" "$TMPDIR/force_uv" 2>/dev/null

info "Building..."
(cd "$TMPDIR/force_uv" && cargo build --release --quiet)

# --- install binary ---

mkdir -p "$INSTALL_DIR"
cp "$TMPDIR/force_uv/target/release/$BINARY_NAME" "$INSTALL_DIR/$BINARY_NAME"
chmod +x "$INSTALL_DIR/$BINARY_NAME"
ok "Installed $INSTALL_DIR/$BINARY_NAME"

# --- configure Claude Code ---

CLAUDE_SETTINGS="${HOME}/.claude/settings.json"
if [ -d "${HOME}/.claude" ]; then
  info "Detected Claude Code"

  if [ -f "$CLAUDE_SETTINGS" ]; then
    uv --directory "$TMPDIR/force_uv" run --isolated python - "$CLAUDE_SETTINGS" "$BINARY_NAME" <<'PYEOF'
import json, sys, os

settings_path = sys.argv[1]
binary = sys.argv[2]
hook_command = f"{binary} --claude-hook-json"

with open(settings_path) as f:
    settings = json.load(f)

hooks = settings.setdefault("hooks", {})
pre = hooks.setdefault("PreToolUse", [])

# find existing Bash matcher or create one
bash_entry = None
for entry in pre:
    if entry.get("matcher") == "Bash":
        bash_entry = entry
        break

if bash_entry is None:
    bash_entry = {"matcher": "Bash", "hooks": []}
    pre.append(bash_entry)

hook_list = bash_entry.setdefault("hooks", [])

# check if already installed
for h in hook_list:
    if hook_command in h.get("command", ""):
        print(f"  Claude Code hook already configured", flush=True)
        sys.exit(0)

hook_list.append({"type": "command", "command": hook_command})

with open(settings_path, "w") as f:
    json.dump(settings, f, indent=2)
    f.write("\n")

print(f"  Added PreToolUse hook to {settings_path}", flush=True)
PYEOF
    ok "Claude Code configured"
  else
    warn "Claude Code detected but could not configure hooks automatically"
    warn "Add this to $CLAUDE_SETTINGS manually:"
    cat <<EOF
  "hooks": {
    "PreToolUse": [{
      "matcher": "Bash",
      "hooks": [{"type": "command", "command": "$BINARY_NAME --claude-hook-json"}]
    }]
  }
EOF
  fi
fi

# --- configure Gemini CLI ---

GEMINI_SETTINGS="${HOME}/.gemini/settings.json"
if [ -d "${HOME}/.gemini" ]; then
  info "Detected Gemini CLI"

  [ -f "$GEMINI_SETTINGS" ] || echo '{}' > "$GEMINI_SETTINGS"

  uv --directory "$TMPDIR/force_uv" run --isolated python - "$GEMINI_SETTINGS" "$BINARY_NAME" <<'PYEOF'
import json, sys

settings_path = sys.argv[1]
binary = sys.argv[2]
hook_command = f"{binary} --gemini-hook-json"

with open(settings_path) as f:
    settings = json.load(f)

hooks = settings.setdefault("hooks", {})
before = hooks.setdefault("BeforeTool", [])

# find existing run_shell_command matcher or create one
shell_entry = None
for entry in before:
    if entry.get("matcher") == "run_shell_command":
        shell_entry = entry
        break

if shell_entry is None:
    shell_entry = {"matcher": "run_shell_command", "hooks": []}
    before.append(shell_entry)

hook_list = shell_entry.setdefault("hooks", [])

for h in hook_list:
    if hook_command in h.get("command", ""):
        print(f"  Gemini CLI hook already configured", flush=True)
        sys.exit(0)

hook_list.append({"type": "command", "command": hook_command})

with open(settings_path, "w") as f:
    json.dump(settings, f, indent=2)
    f.write("\n")

print(f"  Added BeforeTool hook to {settings_path}", flush=True)
PYEOF
  ok "Gemini CLI configured"
fi

# --- install Codex skill ---

CODEX_HOME_DIR="${CODEX_HOME:-$HOME/.codex}"
CODEX_SKILLS="$CODEX_HOME_DIR/skills"
SKILL_DIR="$CODEX_SKILLS/force-uv"
mkdir -p "$SKILL_DIR"
cp "$TMPDIR/force_uv/SKILL.md" "$SKILL_DIR/SKILL.md"
ok "Codex skill installed to $SKILL_DIR"

# --- done ---

echo ""
ok "force_uv installed!"
echo "  Restart any running agent sessions for hooks to take effect."
