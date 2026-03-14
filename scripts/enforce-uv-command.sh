#!/bin/sh

set -eu

usage() {
  cat <<'EOF'
Usage:
  enforce-uv-command.sh --stdin-command [--claude-json]
  enforce-uv-command.sh --command "python -m pytest" [--claude-json]

Exit status:
  0 = allowed
  2 = blocked (plain mode only)
EOF
}

command_text=""
claude_json=0
stdin_command=0

while [ "$#" -gt 0 ]; do
  case "$1" in
    --command)
      if [ "$#" -lt 2 ]; then
        echo "missing value for --command" >&2
        exit 1
      fi
      command_text=$2
      shift 2
      ;;
    --stdin-command)
      stdin_command=1
      shift
      ;;
    --claude-json)
      claude_json=1
      shift
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

if [ "$stdin_command" -eq 1 ]; then
  command_text=$(cat)
fi

trimmed_command=$(printf '%s' "$command_text" | sed 's/^[[:space:]]*//; s/[[:space:]]*$//')

if [ -z "$trimmed_command" ]; then
  exit 0
fi

emit_block() {
  reason=$1

  if [ "$claude_json" -eq 1 ]; then
    escaped_reason=$(printf '%s' "$reason" | sed 's/\\/\\\\/g; s/"/\\"/g')
    printf '{"decision":"block","reason":"%s"}\n' "$escaped_reason"
    exit 0
  fi

  printf '%s\n' "$reason" >&2
  exit 2
}

if printf '%s\n' "$trimmed_command" | grep -Eq '(^|[[:space:]])uv[[:space:]]+init([[:space:]]|$)'; then
  emit_block "Do not run 'uv init' in an existing project unless the user explicitly asks for project creation or conversion. Inspect the repo first and prefer 'uv run', 'uv add', 'uv sync', or 'uv run --with'."
fi

segment_has_uv_context() {
  printf '%s\n' "$1" | grep -Eq '(^|[[:space:]])uvx([[:space:]]|$)|(^|[[:space:]])uv[[:space:]]+(run|add|remove|sync|pip|tool|lock|export|venv)([[:space:]]|$)'
}

segment_has_forbidden_token() {
  printf '%s\n' "$1" | grep -Eq '(^|[[:space:]])(python|python3|pip|pip3)([[:space:]]|$)'
}

original_ifs=$IFS
IFS='
'

for segment in $(printf '%s\n' "$trimmed_command" | sed 's/&&/\n/g; s/||/\n/g; s/;/\n/g; s/|/\n/g'); do
  current_segment=$(printf '%s' "$segment" | sed 's/^[[:space:]]*//; s/[[:space:]]*$//')
  [ -z "$current_segment" ] && continue

  if segment_has_uv_context "$current_segment"; then
    continue
  fi

  if segment_has_forbidden_token "$current_segment"; then
    emit_block "Use uv instead of bare Python or pip commands in this project. Replace the blocked command with 'uv run ...', 'uv add ...', 'uv add --dev ...', 'uv remove ...', or 'uv run --with ...' as appropriate."
  fi
done

IFS=$original_ifs
