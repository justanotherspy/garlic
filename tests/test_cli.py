"""Tests for the garlic CLI."""

import argparse
import importlib.metadata
from datetime import datetime
from unittest.mock import patch

from garlic.cli import (
    _check_latest_version,
    _parse_version,
    _prompt_config,
    build_parser,
    cmd_setup,
    cmd_stats,
    cmd_version,
)


def test_parser_version():
    parser = build_parser()
    args = parser.parse_args(["version"])
    assert args.command == "version"


def test_cmd_version_output(capsys):
    """Version prints current version; update check is silent when no update."""
    with patch("garlic.cli._check_latest_version", return_value=None):
        cmd_version(None)
    out = capsys.readouterr().out.strip()
    expected = f"garlic {importlib.metadata.version('garlic-cli')}"
    assert out == expected


def test_cmd_version_shows_update(capsys):
    """When a newer version exists, prints upgrade suggestion."""
    with patch("garlic.cli._check_latest_version", return_value="99.0.0"):
        cmd_version(None)
    out = capsys.readouterr().out
    assert "update available: 99.0.0" in out
    assert "uv tool upgrade garlic-cli" in out


def test_parse_version():
    assert _parse_version("1.2.3") == (1, 2, 3)
    assert _parse_version("0.1.0") < _parse_version("0.2.0")
    assert _parse_version("1.0.0") > _parse_version("0.99.99")


def test_check_latest_version_network_failure():
    """Network errors return None silently."""
    with patch("urllib.request.urlopen", side_effect=OSError("no network")):
        assert _check_latest_version("0.1.0") is None


def test_check_latest_version_newer():
    """Returns latest version string when PyPI has a newer release."""
    import io
    import json

    payload = json.dumps({"info": {"version": "99.0.0"}}).encode()
    mock_resp = io.BytesIO(payload)
    mock_resp.__enter__ = lambda s: s
    mock_resp.__exit__ = lambda s, *a: None

    with patch("urllib.request.urlopen", return_value=mock_resp):
        assert _check_latest_version("0.1.0") == "99.0.0"


def test_check_latest_version_not_newer():
    """Returns None when installed version is current."""
    import io
    import json

    payload = json.dumps({"info": {"version": "0.1.0"}}).encode()
    mock_resp = io.BytesIO(payload)
    mock_resp.__enter__ = lambda s: s
    mock_resp.__exit__ = lambda s, *a: None

    with patch("urllib.request.urlopen", return_value=mock_resp):
        assert _check_latest_version("0.1.0") is None


def test_parser_setup():
    parser = build_parser()
    args = parser.parse_args(["setup"])
    assert args.command == "setup"
    assert args.yes is False


def test_parser_setup_yes_flag():
    parser = build_parser()
    args = parser.parse_args(["setup", "-y"])
    assert args.yes is True
    args2 = parser.parse_args(["setup", "--yes"])
    assert args2.yes is True


def test_parser_status():
    parser = build_parser()
    args = parser.parse_args(["status"])
    assert args.command == "status"


def test_parser_ignore():
    parser = build_parser()
    args = parser.parse_args(["ignore"])
    assert args.command == "ignore"


def test_parser_hook_events():
    parser = build_parser()
    for event in ("session-start", "prompt", "stop"):
        args = parser.parse_args(["hook", event])
        assert args.command == "hook"
        assert args.hook_event == event


def test_prompt_config_all_defaults():
    """Pressing Enter for every prompt returns no overrides."""
    with patch("builtins.input", return_value=""):
        result = _prompt_config()
    assert result == {}


def test_prompt_config_custom_values():
    """Entering values overrides the corresponding keys."""
    inputs = iter(["15", "20", "4", "spicy"])
    with patch("builtins.input", side_effect=inputs):
        result = _prompt_config()
    assert result["nudge_thresholds_minutes"] == list(range(15, 241, 15))
    assert result["max_prompt_gap_minutes"] == 20
    assert result["reset_hour"] == 4
    assert result["nudge_style"] == "spicy"


