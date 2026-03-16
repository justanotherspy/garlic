"""Hook subcommand handlers for Claude Code integration."""

import json
import sys
from typing import Any

from garlic.config import load_config
from garlic.engine import handle_prompt, handle_session_start, handle_stop
from garlic.nudges import get_nudge
from garlic.state import load_state, save_state


def _read_hook_input() -> dict[str, Any]:
    """Read JSON from stdin (Claude Code hook format)."""
    return json.load(sys.stdin)


def hook_session_start() -> None:
    """Handle SessionStart hook: record start timestamp."""
    _read_hook_input()
    config = load_config()
    state = load_state(config["reset_hour"])
    handle_session_start(state)
    save_state(state)


def hook_prompt() -> None:
    """Handle UserPromptSubmit hook: accumulate time, maybe nudge."""
    _read_hook_input()
    config = load_config()
    state = load_state(config["reset_hour"])
    threshold = handle_prompt(state, config)
    save_state(state)

    if threshold is not None and not state.get("ignored", False):
        nudge = get_nudge(config["nudge_style"], state["accumulated_minutes"])
        print(nudge)


def hook_stop() -> None:
    """Handle Stop hook: update last_event_time."""
    _read_hook_input()
    config = load_config()
    state = load_state(config["reset_hour"])
    handle_stop(state)
    save_state(state)
