#!/usr/bin/env bash
set -euo pipefail

REPO="https://github.com/maksymsherman/force_tool_preferences.git"
INSTALL_DIR="${HOME}/.local/bin"
BINARY_NAME="enforce-tool-preferences-command"
CHECK_BINARY_HASH=0
OVERWRITE_BINARY=0
DRY_RUN=0

info()  { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
ok()    { printf '\033[1;32m==>\033[0m %s\n' "$*"; }
warn()  { printf '\033[1;33m==>\033[0m %s\n' "$*"; }
err()   { printf '\033[1;31m==>\033[0m %s\n' "$*" >&2; }

usage() {
  cat <<'EOF'
Usage: install.sh [--check-binary-hash] [--overwrite-binary] [--dry-run] [--help]

Options:
  --check-binary-hash  Print the SHA-256 hashes for the built and installed binary.
  --overwrite-binary   Force copying the built binary over the installed binary,
                       even when the hashes already match.
  --dry-run            Print the exact repo files, paths, and planned actions without
                       cloning, building, or writing anything.
  --help, -h           Show this help text.
EOF
}

hash_file() {
  local path="$1"

  if command -v sha256sum &>/dev/null; then
    sha256sum "$path" | awk '{print $1}'
    return
  fi

  if command -v shasum &>/dev/null; then
    shasum -a 256 "$path" | awk '{print $1}'
    return
  fi

  if command -v openssl &>/dev/null; then
    openssl dgst -sha256 "$path" | awk '{print $NF}'
    return
  fi

  err "No SHA-256 tool found. Install one of: sha256sum, shasum, or openssl."
  exit 1
}

enable_codex_hooks_in_config() {
  local config_path="$1"
  local config_dir
  local tmp

  config_dir="$(dirname "$config_path")"
  mkdir -p "$config_dir"

  if [ ! -f "$config_path" ]; then
    cat > "$config_path" <<'EOF'
[features]
codex_hooks = true
EOF
    return
  fi

  tmp="$(mktemp)"
  awk '
    function emit_codex_flag() {
      print "codex_hooks = true"
      codex_flag_written = 1
    }

    /^[[:space:]]*\[[^]]+\][[:space:]]*$/ {
      if (in_features && !codex_flag_written) {
        emit_codex_flag()
      }

      in_features = ($0 ~ /^[[:space:]]*\[features\][[:space:]]*$/)
      if (in_features) {
        features_section_seen = 1
      }

      print
      next
    }

    in_features && /^[[:space:]]*codex_hooks[[:space:]]*=/ {
      if (!codex_flag_written) {
        emit_codex_flag()
      }
      next
    }

    { print }

    END {
      if (in_features && !codex_flag_written) {
        emit_codex_flag()
      }

      if (!features_section_seen) {
        if (NR > 0) {
          print ""
        }
        print "[features]"
        print "codex_hooks = true"
      }
    }
  ' "$config_path" > "$tmp"
  mv "$tmp" "$config_path"
}

print_dry_run_plan() {
  local codex_home_dir="${CODEX_HOME:-$HOME/.codex}"

  info "Dry run only. No files will be written and no code will be executed."
  echo "Installer source:"
  echo "  $REPO/raw/main/install.sh"
  echo "Repo files this installer may execute after cloning:"
  echo "  install.sh"
  echo "  src/main.rs"
  echo "Planned actions:"
  echo "  1. git clone --depth 1 $REPO <tmp>/force_tool_preferences"
  echo "  2. (cd <tmp>/force_tool_preferences && cargo build --release --quiet)"
  echo "  3. Compare <tmp>/force_tool_preferences/target/release/$BINARY_NAME against $INSTALL_DIR/$BINARY_NAME"
  echo "  4. Install or update $INSTALL_DIR/$BINARY_NAME if needed"
  echo "  5. If Claude Code is present, update $HOME/.claude/settings.json via $BINARY_NAME --configure-claude-hook"
  echo "  6. If Gemini CLI is present, update $HOME/.gemini/settings.json via $BINARY_NAME --configure-gemini-hook"
  echo "  7. Ensure $codex_home_dir/config.toml enables codex_hooks = true"
  echo "  8. Update $codex_home_dir/hooks.json via $BINARY_NAME --configure-codex-hook"
  echo "Inspect locally before running:"
  echo "  git clone $REPO"
  echo "  cd force_tool_preferences"
  echo "  sed -n '1,260p' install.sh"
  echo "  sed -n '1,260p' src/main.rs"
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --check-binary-hash)
      CHECK_BINARY_HASH=1
      ;;
    --overwrite-binary)
      OVERWRITE_BINARY=1
      ;;
    --dry-run)
      DRY_RUN=1
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      err "unknown argument: $1"
      usage
      exit 1
      ;;
  esac
  shift
