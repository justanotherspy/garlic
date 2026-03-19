"""Install/update garlic hooks in ~/.claude/settings.json."""

import json
import os
import tempfile
from pathlib import Path
from typing import Any

from garlic.config import load_config, save_config

CLAUDE_SETTINGS_PATH = Path.home() / ".claude" / "settings.json"
CLAUDE_COMMANDS_DIR = Path.home() / ".claude" / "commands"
CLAUDE_MD_PATH = Path.home() / ".claude" / "CLAUDE.md"

_GARLIC_CLAUDE_MD_BLOCK = """\
## garlic
If a garlic nudge appears in a system-reminder, relay it verbatim as the last line of your response, after all work is complete. Never skip it.
"""

GARLIC_COMMAND = "Run `garlic status` and show the output to me.\n"

_HOOK_DEFINITIONS = [
    ("SessionStart", "startup", "garlic hook session-start"),
    ("UserPromptSubmit", "", "garlic hook prompt"),
    ("Stop", "", "garlic hook stop"),
]


def _hook_definitions(debug: bool) -> list[tuple[str, str, str]]:
    suffix = " --debug" if debug else ""
    return [(ev, m, cmd + suffix) for ev, m, cmd in _HOOK_DEFINITIONS]


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


def _atomic_write_json(path: Path, data: dict[str, Any]) -> None:
    """Write JSON to *path* atomically via temp-file + rename."""
    content = json.dumps(data, indent=2) + "\n"
    fd, tmp = tempfile.mkstemp(dir=path.parent, suffix=".tmp")
    try:
        os.write(fd, content.encode())
        os.fsync(fd)
        os.close(fd)
        fd = -1
        os.replace(tmp, path)  # atomic on POSIX
    except BaseException:
        if fd >= 0:
            os.close(fd)
        try:
            os.unlink(tmp)
        except OSError:
            pass
        raise


def install_hooks(
    debug: bool = False,
    config_overrides: dict[str, Any] | None = None,
) -> None:
    """Install garlic hooks into ~/.claude/settings.json (idempotent)."""
    # Ensure config exists, then apply any overrides
    config = load_config()
    if config_overrides:
        config.update(config_overrides)
        save_config(config)

    settings_path = CLAUDE_SETTINGS_PATH
    settings_path.parent.mkdir(parents=True, exist_ok=True)

    if settings_path.exists():
        settings = json.loads(settings_path.read_text())
    else:
        settings = {}

    hooks = settings.setdefault("hooks", {})

    for event_key, matcher, command in _hook_definitions(debug):
        event_hooks = hooks.setdefault(event_key, [])

        # Remove any existing garlic entries for this event
        event_hooks[:] = [h for h in event_hooks if not _is_garlic_entry(h)]

        # Build the envelope entry expected by Claude Code
        entry: dict[str, Any] = {
            "matcher": matcher,
            "hooks": [{"type": "command", "command": command}],
        }
        event_hooks.append(entry)

    _atomic_write_json(settings_path, settings)

    # Install /garlic slash command globally
    install_slash_command()

    # Install nudge-relay instruction into ~/.claude/CLAUDE.md
    install_claude_md()


def install_slash_command() -> None:
    """Install the /garlic slash command into ~/.claude/commands/."""
    CLAUDE_COMMANDS_DIR.mkdir(parents=True, exist_ok=True)
    command_path = CLAUDE_COMMANDS_DIR / "garlic.md"
    command_path.write_text(GARLIC_COMMAND)


def install_claude_md() -> None:
    """Append garlic nudge-relay instruction to ~/.claude/CLAUDE.md (idempotent)."""
    CLAUDE_MD_PATH.parent.mkdir(parents=True, exist_ok=True)
    existing = CLAUDE_MD_PATH.read_text() if CLAUDE_MD_PATH.exists() else ""
    if "## garlic" not in existing:
        separator = "\n" if existing and not existing.endswith("\n") else ""
        CLAUDE_MD_PATH.write_text(existing + separator + _GARLIC_CLAUDE_MD_BLOCK)
