"""Tests for the garlic CLI."""

import argparse
import importlib.metadata
from unittest.mock import patch

from garlic.cli import _prompt_config, build_parser, cmd_setup, cmd_version


def test_parser_version():
    parser = build_parser()
    args = parser.parse_args(["version"])
    assert args.command == "version"


def test_cmd_version_output(capsys):
    cmd_version(None)
    out = capsys.readouterr().out.strip()
    expected = f"garlic {importlib.metadata.version('garlic-cli')}"
    assert out == expected


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
    args = argparse.Namespace(yes=True, debug=False)
    with patch("garlic.cli.install_hooks") as mock_install:
        cmd_setup(args)
    mock_install.assert_called_once_with(debug=False, config_overrides={})
    out = capsys.readouterr().out
    assert "hooks installed" in out


def test_cmd_setup_interactive_passes_overrides(capsys):
    """Interactive mode passes user overrides to install_hooks."""
    args = argparse.Namespace(yes=False, debug=False)
    inputs = iter(["60", "", "", "firm"])
    with patch("builtins.input", side_effect=inputs), \
         patch("garlic.cli.install_hooks") as mock_install:
        cmd_setup(args)
    call_kwargs = mock_install.call_args[1]
    assert call_kwargs["config_overrides"]["nudge_thresholds_minutes"] == [60, 120, 180, 240]
    assert call_kwargs["config_overrides"]["nudge_style"] == "firm"
