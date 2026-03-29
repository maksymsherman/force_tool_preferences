#!/usr/bin/env bash
set -euo pipefail

REPO="https://github.com/maksymsherman/force_tool_preferences.git"
INSTALL_DIR="${HOME}/.local/bin"
BINARY_NAME="enforce-tool-preferences-command"
CHECK_BINARY_HASH=0
OVERWRITE_BINARY=0
DRY_RUN=0
LIST_RULES=0
RULES=""
EXACT_RULES=""
ENABLE_RULES=()
DISABLE_RULES=()

info()  { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
ok()    { printf '\033[1;32m==>\033[0m %s\n' "$*"; }
warn()  { printf '\033[1;33m==>\033[0m %s\n' "$*"; }
err()   { printf '\033[1;31m==>\033[0m %s\n' "$*" >&2; }

rule_manifest() {
  cat <<'EOF'
# BEGIN_SHARED_RULE_CATALOG
rg	ripgrep	grep-family -> rg enforcement	cargo,rg
uv	-	python/pip-family -> uv enforcement	cargo,uv
# END_SHARED_RULE_CATALOG
EOF
}

usage() {
  cat <<'EOF'
Usage: install.sh [--check-binary-hash] [--overwrite-binary] [--dry-run] [--list-rules] [--rules <rule[,rule...]>] [--enable-rule <name>] [--disable-rule <name>] [--help]

Options:
  --check-binary-hash  Print the SHA-256 hashes for the built and installed binary.
  --overwrite-binary   Force copying the built binary over the installed binary,
                       even when the hashes already match.
  --dry-run            Print the exact repo files, paths, and planned actions without
                       cloning, building, or writing anything.
  --list-rules         Show the supported rule families, aliases, and prerequisites.
  --rules              Set the exact enabled rule family list. Best for automation,
                       CI, and dotfiles that want a stable explicit selection.
  --enable-rule        Add one rule family by name. Repeat to build an exact subset.
                       If omitted, installation starts from all supported rules.
  --disable-rule       Remove one rule family by name. Repeat to subtract from the
                       default all-rules install or from a prior --enable-rule set.
  --only-rg            Compatibility alias for --rules rg.
  --only-uv            Compatibility alias for --rules uv.
  --help, -h           Show this help text.
EOF
}

each_rule_manifest_row() {
  local line

  while IFS= read -r line; do
    case "$line" in
      '# BEGIN_SHARED_RULE_CATALOG'|'# END_SHARED_RULE_CATALOG'|'')
        continue
        ;;
      \#*)
        continue
        ;;
      *)
        printf '%s\n' "$line"
        ;;
    esac
  done < <(rule_manifest)
}

format_csv_for_display() {
  local value="$1"
  local output=""
  local item
  local items=()

  if [ -z "$value" ] || [ "$value" = "-" ]; then
    printf '<none>\n'
    return
  fi

  IFS=',' read -r -a items <<< "$value"
  for item in "${items[@]}"; do
    [ -z "$item" ] && continue
    if [ -n "$output" ]; then
      output="$output, "
    fi
    output="$output$item"
  done

  if [ -z "$output" ]; then
    printf '<none>\n'
    return
  fi

  printf '%s\n' "$output"
}

canonicalize_rule_name() {
  local name="${1// /}"
  local cli_name aliases description prerequisites alias aliases_list=()

  while IFS=$'\t' read -r cli_name aliases description prerequisites; do
    if [ "$cli_name" = "$name" ]; then
      printf '%s\n' "$cli_name"
      return 0
    fi

    if [ -z "$aliases" ] || [ "$aliases" = "-" ]; then
      continue
    fi

    IFS=',' read -r -a aliases_list <<< "$aliases"
    for alias in "${aliases_list[@]}"; do
      if [ "$alias" = "$name" ]; then
        printf '%s\n' "$cli_name"
        return 0
      fi
    done
  done < <(each_rule_manifest_row)

  return 1
}

supported_rules_display() {
  local output=""
  local cli_name aliases description prerequisites

  while IFS=$'\t' read -r cli_name aliases description prerequisites; do
    if [ -n "$output" ]; then
      output="$output, "
    fi
    output="$output$cli_name"
  done < <(each_rule_manifest_row)

  printf '%s\n' "$output"
}

csv_contains_rule() {
  local csv="$1"
  local rule="$2"

  case ",$csv," in
    *",$rule,"*)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

csv_add_rule() {
  local csv="$1"
  local rule="$2"

  if [ -z "$csv" ]; then
    printf '%s\n' "$rule"
    return
  fi

  if csv_contains_rule "$csv" "$rule"; then
    printf '%s\n' "$csv"
    return
  fi

  printf '%s,%s\n' "$csv" "$rule"
}

