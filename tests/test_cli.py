"""Tests for the garlic CLI."""

import argparse
import importlib.metadata
from unittest.mock import patch

from garlic.cli import (
    _check_latest_version,
    _parse_version,
    _prompt_config,
    build_parser,
    cmd_setup,
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
