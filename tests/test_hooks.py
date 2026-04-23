"""Tests for garlic.hooks."""

import io
import json
from unittest.mock import patch

from garlic.hooks import hook_prompt, hook_session_start, hook_stop


def _make_stdin(data=None):
    """Create a stdin mock with JSON data."""
    if data is None:
        data = {"session_id": "test-123", "cwd": "/tmp"}
    return io.TextIOWrapper(io.BytesIO(json.dumps(data).encode()))


def _make_config(**overrides):
    config = {
        "max_prompt_gap_minutes": 10,
        "reset_hour": 2,
        "nudge_thresholds_minutes": [60, 120],
        "nudge_style": "gentle",
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


def test_hook_session_start(monkeypatch):
    """session-start records timestamp and saves state."""
    saved = {}

    def fake_save(state):
        saved.update(state)

    monkeypatch.setattr("garlic.hooks.sys.stdin", _make_stdin())
    monkeypatch.setattr("garlic.hooks.load_config", lambda: _make_config())
    monkeypatch.setattr("garlic.hooks.load_state", lambda rh: _make_state())
    monkeypatch.setattr("garlic.hooks.save_state", fake_save)

    with patch("garlic.engine.time") as mock_time:
        mock_time.time.return_value = 1710567900.0
        hook_session_start()

    assert saved["last_event_time"] == 1710567900.0


def test_hook_session_start_shows_status_with_accumulated_time(monkeypatch, capsys):
    """session-start prints accumulated time when > 0."""
    monkeypatch.setattr("garlic.hooks.sys.stdin", _make_stdin())
    monkeypatch.setattr("garlic.hooks.load_config", lambda: _make_config())
    monkeypatch.setattr(
        "garlic.hooks.load_state",
        lambda rh: _make_state(accumulated_minutes=95.0),
    )
    monkeypatch.setattr("garlic.hooks.save_state", lambda s: None)

    with patch("garlic.engine.time") as mock_time:
        mock_time.time.return_value = 1710567900.0
        hook_session_start()

    captured = capsys.readouterr()
    assert "1h 35m" in captured.out


def test_hook_session_start_silent_when_no_time(monkeypatch, capsys):
    """session-start prints nothing when accumulated time is 0."""
    monkeypatch.setattr("garlic.hooks.sys.stdin", _make_stdin())
    monkeypatch.setattr("garlic.hooks.load_config", lambda: _make_config())
    monkeypatch.setattr(
        "garlic.hooks.load_state",
        lambda rh: _make_state(accumulated_minutes=0.0),
    )
    monkeypatch.setattr("garlic.hooks.save_state", lambda s: None)

    with patch("garlic.engine.time") as mock_time:
        mock_time.time.return_value = 1710567900.0
        hook_session_start()

    captured = capsys.readouterr()
    assert captured.out == ""


def test_hook_stop(monkeypatch):
    """stop accumulates generation time and updates last_event_time."""
    now = 1710567900.0
    saved = {}

    def fake_save(state):
        saved.update(state)

    monkeypatch.setattr("garlic.hooks.sys.stdin", _make_stdin())
    monkeypatch.setattr("garlic.hooks.load_config", lambda: _make_config())
    monkeypatch.setattr(
        "garlic.hooks.load_state",
        # last_event_time 2 minutes ago
        lambda rh: _make_state(accumulated_minutes=30.0, last_event_time=now - 120),
    )
    monkeypatch.setattr("garlic.hooks.save_state", fake_save)

    with patch("garlic.engine.time") as mock_time:
        mock_time.time.return_value = now
        hook_stop()

    assert saved["last_event_time"] == now
    assert abs(saved["accumulated_minutes"] - 32.0) < 0.01  # 30 + 2


def test_hook_prompt_no_nudge(monkeypatch, capsys):
    """prompt accumulates time but no nudge when below threshold."""
    now = 1710567900.0
    saves = []

    def fake_save(state):
        saves.append(dict(state))

    monkeypatch.setattr("garlic.hooks.sys.stdin", _make_stdin())
    monkeypatch.setattr("garlic.hooks.load_config", lambda: _make_config())
    monkeypatch.setattr(
        "garlic.hooks.load_state",
        lambda rh: _make_state(last_event_time=now - 300),
    )
    monkeypatch.setattr("garlic.hooks.save_state", fake_save)

    with patch("garlic.engine.time") as mock_time:
        mock_time.time.return_value = now
        hook_prompt()

    captured = capsys.readouterr()
    assert captured.out == ""  # no nudge
    assert len(saves) == 1  # JUS-14: one save per prompt
    assert saves[-1]["accumulated_minutes"] > 0


def test_hook_prompt_with_nudge(monkeypatch, capsys):
    """prompt outputs nudge when threshold crossed."""
    now = 1710567900.0
    saves = []

    def fake_save(state):
        saves.append(dict(state))

    monkeypatch.setattr("garlic.hooks.sys.stdin", _make_stdin())
    monkeypatch.setattr("garlic.hooks.load_config", lambda: _make_config())
    monkeypatch.setattr(
        "garlic.hooks.load_state",
        lambda rh: _make_state(
            accumulated_minutes=58.0, last_event_time=now - 300
        ),
    )
    monkeypatch.setattr("garlic.hooks.save_state", fake_save)

    with patch("garlic.engine.time") as mock_time:
        mock_time.time.return_value = now
        hook_prompt()

    captured = capsys.readouterr()
    assert len(captured.out.strip()) > 0  # nudge was printed
    response = json.loads(captured.out)
    assert "hookSpecificOutput" in response
    assert response["hookSpecificOutput"]["hookEventName"] == "UserPromptSubmit"
    assert len(response["hookSpecificOutput"]["additionalContext"]) > 0
    assert len(saves) == 1  # JUS-14: one save per prompt
    assert 60 in saves[-1]["nudges_given"]


def test_hook_prompt_ignored_no_nudge(monkeypatch, capsys):
    """prompt suppresses nudge when ignored=true."""
    now = 1710567900.0
    saves = []

    def fake_save(state):
        saves.append(dict(state))

    monkeypatch.setattr("garlic.hooks.sys.stdin", _make_stdin())
    monkeypatch.setattr("garlic.hooks.load_config", lambda: _make_config())
    monkeypatch.setattr(
        "garlic.hooks.load_state",
        lambda rh: _make_state(
            accumulated_minutes=58.0, last_event_time=now - 300, ignored=True
        ),
    )
    monkeypatch.setattr("garlic.hooks.save_state", fake_save)

    with patch("garlic.engine.time") as mock_time:
        mock_time.time.return_value = now
        hook_prompt()

    captured = capsys.readouterr()
    assert captured.out == ""  # nudge suppressed
    assert len(saves) == 1  # JUS-14: one save per prompt


def test_hook_prompt_bedtime_nudge(monkeypatch, capsys):
    """prompt outputs bedtime nudge when in bedtime window."""
    from datetime import datetime

    now = 1710567900.0
    saves = []

    def fake_save(state):
        saves.append(dict(state))

    monkeypatch.setattr("garlic.hooks.sys.stdin", _make_stdin())
    monkeypatch.setattr(
        "garlic.hooks.load_config", lambda: _make_config(reset_hour=2)
    )
    monkeypatch.setattr(
        "garlic.hooks.load_state",
        lambda rh: _make_state(
            accumulated_minutes=45.0, last_event_time=now - 60
        ),
    )
    monkeypatch.setattr("garlic.hooks.save_state", fake_save)

    with patch("garlic.engine.time") as mock_time, \
         patch("garlic.engine.datetime") as mock_dt:
        mock_time.time.return_value = now
        mock_dt.now.return_value = datetime(2026, 3, 16, 1, 30)  # bedtime window
        hook_prompt()

    captured = capsys.readouterr()
    assert len(captured.out.strip()) > 0
    response = json.loads(captured.out)
    assert "hookSpecificOutput" in response
    assert response["hookSpecificOutput"]["hookEventName"] == "UserPromptSubmit"
    assert len(response["hookSpecificOutput"]["additionalContext"]) > 0
    assert len(saves) == 1  # JUS-14: bedtime branch must not double-save
    assert saves[-1]["bedtime_nudge_given"] is True
