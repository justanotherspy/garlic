"""Time gap calculation, accumulation, and threshold checking."""

import time
from typing import Any


def handle_prompt(state: dict[str, Any], config: dict[str, Any]) -> int | None:
    """Process a prompt event: accumulate time and check thresholds.

    Returns the threshold (in minutes) that was just crossed, or None.
    """
    now = time.time()
    last = state.get("last_event_time", 0.0)

    if last > 0:
        gap_minutes = (now - last) / 60.0
        cap = config["max_prompt_gap_minutes"]
        if gap_minutes > cap:
            gap_minutes = cap
        state["accumulated_minutes"] += gap_minutes

    state["last_event_time"] = now

    # Check if a new threshold was crossed
    return _check_thresholds(state, config)


def handle_stop(state: dict[str, Any]) -> None:
    """Process a stop event: update last_event_time (no accumulation)."""
    state["last_event_time"] = time.time()


def handle_session_start(state: dict[str, Any]) -> None:
    """Process a session-start event: record timestamp."""
    state["last_event_time"] = time.time()


def _check_thresholds(
    state: dict[str, Any], config: dict[str, Any]
) -> int | None:
    """Return the highest newly-crossed threshold, or None."""
    accumulated = state["accumulated_minutes"]
    thresholds = sorted(config.get("nudge_thresholds_minutes", []))
    nudges_given = state.get("nudges_given", [])

    for threshold in reversed(thresholds):
        if accumulated >= threshold and threshold not in nudges_given:
            nudges_given.append(threshold)
            state["nudges_given"] = nudges_given
            return threshold

    return None
