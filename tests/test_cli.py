"""Tests for the garlic CLI."""

import importlib.metadata

from garlic.cli import build_parser, cmd_version


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
