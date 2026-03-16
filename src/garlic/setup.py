"""Install/update garlic hooks in ~/.claude/settings.json."""

import json
from pathlib import Path
from typing import Any

from garlic.config import load_config

CLAUDE_SETTINGS_PATH = Path.home() / ".claude" / "settings.json"

# Each entry: (event_key, matcher, command)
HOOK_DEFINITIONS: list[tuple[str, str, str]] = [
    ("SessionStart", "startup", "garlic hook session-start"),
    ("UserPromptSubmit", "", "garlic hook prompt"),
    ("Stop", "", "garlic hook stop"),
]


def _is_garlic_entry(entry: dict[str, Any]) -> bool:
    """Check if a hook entry belongs to garlic (new envelope or legacy flat format)."""
    # New envelope format: {"matcher": ..., "hooks": [...]}
    if any(
        h.get("type") == "command" and h.get("command", "").startswith("garlic hook")
        for h in entry.get("hooks", [])
    ):
        return True
    # Legacy flat format: {"type": "command", "command": "garlic hook ..."}
    return entry.get("type") == "command" and entry.get("command", "").startswith(
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

    for event_key, matcher, command in HOOK_DEFINITIONS:
        event_hooks = hooks.setdefault(event_key, [])

        # Remove any existing garlic entries for this event
        event_hooks[:] = [h for h in event_hooks if not _is_garlic_entry(h)]

        # Build the envelope entry expected by Claude Code
        entry: dict[str, Any] = {
            "matcher": matcher,
            "hooks": [{"type": "command", "command": command}],
        }
        event_hooks.append(entry)

    settings_path.write_text(json.dumps(settings, indent=2) + "\n")