def test_prompt_config_invalid_values_use_defaults(capsys):
    """Invalid inputs fall back to defaults with a warning."""
    inputs = iter(["abc", "-5", "99", "bogus"])
    with patch("builtins.input", side_effect=inputs):
        result = _prompt_config()
    assert result == {}
    out = capsys.readouterr().out
    assert out.count("invalid") == 4


def test_cmd_setup_yes_skips_prompts(capsys):
    """--yes flag skips interactive prompts entirely."""
    args = argparse.Namespace(yes=True, debug=False, defaults=False)
    with patch("garlic.cli.install_hooks") as mock_install:
        cmd_setup(args)
    mock_install.assert_called_once_with(debug=False, config_overrides={})
    out = capsys.readouterr().out
    assert "hooks installed" in out


def test_cmd_setup_defaults_with_yes(capsys):
    """--defaults -y overwrites config with built-in defaults without prompting."""
    args = argparse.Namespace(yes=True, debug=False, defaults=True)
    with patch("garlic.cli.install_hooks") as mock_install:
        cmd_setup(args)
    from garlic.config import DEFAULTS
    mock_install.assert_called_once_with(debug=False, config_overrides=dict(DEFAULTS))
    out = capsys.readouterr().out
    assert "config reset to built-in defaults" in out


def test_cmd_setup_defaults_confirmed(capsys):
    """--defaults prompts for confirmation and proceeds on 'y'."""
    args = argparse.Namespace(yes=False, debug=False, defaults=True)
    with patch("builtins.input", return_value="y"), \
         patch("garlic.cli.install_hooks") as mock_install:
        cmd_setup(args)
    from garlic.config import DEFAULTS
    mock_install.assert_called_once_with(debug=False, config_overrides=dict(DEFAULTS))
    out = capsys.readouterr().out
    assert "config reset to built-in defaults" in out


def test_cmd_setup_defaults_declined(capsys):
    """--defaults prompts for confirmation and aborts on 'n'."""
    args = argparse.Namespace(yes=False, debug=False, defaults=True)
    with patch("builtins.input", return_value="n"), \
         patch("garlic.cli.install_hooks") as mock_install:
        cmd_setup(args)
    mock_install.assert_not_called()
    out = capsys.readouterr().out
    assert "setup cancelled" in out


def test_parser_stats():
    parser = build_parser()
    args = parser.parse_args(["stats"])
    assert args.command == "stats"


class _FixedDatetime(datetime):
    """Datetime with a pinned `now()` for deterministic stats tests."""

    _fixed: "datetime"

    @classmethod
    def now(cls, tz=None):  # type: ignore[override]
        return cls._fixed


def _write_state_with_history(state_path, today, accumulated, history):
    """Overwrite state.toml with given date, today's minutes, and history."""
    history_str = ", ".join(
        f'{{date = "{e["date"]}", minutes = {e["minutes"]}}}' for e in history
    )
    state_path.write_text(
        f'date = "{today}"\n'
        f"accumulated_minutes = {accumulated}\n"
        'last_event_time = 0.0\n'
        'nudges_given = []\n'
        'ignored = false\n'
        'bedtime_nudge_given = false\n'
        f"history = [{history_str}]\n"
    )


def test_cmd_stats_empty_history(garlic_env, capsys):
    """With no history and no time today, stats shows zero streaks, no busiest day."""
    _, _, state_path = garlic_env
    _write_state_with_history(state_path, "2026-04-14", 0.0, [])

    _FixedDatetime._fixed = datetime(2026, 4, 14, 10, 0, 0)
    with patch("garlic.cli.datetime", _FixedDatetime):
        cmd_stats(argparse.Namespace())

    out = capsys.readouterr().out
    assert "April 2026" in out
    assert "This month:" in out
    assert "0 active days" in out
    assert "Current streak:" in out
    assert "0 days" in out
    # No busiest day when everything is zero
    assert "Busiest day:" not in out
    # Honesty footer: makes the 30-day bound visible
    assert "last 30 days" in out


