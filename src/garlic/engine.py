"""Time gap calculation, accumulation, and threshold checking."""

import sys
import time
from datetime import datetime
from typing import Any


def handle_prompt(
    state: dict[str, Any], config: dict[str, Any], debug: bool = False
) -> int | None:
    """Process a prompt event: accumulate time and check thresholds.

    Returns the threshold (in minutes) that was just crossed, or None.
    """
    now = time.time()
    last = state.get("last_event_time", 0.0)

    if last > 0:
        raw_gap = (now - last) / 60.0
        cap = config["max_prompt_gap_minutes"]
        gap_minutes = raw_gap if raw_gap <= cap else 0.0
        state["accumulated_minutes"] += gap_minutes

        if debug:
            dropped = raw_gap > cap
            print(
                f"[garlic debug] gap: {raw_gap:.2f}m"
                + (f" → dropped (exceeded {cap}m cap)" if dropped else "")
                + f" | total: {state['accumulated_minutes']:.2f}m",
                file=sys.stderr,
            )
    else:
        if debug:
            print("[garlic debug] no last_event_time, skipping gap", file=sys.stderr)

    state["last_event_time"] = now

    # Check if a new threshold was crossed
    return _check_thresholds(state, config)


def handle_stop(state: dict[str, Any], config: dict[str, Any], debug: bool = False) -> None:
    """Process a stop event: accumulate generation time and update last_event_time."""
    now = time.time()
    last = state.get("last_event_time", 0.0)

    if last > 0:
        raw_gap = (now - last) / 60.0
        cap = config["max_prompt_gap_minutes"]
        gap_minutes = raw_gap if raw_gap <= cap else 0.0
        state["accumulated_minutes"] += gap_minutes

        if debug:
            dropped = raw_gap > cap
            print(
                f"[garlic debug] stop gap: {raw_gap:.2f}m"
                + (f" → dropped (exceeded {cap}m cap)" if dropped else "")
                + f" | total: {state['accumulated_minutes']:.2f}m",
                file=sys.stderr,
            )

    state["last_event_time"] = now


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


def check_bedtime(state: dict[str, Any], config: dict[str, Any]) -> bool:
    """Return True if we're in the bedtime window and haven't nudged yet.

    The bedtime window is the hour before reset_hour (e.g. 1 AM if reset is 2 AM).
    """
    if state.get("bedtime_nudge_given", False):
        return False

    reset_hour = config.get("reset_hour", 2)
    bedtime_hour = (reset_hour - 1) % 24
    now = datetime.now()

    if now.hour == bedtime_hour:
        state["bedtime_nudge_given"] = True
        return True

    return False
