"""Tests for garlic set and garlic reset commands."""

import argparse
import tomllib

import pytest

from garlic.cli import cmd_set, cmd_reset, _parse_config_value


@pytest.fixture
def garlic_env(tmp_path, monkeypatch):
    """Set up a temporary garlic dir for config and state."""
    garlic_dir = tmp_path / ".garlic"
    garlic_dir.mkdir()
    config_path = garlic_dir / "config.toml"
    state_path = garlic_dir / "state.toml"

    # Write default config
    config_path.write_text(
        'max_prompt_gap_minutes = 20\n'
        'reset_hour = 2\n'
        'nudge_thresholds_minutes = [60, 120, 180, 240]\n'
        'nudge_style = "gentle"\n'
    )

    # Write state with some accumulated time
    state_path.write_text(
        'date = "2026-03-16"\n'
        'accumulated_minutes = 95.0\n'
        'last_event_time = 1710567890.123\n'
        'nudges_given = [60]\n'
        'ignored = false\n'
    )

    monkeypatch.setattr("garlic.config.GARLIC_DIR", garlic_dir)
    monkeypatch.setattr("garlic.config.CONFIG_PATH", config_path)
    monkeypatch.setattr("garlic.state.GARLIC_DIR", garlic_dir)
    monkeypatch.setattr("garlic.state.STATE_PATH", state_path)

    return garlic_dir, config_path, state_path


# --- garlic set tests ---


class TestParseConfigValue:
    def test_nudge_style_valid(self):
        assert _parse_config_value("nudge_style", "spicy") == "spicy"
        assert _parse_config_value("nudge_style", "firm") == "firm"
        assert _parse_config_value("nudge_style", "gentle") == "gentle"

    def test_nudge_style_invalid(self):
        with pytest.raises(ValueError, match="must be one of"):
            _parse_config_value("nudge_style", "aggressive")

    def test_int_keys(self):
        assert _parse_config_value("max_prompt_gap_minutes", "15") == 15
        assert _parse_config_value("reset_hour", "4") == 4

    def test_int_keys_invalid(self):
        with pytest.raises(ValueError, match="must be an integer"):
            _parse_config_value("reset_hour", "abc")

    def test_thresholds(self):
        assert _parse_config_value("nudge_thresholds_minutes", "30,60,90") == [30, 60, 90]

    def test_thresholds_invalid(self):
        with pytest.raises(ValueError, match="comma-separated integers"):
            _parse_config_value("nudge_thresholds_minutes", "30,abc,90")

    def test_unknown_key(self):
        with pytest.raises(ValueError, match="unknown config key"):
            _parse_config_value("nonexistent", "value")


def test_cmd_set_nudge_style(garlic_env, capsys):
    _, config_path, _ = garlic_env
    args = argparse.Namespace(assignment="nudge_style=spicy")
    cmd_set(args)

    out = capsys.readouterr().out
    assert "nudge_style" in out
    assert "spicy" in out

    with config_path.open("rb") as f:
        config = tomllib.load(f)
    assert config["nudge_style"] == "spicy"


def test_cmd_set_thresholds(garlic_env, capsys):
    _, config_path, _ = garlic_env
    args = argparse.Namespace(assignment="nudge_thresholds_minutes=30,60,120")
    cmd_set(args)

    with config_path.open("rb") as f:
        config = tomllib.load(f)
    assert config["nudge_thresholds_minutes"] == [30, 60, 120]


def test_cmd_set_int_value(garlic_env, capsys):
    _, config_path, _ = garlic_env
    args = argparse.Namespace(assignment="reset_hour=10")
    cmd_set(args)

    with config_path.open("rb") as f:
        config = tomllib.load(f)
    assert config["reset_hour"] == 10


def test_cmd_set_invalid_style(garlic_env, capsys):
    args = argparse.Namespace(assignment="nudge_style=invalid")
    with pytest.raises(SystemExit):
        cmd_set(args)


def test_cmd_set_unknown_key(garlic_env, capsys):
    args = argparse.Namespace(assignment="bogus=5")
    with pytest.raises(SystemExit):
        cmd_set(args)


def test_cmd_set_no_equals(garlic_env, capsys):
    args = argparse.Namespace(assignment="noequals")
    with pytest.raises(SystemExit):
        cmd_set(args)


# --- garlic reset tests ---


def test_cmd_reset_with_yes_flag(garlic_env, capsys):
    _, _, state_path = garlic_env
    args = argparse.Namespace(yes=True)
    cmd_reset(args)

    out = capsys.readouterr().out
    assert "timer reset" in out

    with state_path.open("rb") as f:
        state = tomllib.load(f)
    assert state["accumulated_minutes"] == 0.0
    assert state["nudges_given"] == []
    assert state["ignored"] is False
    assert state["last_event_time"] == 0.0


def test_cmd_reset_confirmed(garlic_env, capsys, monkeypatch):
    monkeypatch.setattr("builtins.input", lambda _: "y")
    args = argparse.Namespace(yes=False)
    cmd_reset(args)

    out = capsys.readouterr().out
    assert "timer reset" in out


def test_cmd_reset_declined(garlic_env, capsys, monkeypatch):
    _, _, state_path = garlic_env
    monkeypatch.setattr("builtins.input", lambda _: "n")
    args = argparse.Namespace(yes=False)
    cmd_reset(args)

    out = capsys.readouterr().out
    assert "cancelled" in out

    # State should be unchanged
    with state_path.open("rb") as f:
        state = tomllib.load(f)
    assert state["accumulated_minutes"] == 95.0
