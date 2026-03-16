"""Install/update garlic hooks in ~/.claude/settings.json."""

import json
from pathlib import Path
from typing import Any

from garlic.config import load_config

CLAUDE_SETTINGS_PATH = Path.home() / ".claude" / "settings.json"

HOOK_DEFINITIONS: list[dict[str, Any]] = [
    {
        "type": "command",
        "command": "garlic hook session-start",
    },
    {
        "type": "command",
        "command": "garlic hook prompt",
    },
    {
        "type": "command",
        "command": "garlic hook stop",
    },
]

# Map hook event names to Claude Code hook event keys
HOOK_EVENTS = {
    "garlic hook session-start": "SessionStart",
    "garlic hook prompt": "UserPromptSubmit",
    "garlic hook stop": "Stop",
}

# SessionStart uses a matcher
SESSION_START_MATCHER = "startup"


def _is_garlic_hook(hook: dict[str, Any]) -> bool:
    """Check if a hook entry belongs to garlic."""
    return hook.get("type") == "command" and hook.get("command", "").startswith(
        "garlic hook"
    )


def install_hooks() -> None:
    """Install garlic hooks into ~/.claude/settings.json (idempotent)."""
    # Ensure config exists
    load_config()

    settings_path = CLAUDE_SETTINGS_PATH
    settings_path.parent.mkdir(parents=True, exist_ok=True)

    if settings_path.exists():
        settings = json.loads(settings_path.read_text())
    else:
        settings = {}

    hooks = settings.setdefault("hooks", {})

    for hook_def in HOOK_DEFINITIONS:
        event_key = HOOK_EVENTS[hook_def["command"]]
        event_hooks = hooks.setdefault(event_key, [])

        # Remove any existing garlic hooks for this event
        event_hooks[:] = [h for h in event_hooks if not _is_garlic_hook(h)]

        # Build the new hook entry
        entry: dict[str, Any] = {
            "type": "command",
            "command": hook_def["command"],
        }
        if event_key == "SessionStart":
            entry["matcher"] = SESSION_START_MATCHER

        event_hooks.append(entry)

    settings_path.write_text(json.dumps(settings, indent=2) + "\n")
