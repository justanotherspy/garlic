"""Tests for the garlic CLI."""

from garlic.cli import build_parser


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