def test_cmd_stats_aggregates_and_streaks(garlic_env, capsys):
    """With populated history, stats computes totals, averages, streaks, busiest day."""
    _, _, state_path = garlic_env
    # Three consecutive days ending yesterday, plus 60m today
    history = [
        {"date": "2026-04-10", "minutes": 30.0},  # Fri
        {"date": "2026-04-12", "minutes": 90.0},  # Sun
        {"date": "2026-04-13", "minutes": 180.0},  # Mon (busiest)
    ]
    _write_state_with_history(state_path, "2026-04-14", 60.0, history)

    _FixedDatetime._fixed = datetime(2026, 4, 14, 10, 0, 0)
    with patch("garlic.cli.datetime", _FixedDatetime):
        cmd_stats(argparse.Namespace())

    out = capsys.readouterr().out
    # Month total = 30 + 90 + 180 + 60 = 360 min = 6h 00m
    assert "6h 00m" in out
    # 4 active days this month
    assert "4 active days" in out
    # Busiest day was Mon 04/13 at 3h
    assert "3h 00m" in out
    assert "Mon Apr 13" in out
    # Current streak: today (04-14), 04-13, 04-12 → 3 days (04-11 missing breaks)
    assert "Current streak:      3 days" in out
    # Longest streak: same 3 days
    assert "Longest streak:      3 days" in out


def test_cmd_stats_current_streak_excludes_today_when_zero(garlic_env, capsys):
    """When today has no time yet, current streak looks back from yesterday."""
    _, _, state_path = garlic_env
    history = [
        {"date": "2026-04-12", "minutes": 60.0},
        {"date": "2026-04-13", "minutes": 60.0},
    ]
    _write_state_with_history(state_path, "2026-04-14", 0.0, history)

    _FixedDatetime._fixed = datetime(2026, 4, 14, 10, 0, 0)
    with patch("garlic.cli.datetime", _FixedDatetime):
        cmd_stats(argparse.Namespace())

    out = capsys.readouterr().out
    # Streak ends yesterday: 2 days (04-13, 04-12)
    assert "Current streak:      2 days" in out


def test_cmd_stats_rolling_30d_window(garlic_env, capsys):
    """Rolling-30d total excludes entries older than 30 days."""
    _, _, state_path = garlic_env
    history = [
        # 40 days ago — outside the 30-day window
        {"date": "2026-03-05", "minutes": 120.0},
        # 10 days ago — inside the window
        {"date": "2026-04-04", "minutes": 60.0},
    ]
    _write_state_with_history(state_path, "2026-04-14", 0.0, history)

    _FixedDatetime._fixed = datetime(2026, 4, 14, 10, 0, 0)
    with patch("garlic.cli.datetime", _FixedDatetime):
        cmd_stats(argparse.Namespace())

    out = capsys.readouterr().out
    # Rolling 30d should only include 60 minutes (1h 00m), not 180
    rolling_line = [l for l in out.splitlines() if "Rolling 30d" in l][0]
    assert "1h 00m" in rolling_line
    assert "1 active day" in rolling_line


def test_cmd_setup_interactive_passes_overrides(capsys):
    """Interactive mode passes user overrides to install_hooks."""
    args = argparse.Namespace(yes=False, debug=False, defaults=False)
    inputs = iter(["60", "", "", "firm"])
    with patch("builtins.input", side_effect=inputs), \
         patch("garlic.cli.install_hooks") as mock_install:
        cmd_setup(args)
    call_kwargs = mock_install.call_args[1]
    assert call_kwargs["config_overrides"]["nudge_thresholds_minutes"] == [60, 120, 180, 240]
    assert call_kwargs["config_overrides"]["nudge_style"] == "firm"
