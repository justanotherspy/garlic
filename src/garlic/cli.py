"""CLI entry point for garlic."""

import argparse
import sys


def cmd_setup(args: argparse.Namespace) -> None:
    """Install garlic hooks into ~/.claude/settings.json."""
    print("garlic: setup not yet implemented")


def cmd_status(args: argparse.Namespace) -> None:
    """Show accumulated active coding time today."""
    print("garlic: status not yet implemented")


def cmd_ignore(args: argparse.Namespace) -> None:
    """Disable nudging for the rest of the day."""
    print("garlic: ignore not yet implemented")


def cmd_hook(args: argparse.Namespace) -> None:
    """Handle a Claude Code hook event."""
    print(f"garlic: hook {args.hook_event} not yet implemented")


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="garlic",
        description="Track active coding time with Claude Code",
    )
    sub = parser.add_subparsers(dest="command")

    sub.add_parser("setup", help="Install hooks into ~/.claude/settings.json")
    sub.add_parser("status", help="Show accumulated active time today")
    sub.add_parser("ignore", help="Disable nudging for the day")

    hook_parser = sub.add_parser("hook", help="Handle a Claude Code hook event")
    hook_parser.add_argument(
        "hook_event",
        choices=["session-start", "prompt", "stop"],
        help="Which hook event to handle",
    )

    return parser


def main() -> None:
    parser = build_parser()
    args = parser.parse_args()

    if args.command is None:
        parser.print_help()
        sys.exit(1)

    dispatch = {
        "setup": cmd_setup,
        "status": cmd_status,
        "ignore": cmd_ignore,
        "hook": cmd_hook,
    }
    dispatch[args.command](args)
