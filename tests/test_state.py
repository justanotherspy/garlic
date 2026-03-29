"""Tests for garlic.state."""

import threading
import tomllib
from datetime import datetime, timedelta
from unittest.mock import patch

from garlic.state import (
    DEFAULT_STATE,
    _current_date,
    load_state,
    save_state,
)


def test_current_date_after_reset_hour():
    """After reset_hour, current_date returns today."""
    # Mock 10 AM
    fake_now = datetime(2026, 3, 16, 10, 0, 0)
    with patch("garlic.state.datetime") as mock_dt:
        mock_dt.now.return_value = fake_now
        mock_dt.side_effect = lambda *a, **kw: datetime(*a, **kw)
        assert _current_date(2) == "2026-03-16"


def test_current_date_before_reset_hour():
    """Before reset_hour, current_date returns yesterday."""
    # Mock 1 AM
    fake_now = datetime(2026, 3, 16, 1, 0, 0)
    with patch("garlic.state.datetime") as mock_dt:
        mock_dt.now.return_value = fake_now
        mock_dt.side_effect = lambda *a, **kw: datetime(*a, **kw)
        assert _current_date(2) == "2026-03-15"


def test_load_state_creates_fresh(garlic_env):
    """When no state file exists, returns fresh state with today's date."""
    _, _, state_path = garlic_env
    state_path.unlink()  # remove default state so it creates fresh

    with patch("garlic.state._current_date", return_value="2026-03-16"):
        state = load_state(reset_hour=2)

    assert state["date"] == "2026-03-16"
    assert state["accumulated_minutes"] == 0.0
    assert state["nudges_given"] == []
    assert state["ignored"] is False


def test_load_state_reads_existing(garlic_env):
    """When state file exists with matching date, reads it."""
    _, _, state_path = garlic_env
    state_path.write_text(
        'date = "2026-03-16"\n'
        "accumulated_minutes = 45.5\n"
        "last_event_time = 1710567890.123\n"
        "nudges_given = [60]\n"
        "ignored = false\n"
    )

    with patch("garlic.state._current_date", return_value="2026-03-16"):
        state = load_state(reset_hour=2)

    assert state["accumulated_minutes"] == 45.5
    assert state["nudges_given"] == [60]


def test_load_state_resets_on_new_day(garlic_env):
    """When date doesn't match, state resets."""
    _, _, state_path = garlic_env
    state_path.write_text(
        'date = "2026-03-15"\n'
        "accumulated_minutes = 120.0\n"
        "last_event_time = 1710567890.123\n"
        "nudges_given = [60, 120]\n"
        "ignored = true\n"
    )

    with patch("garlic.state._current_date", return_value="2026-03-16"):
        state = load_state(reset_hour=2)

    assert state["date"] == "2026-03-16"
    assert state["accumulated_minutes"] == 0.0
    assert state["nudges_given"] == []
    assert state["ignored"] is False


def test_concurrent_saves_dont_corrupt(garlic_env):
    """Concurrent save_state calls from multiple threads produce valid TOML."""
    _, _, state_path = garlic_env

    errors = []

    def writer(minutes):
        try:
            save_state({
                "date": "2026-03-16",
                "accumulated_minutes": float(minutes),
                "last_event_time": 1710567890.0,
                "nudges_given": [],
                "ignored": False,
            })
        except Exception as e:
            errors.append(e)

    threads = [threading.Thread(target=writer, args=(i,)) for i in range(20)]
    for t in threads:
        t.start()
    for t in threads:
        t.join()

    assert errors == [], f"Errors during concurrent writes: {errors}"

    # File must be valid TOML after all writes
    with state_path.open("rb") as f:
        loaded = tomllib.load(f)
    assert "accumulated_minutes" in loaded


def test_save_state_roundtrip(garlic_env):
    """save_state writes state that can be read back by tomllib."""
    _, _, state_path = garlic_env

    state = {
        "date": "2026-03-16",
        "accumulated_minutes": 95.3,
        "last_event_time": 1710567890.123,
        "nudges_given": [60],
        "ignored": True,
    }
    save_state(state)

    with state_path.open("rb") as f:
        loaded = tomllib.load(f)

    assert loaded["date"] == "2026-03-16"
    assert loaded["accumulated_minutes"] == 95.3
    assert loaded["nudges_given"] == [60]
    assert loaded["ignored"] is True