csv_remove_rule() {
  local csv="$1"
  local rule="$2"
  local output=""
  local item
  local items=()

  IFS=',' read -r -a items <<< "$csv"

  for item in "${items[@]}"; do
    [ "$item" = "$rule" ] && continue
    output="$(csv_add_rule "$output" "$item")"
  done

  printf '%s\n' "$output"
}

supported_rules_csv() {
  local output=""
  local cli_name aliases description prerequisites

  while IFS=$'\t' read -r cli_name aliases description prerequisites; do
    output="$(csv_add_rule "$output" "$cli_name")"
  done < <(each_rule_manifest_row)

  printf '%s\n' "$output"
}

normalize_rules_csv() {
  local normalized
  local output=""
  local item
  local items=()

  IFS=',' read -r -a items <<< "$1"

  for item in "${items[@]}"; do
    [ -z "${item// /}" ] && continue

    if ! normalized="$(canonicalize_rule_name "$item")"; then
      return 1
    fi

    output="$(csv_add_rule "$output" "$normalized")"
  done

  if [ -z "$output" ]; then
    return 1
  fi

  printf '%s\n' "$output"
}

set_exact_rules() {
  local normalized

  if [ "${#ENABLE_RULES[@]}" -gt 0 ] || [ "${#DISABLE_RULES[@]}" -gt 0 ]; then
    err "cannot combine --rules with --enable-rule or --disable-rule"
    exit 1
  fi

  if ! normalized="$(normalize_rules_csv "$1")"; then
    err "invalid rule selection: $1"
    err "supported rule ids: $(supported_rules_display)"
    exit 1
  fi

  if [ -n "$EXACT_RULES" ] && [ "$EXACT_RULES" != "$normalized" ]; then
    err "multiple conflicting rule-selection flags provided"
    exit 1
  fi

  EXACT_RULES="$normalized"
}

append_rule_selection() {
  local mode="$1"
  local value="$2"
  local normalized

  if [ -n "$EXACT_RULES" ]; then
    err "cannot combine --rules with --${mode}-rule"
    exit 1
  fi

  if ! normalized="$(canonicalize_rule_name "$value")"; then
    err "unknown rule '$value'"
    err "supported rule ids: $(supported_rules_display)"
    exit 1
  fi

  if [ "$mode" = "enable" ]; then
    ENABLE_RULES+=("$normalized")
    return
  fi

  if [ "$mode" = "disable" ]; then
    DISABLE_RULES+=("$normalized")
    return
  fi

  err "internal installer error: unknown rule-selection mode '$mode'"
  exit 1
}

resolve_rules() {
  local selected=""
  local rule

  if [ -n "$EXACT_RULES" ]; then
    RULES="$EXACT_RULES"
    return
  fi

  if [ "${#ENABLE_RULES[@]}" -gt 0 ]; then
    for rule in "${ENABLE_RULES[@]}"; do
      selected="$(csv_add_rule "$selected" "$rule")"
    done
  else
    selected="$(supported_rules_csv)"
  fi

  for rule in "${DISABLE_RULES[@]}"; do
    selected="$(csv_remove_rule "$selected" "$rule")"
  done

  if [ -z "$selected" ]; then
    err "rule selection resolved to an empty set"
    err "enable at least one rule family from: $(supported_rules_display)"
    exit 1
  fi

  RULES="$selected"
}

list_rules() {
  local cli_name aliases description prerequisites

  echo "Supported rule families:"
  while IFS=$'\t' read -r cli_name aliases description prerequisites; do
    echo "  $cli_name"
    echo "    Description: $description"
    echo "    Aliases: $(format_csv_for_display "$aliases")"
    echo "    Requires: $(format_csv_for_display "$prerequisites")"
  done < <(each_rule_manifest_row)
}

rule_enabled() {
  local rule="$1"

  csv_contains_rule "$RULES" "$rule"
}

missing_prerequisite_message() {
  local tool_name="$1"
  local rule_name="$2"

  case "$tool_name" in
    cargo)
      printf "cargo not found. Install Rust first: https://rustup.rs\n"
      ;;
    rg)
      printf "rg not found. Install ripgrep first: https://github.com/BurntSushi/ripgrep#installation\n"
      ;;
    uv)
      printf "uv not found. Install uv first: https://docs.astral.sh/uv/\n"
      ;;
    *)
      printf "required tool '%s' not found for enabled rule '%s'\n" "$tool_name" "$rule_name"
      ;;
  esac
}

