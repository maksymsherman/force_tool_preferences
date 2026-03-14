import json
import sys


def main() -> int:
    settings_path = sys.argv[1]
    binary = sys.argv[2]
    hook_command = f"{binary} --claude-hook-json"

    with open(settings_path, encoding="utf-8") as f:
        settings = json.load(f)

    hooks = settings.setdefault("hooks", {})
    pre = hooks.setdefault("PreToolUse", [])

    bash_entry = None
    for entry in pre:
        if entry.get("matcher") == "Bash":
            bash_entry = entry
            break

    if bash_entry is None:
        bash_entry = {"matcher": "Bash", "hooks": []}
        pre.append(bash_entry)

    hook_list = bash_entry.setdefault("hooks", [])

    for hook in hook_list:
        if hook_command in hook.get("command", ""):
            print("  Claude Code hook already configured", flush=True)
            return 0

    hook_list.append({"type": "command", "command": hook_command})

    with open(settings_path, "w", encoding="utf-8") as f:
        json.dump(settings, f, indent=2)
        f.write("\n")

    print(f"  Added PreToolUse hook to {settings_path}", flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
