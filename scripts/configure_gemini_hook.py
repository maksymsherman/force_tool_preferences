import json
import sys


def main() -> int:
    settings_path = sys.argv[1]
    binary = sys.argv[2]
    hook_command = f"{binary} --gemini-hook-json"

    with open(settings_path, encoding="utf-8") as f:
        settings = json.load(f)

    hooks = settings.setdefault("hooks", {})
    before = hooks.setdefault("BeforeTool", [])

    shell_entry = None
    for entry in before:
        if entry.get("matcher") == "run_shell_command":
            shell_entry = entry
            break

    if shell_entry is None:
        shell_entry = {"matcher": "run_shell_command", "hooks": []}
        before.append(shell_entry)

    hook_list = shell_entry.setdefault("hooks", [])

    for hook in hook_list:
        if hook_command in hook.get("command", ""):
            print("  Gemini CLI hook already configured", flush=True)
            return 0

    hook_list.append({"type": "command", "command": hook_command})

    with open(settings_path, "w", encoding="utf-8") as f:
        json.dump(settings, f, indent=2)
        f.write("\n")

    print(f"  Added BeforeTool hook to {settings_path}", flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