check_enabled_rule_prerequisites() {
  local checked=""
  local cli_name aliases description prerequisites tool tools=()

  while IFS=$'\t' read -r cli_name aliases description prerequisites; do
    if ! rule_enabled "$cli_name"; then
      continue
    fi

    if [ -z "$prerequisites" ] || [ "$prerequisites" = "-" ]; then
      continue
    fi

    IFS=',' read -r -a tools <<< "$prerequisites"
    for tool in "${tools[@]}"; do
      [ -z "$tool" ] && continue

      if csv_contains_rule "$checked" "$tool"; then
        continue
      fi

      if ! command -v "$tool" &>/dev/null; then
        err "$(missing_prerequisite_message "$tool" "$cli_name")"
        exit 1
      fi

      checked="$(csv_add_rule "$checked" "$tool")"
    done
  done < <(each_rule_manifest_row)
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
  echo "Selected rule families:"
  echo "  $RULES"
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
  echo "  5. If Claude Code is present, update $HOME/.claude/settings.json via $BINARY_NAME --configure-claude-hook --rules $RULES"
  echo "  6. If Gemini CLI is present, update $HOME/.gemini/settings.json via $BINARY_NAME --configure-gemini-hook --rules $RULES"
  echo "  7. Ensure $codex_home_dir/config.toml enables codex_hooks = true"
  echo "  8. Update $codex_home_dir/hooks.json via $BINARY_NAME --configure-codex-hook --rules $RULES"
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
    --list-rules)
      LIST_RULES=1
      ;;
    --rules)
      if [ "$#" -lt 2 ]; then
        err "missing value for --rules"
        usage
        exit 1
      fi
      set_exact_rules "$2"
      shift
      ;;
    --enable-rule)
      if [ "$#" -lt 2 ]; then
        err "missing value for --enable-rule"
        usage
        exit 1
      fi
      append_rule_selection "enable" "$2"
      shift
      ;;
    --disable-rule)
      if [ "$#" -lt 2 ]; then
        err "missing value for --disable-rule"
        usage
        exit 1
      fi
      append_rule_selection "disable" "$2"
      shift
      ;;
    --only-rg)
      warn "--only-rg is deprecated; prefer --enable-rule rg or --rules rg"
      set_exact_rules "rg"
      ;;
    --only-uv)
      warn "--only-uv is deprecated; prefer --enable-rule uv or --rules uv"
      set_exact_rules "uv"
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

if [ "$LIST_RULES" -eq 1 ]; then
  list_rules
  exit 0
fi

resolve_rules

if [ "$DRY_RUN" -eq 1 ] && [ "$CHECK_BINARY_HASH" -eq 1 ]; then
  warn "--check-binary-hash is ignored during --dry-run because the build step is skipped."
fi

if [ "$DRY_RUN" -eq 1 ]; then
  print_dry_run_plan
  exit 0
fi

check_enabled_rule_prerequisites

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

HOOK_RULE_ARGS=(--rules "$RULES")

CLAUDE_SETTINGS="${HOME}/.claude/settings.json"
if [ -d "${HOME}/.claude" ]; then
  info "Detected Claude Code"

  if [ -f "$CLAUDE_SETTINGS" ]; then
    "$SOURCE_BINARY" --configure-claude-hook "$CLAUDE_SETTINGS" "$TARGET_BINARY" "${HOOK_RULE_ARGS[@]}"
    ok "Claude Code configured"
  else
    warn "Claude Code detected but could not configure hooks automatically"
    warn "Add this to $CLAUDE_SETTINGS manually:"
    cat <<EOF
  "hooks": {
    "PreToolUse": [{
      "matcher": "Bash",
      "hooks": [{"type": "command", "command": "$BINARY_NAME --claude-hook-json --rules $RULES"}]
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
  "$SOURCE_BINARY" --configure-gemini-hook "$GEMINI_SETTINGS" "$TARGET_BINARY" "${HOOK_RULE_ARGS[@]}"
  ok "Gemini CLI configured"
fi

CODEX_HOME_DIR="${CODEX_HOME:-$HOME/.codex}"
CODEX_CONFIG="$CODEX_HOME_DIR/config.toml"
CODEX_HOOKS="$CODEX_HOME_DIR/hooks.json"

mkdir -p "$CODEX_HOME_DIR"
[ -f "$CODEX_HOOKS" ] || printf '{}\n' > "$CODEX_HOOKS"
enable_codex_hooks_in_config "$CODEX_CONFIG"
"$SOURCE_BINARY" --configure-codex-hook "$CODEX_HOOKS" "$TARGET_BINARY" "${HOOK_RULE_ARGS[@]}"
ok "Codex configured"

echo ""
ok "force_tool_preferences installed for rules: $RULES"
echo "  Restart any running agent sessions for hooks to take effect."
