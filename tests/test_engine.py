"""Tests for garlic.engine."""

import time
from unittest.mock import patch

from garlic.engine import handle_prompt, handle_session_start, handle_stop


def _make_config(**overrides):
    config = {
        "max_prompt_gap_minutes": 10,
        "nudge_thresholds_minutes": [60, 120, 180, 240],
    }
    config.update(overrides)
    return config


def _make_state(**overrides):
    state = {
        "date": "2026-03-16",
        "accumulated_minutes": 0.0,
        "last_event_time": 0.0,
        "nudges_given": [],
        "ignored": False,
    }
    state.update(overrides)
    return state


def test_handle_prompt_accumulates_time():
    """Prompt accumulates gap time capped at max_prompt_gap_minutes."""
    now = 1710567900.0
    # Last event was 5 minutes ago
    state = _make_state(last_event_time=now - 300)
    config = _make_config()

    with patch("garlic.engine.time") as mock_time:
        mock_time.time.return_value = now
        handle_prompt(state, config)

    assert abs(state["accumulated_minutes"] - 5.0) < 0.01
    assert state["last_event_time"] == now


def test_handle_prompt_caps_large_gap():
    """Gaps larger than max_prompt_gap_minutes are capped."""
    now = 1710567900.0
    # Last event was 30 minutes ago
    state = _make_state(last_event_time=now - 1800)
    config = _make_config(max_prompt_gap_minutes=10)

    with patch("garlic.engine.time") as mock_time:
        mock_time.time.return_value = now
        handle_prompt(state, config)

    assert abs(state["accumulated_minutes"] - 10.0) < 0.01


def test_handle_prompt_first_event_no_accumulation():
    """First prompt (last_event_time=0) doesn't accumulate."""
    now = 1710567900.0
    state = _make_state(last_event_time=0.0)
    config = _make_config()

    with patch("garlic.engine.time") as mock_time:
        mock_time.time.return_value = now
        handle_prompt(state, config)

    assert state["accumulated_minutes"] == 0.0
    assert state["last_event_time"] == now


def test_handle_prompt_crosses_threshold():
    """Returns threshold when accumulated time crosses it."""
    now = 1710567900.0
    # Already at 58 minutes, 5-minute gap will cross 60
    state = _make_state(accumulated_minutes=58.0, last_event_time=now - 300)
    config = _make_config()

    with patch("garlic.engine.time") as mock_time:
        mock_time.time.return_value = now
        result = handle_prompt(state, config)

    assert result == 60
    assert 60 in state["nudges_given"]


def test_handle_prompt_no_duplicate_nudge():
    """Already-given thresholds are not returned again."""
    now = 1710567900.0
    state = _make_state(
        accumulated_minutes=58.0,
        last_event_time=now - 300,
        nudges_given=[60],
    )
    config = _make_config()

    with patch("garlic.engine.time") as mock_time:
        mock_time.time.return_value = now
        result = handle_prompt(state, config)

    assert result is None


def test_handle_prompt_crosses_multiple_returns_highest():
    """When multiple thresholds are crossed, returns the highest new one."""
    now = 1710567900.0
    # At 55 min, 10-min gap -> 65 min, crosses 60. But if we start at 115...
    state = _make_state(accumulated_minutes=115.0, last_event_time=now - 600)
    config = _make_config()

    with patch("garlic.engine.time") as mock_time:
        mock_time.time.return_value = now
        result = handle_prompt(state, config)

    # Crosses both 60 and 120; returns highest (120)
    assert result == 120
    assert 120 in state["nudges_given"]


def test_handle_stop_updates_last_event_time():
    """Stop just updates last_event_time, no accumulation."""
    now = 1710567900.0
    state = _make_state(accumulated_minutes=30.0, last_event_time=now - 600)

    with patch("garlic.engine.time") as mock_time:
        mock_time.time.return_value = now
        handle_stop(state)

    assert state["last_event_time"] == now
    assert state["accumulated_minutes"] == 30.0  # unchanged


def test_handle_session_start_records_timestamp():
    """Session start records timestamp."""
    now = 1710567900.0
    state = _make_state()

    with patch("garlic.engine.time") as mock_time:
        mock_time.time.return_value = now
        handle_session_start(state)

    assert state["last_event_time"] == now
