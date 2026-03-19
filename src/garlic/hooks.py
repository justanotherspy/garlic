"""Hook subcommand handlers for Claude Code integration."""

import json
import sys
from typing import Any

from garlic.config import load_config
from garlic.engine import check_bedtime, handle_prompt, handle_session_start, handle_stop
from garlic.nudges import get_bedtime_nudge, get_nudge
from garlic.state import load_state, save_state


def _read_hook_input() -> dict[str, Any]:
    """Read JSON from stdin (Claude Code hook format)."""
    return json.load(sys.stdin)


def _format_time(minutes: float) -> str:
    """Format minutes as a human-readable time string."""
    hours = int(minutes // 60)
    mins = int(minutes % 60)
    if hours > 0:
        return f"{hours}h {mins:02d}m"
    return f"{mins}m"


def hook_session_start(debug: bool = False) -> None:
    """Handle SessionStart hook: record start timestamp and show status."""
    _read_hook_input()
    config = load_config()
    state = load_state(config["reset_hour"])
    if debug:
        import sys
        print("[garlic debug] session-start: recording timestamp", file=sys.stderr)
    handle_session_start(state)
    save_state(state)

    minutes = state["accumulated_minutes"]
    if minutes > 0:
        print(f"🧄 {_format_time(minutes)} of active coding today")


def hook_prompt(debug: bool = False) -> None:
    """Handle UserPromptSubmit hook: accumulate time, maybe nudge."""
    _read_hook_input()
    config = load_config()
    state = load_state(config["reset_hour"])
    threshold = handle_prompt(state, config, debug=debug)
    save_state(state)

    nudge = None

    if threshold is not None and not state.get("ignored", False):
        thresholds = config.get("nudge_thresholds_minutes", [])
        is_final = thresholds and threshold == max(thresholds)
        nudge = get_nudge(config["nudge_style"], state["accumulated_minutes"], is_final=is_final)
    elif not state.get("ignored", False) and check_bedtime(state, config):
        nudge = get_bedtime_nudge(state["accumulated_minutes"])
        save_state(state)  # persist bedtime_nudge_given

    if nudge is not None:
        print(
            "Please relay the following message to the user exactly as"
            " written, without rephrasing:\n\n" + nudge
        )


def hook_stop(debug: bool = False) -> None:
    """Handle Stop hook: accumulate generation time and update last_event_time."""
    _read_hook_input()
    config = load_config()
    state = load_state(config["reset_hour"])
    handle_stop(state, config, debug=debug)
    save_state(state)