done

if [ "$DRY_RUN" -eq 1 ] && [ "$CHECK_BINARY_HASH" -eq 1 ]; then
  warn "--check-binary-hash is ignored during --dry-run because the build step is skipped."
fi

if [ "$DRY_RUN" -eq 1 ]; then
  print_dry_run_plan
  exit 0
fi

if ! command -v cargo &>/dev/null; then
  err "cargo not found. Install Rust first: https://rustup.rs"
  exit 1
fi

if ! command -v rg &>/dev/null; then
  err "rg not found. Install ripgrep first: https://github.com/BurntSushi/ripgrep#installation"
  exit 1
fi

if ! command -v uv &>/dev/null; then
  err "uv not found. Install uv first: https://docs.astral.sh/uv/"
  exit 1
fi

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

info "Cloning force_tool_preferences..."
git clone --depth 1 "$REPO" "$TMPDIR/force_tool_preferences" 2>/dev/null

info "Building..."
(cd "$TMPDIR/force_tool_preferences" && cargo build --release --quiet)

mkdir -p "$INSTALL_DIR"
SOURCE_BINARY="$TMPDIR/force_tool_preferences/target/release/$BINARY_NAME"
TARGET_BINARY="$INSTALL_DIR/$BINARY_NAME"
SOURCE_HASH="$(hash_file "$SOURCE_BINARY")"
TARGET_HASH=""

if [ -f "$TARGET_BINARY" ]; then
  TARGET_HASH="$(hash_file "$TARGET_BINARY")"
fi

if [ "$CHECK_BINARY_HASH" -eq 1 ]; then
  info "Built binary sha256:     $SOURCE_HASH"
  if [ -n "$TARGET_HASH" ]; then
    info "Installed binary sha256: $TARGET_HASH"
  else
    info "Installed binary sha256: <missing>"
  fi
fi

if [ ! -f "$TARGET_BINARY" ]; then
  cp "$SOURCE_BINARY" "$TARGET_BINARY"
  chmod +x "$TARGET_BINARY"
  ok "Installed $TARGET_BINARY"
elif [ "$OVERWRITE_BINARY" -eq 1 ]; then
  cp "$SOURCE_BINARY" "$TARGET_BINARY"
  chmod +x "$TARGET_BINARY"
  ok "Overwrote $TARGET_BINARY"
elif [ "$SOURCE_HASH" != "$TARGET_HASH" ]; then
  cp "$SOURCE_BINARY" "$TARGET_BINARY"
  chmod +x "$TARGET_BINARY"
  ok "Updated $TARGET_BINARY"
else
  ok "Binary already up to date at $TARGET_BINARY"
fi

CLAUDE_SETTINGS="${HOME}/.claude/settings.json"
if [ -d "${HOME}/.claude" ]; then
  info "Detected Claude Code"

  if [ -f "$CLAUDE_SETTINGS" ]; then
    "$SOURCE_BINARY" --configure-claude-hook "$CLAUDE_SETTINGS" "$TARGET_BINARY"
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

GEMINI_SETTINGS="${HOME}/.gemini/settings.json"
if [ -d "${HOME}/.gemini" ]; then
  info "Detected Gemini CLI"
  mkdir -p "${HOME}/.gemini"
  [ -f "$GEMINI_SETTINGS" ] || printf '{}\n' > "$GEMINI_SETTINGS"
  "$SOURCE_BINARY" --configure-gemini-hook "$GEMINI_SETTINGS" "$TARGET_BINARY"
  ok "Gemini CLI configured"
fi

CODEX_HOME_DIR="${CODEX_HOME:-$HOME/.codex}"
CODEX_CONFIG="$CODEX_HOME_DIR/config.toml"
CODEX_HOOKS="$CODEX_HOME_DIR/hooks.json"

mkdir -p "$CODEX_HOME_DIR"
[ -f "$CODEX_HOOKS" ] || printf '{}\n' > "$CODEX_HOOKS"
enable_codex_hooks_in_config "$CODEX_CONFIG"
"$SOURCE_BINARY" --configure-codex-hook "$CODEX_HOOKS" "$TARGET_BINARY"
ok "Codex configured"

echo ""
ok "force_tool_preferences installed!"
echo "  Restart any running agent sessions for hooks to take effect."
