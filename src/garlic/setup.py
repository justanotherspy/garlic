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

GARLIC_COMMAND = """\
---
description: Show accumulated active coding time today
argument-hint: "[status|week|stats|ignore]"
allowed-tools: Bash(garlic:*)
---
Run `garlic $ARGUMENTS` (or `garlic status` if no arg) and show the output to me.
"""

_HOOK_DEFINITIONS = [
    ("SessionStart", "startup", "garlic hook session-start"),
    ("UserPromptSubmit", "", "garlic hook prompt"),
    ("Stop", "", "garlic hook stop"),
    ("SessionEnd", "", "garlic hook session-end"),
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

    # Remove legacy CLAUDE.md injection if present
    if cleanup_claude_md():
        print("removed legacy CLAUDE.md injection")


def install_slash_command() -> None:
    """Install the /garlic slash command into ~/.claude/commands/."""
    CLAUDE_COMMANDS_DIR.mkdir(parents=True, exist_ok=True)
    command_path = CLAUDE_COMMANDS_DIR / "garlic.md"
    command_path.write_text(GARLIC_COMMAND)


def cleanup_claude_md() -> bool:
    """Remove legacy garlic nudge-relay block from ~/.claude/CLAUDE.md if present.

    Returns True if the block was found and removed, False otherwise.
    """
    if not CLAUDE_MD_PATH.exists():
        return False

    content = CLAUDE_MD_PATH.read_text()

    # Look for the legacy "## garlic" block
    if "## garlic" not in content:
        return False

    # Remove the block (match the section header and everything until the next section or EOF)
    lines = content.split("\n")
    cleaned_lines = []
    in_garlic_section = False

    for line in lines:
        # Start of garlic section
        if line.strip().startswith("## garlic"):
            in_garlic_section = True
            continue
        # End of garlic section (next section header or end)
        if in_garlic_section and line.strip().startswith("#"):
            in_garlic_section = False
        # Skip lines in garlic section
        if in_garlic_section:
            continue
        cleaned_lines.append(line)

    # Remove trailing blank lines that might have been left
    while cleaned_lines and not cleaned_lines[-1].strip():
        cleaned_lines.pop()

    cleaned_content = "\n".join(cleaned_lines)
    if cleaned_content and not cleaned_content.endswith("\n"):
        cleaned_content += "\n"

    CLAUDE_MD_PATH.write_text(cleaned_content)
    return True
