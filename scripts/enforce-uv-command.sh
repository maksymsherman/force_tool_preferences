#!/usr/bin/env bash

set -euo pipefail

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

while (($# > 0)); do
  case "$1" in
    --command)
      if (($# < 2)); then
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

if ((stdin_command)); then
  command_text=$(</dev/stdin)
fi

trimmed_command=$command_text
trimmed_command="${trimmed_command#"${trimmed_command%%[![:space:]]*}"}"
trimmed_command="${trimmed_command%"${trimmed_command##*[![:space:]]}"}"

if [[ -z $trimmed_command ]]; then
  exit 0
fi

emit_block() {
  local reason=$1

  if ((claude_json)); then
    reason=${reason//\\/\\\\}
    reason=${reason//\"/\\\"}
    printf '{"decision":"block","reason":"%s"}\n' "$reason"
    exit 0
  fi

  printf '%s\n' "$reason" >&2
  exit 2
}

segment_has_uv_context=0
expect_uv_subcommand=0
token=""

reset_segment_state() {
  segment_has_uv_context=0
  expect_uv_subcommand=0
}

process_token() {
  local current_token=$1

  [[ -z $current_token ]] && return

  if ((expect_uv_subcommand)); then
    case "$current_token" in
      init)
        emit_block "Do not run 'uv init' in an existing project unless the user explicitly asks for project creation or conversion. Inspect the repo first and prefer 'uv run', 'uv add', 'uv sync', or 'uv run --with'."
        ;;
      run|add|remove|sync|pip|tool|lock|export|venv)
        segment_has_uv_context=1
        ;;
    esac
    expect_uv_subcommand=0
    return
  fi

  case "$current_token" in
    uv)
      expect_uv_subcommand=1
      ;;
    uvx)
      segment_has_uv_context=1
      ;;
    python|python3|pip|pip3)
      if (( ! segment_has_uv_context )); then
        emit_block "Use uv instead of bare Python or pip commands in this project. Replace the blocked command with 'uv run ...', 'uv add ...', 'uv add --dev ...', 'uv remove ...', or 'uv run --with ...' as appropriate."
      fi
      ;;
  esac
}

flush_token() {
  if [[ -n $token ]]; then
    process_token "$token"
    token=""
  fi
}

length=${#trimmed_command}
in_single_quote=0
in_double_quote=0
escape_next=0

for ((i = 0; i < length; i++)); do
  char=${trimmed_command:i:1}

  if ((escape_next)); then
    token+=$char
    escape_next=0
    continue
  fi

  if ((in_single_quote)); then
    if [[ $char == "'" ]]; then
      in_single_quote=0
    else
      token+=$char
    fi
    continue
  fi

  if ((in_double_quote)); then
    case "$char" in
      '"')
        in_double_quote=0
        ;;
      '\\')
        escape_next=1
        ;;
      *)
        token+=$char
        ;;
    esac
    continue
  fi

  case "$char" in
    [[:space:]])
      flush_token
      ;;
    "'")
      in_single_quote=1
      ;;
    '"')
      in_double_quote=1
      ;;
    ';')
      flush_token
      reset_segment_state
      ;;
    '|')
      flush_token
      if ((i + 1 < length)) && [[ ${trimmed_command:i+1:1} == "|" ]]; then
        ((i++))
      fi
      reset_segment_state
      ;;
    '&')
      flush_token
      if ((i + 1 < length)) && [[ ${trimmed_command:i+1:1} == "&" ]]; then
        ((i++))
      fi
      reset_segment_state
      ;;
    '\\')
      if ((i + 1 < length)); then
        ((i++))
        token+=${trimmed_command:i:1}
      else
        token+='\\'
      fi
      ;;
    *)
      token+=$char
      ;;
  esac
done

flush_token
