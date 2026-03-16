"""State file (~/.garlic/state.toml) read/write with file locking."""

import fcntl
import time
import tomllib
from datetime import datetime, timedelta
from pathlib import Path
from typing import Any

from garlic.config import GARLIC_DIR, _write_toml

STATE_PATH = GARLIC_DIR / "state.toml"

DEFAULT_STATE: dict[str, Any] = {
    "date": "",
    "accumulated_minutes": 0.0,
    "last_event_time": 0.0,
    "nudges_given": [],
    "ignored": False,
}


def _current_date(reset_hour: int) -> str:
    """Return today's date string, shifted by reset_hour.

    If it's before reset_hour (e.g., 2 AM), we still consider it
    the previous day for tracking purposes.
    """
    now = datetime.now()
    if now.hour < reset_hour:
        now -= timedelta(days=1)
    return now.strftime("%Y-%m-%d")


def load_state(reset_hour: int) -> dict[str, Any]:
    """Load state from disk, resetting if the date has rolled over.

    Uses fcntl.flock for concurrent session safety.
    """
    GARLIC_DIR.mkdir(parents=True, exist_ok=True)
    today = _current_date(reset_hour)

    if not STATE_PATH.exists():
        state = dict(DEFAULT_STATE)
        state["date"] = today
        return state

    with STATE_PATH.open("rb") as f:
        fcntl.flock(f, fcntl.LOCK_SH)
        try:
            state = tomllib.load(f)
        finally:
            fcntl.flock(f, fcntl.LOCK_UN)

    # Day reset
    if state.get("date") != today:
        state = dict(DEFAULT_STATE)
        state["date"] = today

    return state


def save_state(state: dict[str, Any]) -> None:
    """Write state to disk with an exclusive file lock."""
    GARLIC_DIR.mkdir(parents=True, exist_ok=True)

    with STATE_PATH.open("w") as f:
        fcntl.flock(f, fcntl.LOCK_EX)
        try:
            _write_state(f, state)
        finally:
            fcntl.flock(f, fcntl.LOCK_UN)


def _write_state(f: Any, state: dict[str, Any]) -> None:
    """Serialize state as TOML to an open file handle."""
    lines: list[str] = []
    for key, value in state.items():
        if isinstance(value, str):
            lines.append(f'{key} = "{value}"')
        elif isinstance(value, bool):
            lines.append(f"{key} = {'true' if value else 'false'}")
        elif isinstance(value, (int, float)):
            lines.append(f"{key} = {value}")
        elif isinstance(value, list):
            items = ", ".join(
                f'"{v}"' if isinstance(v, str) else str(v) for v in value
            )
            lines.append(f"{key} = [{items}]")
    f.write("\n".join(lines) + "\n")
