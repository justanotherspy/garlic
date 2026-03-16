"""CLI entry point for garlic."""

import argparse
import importlib.metadata
import sys

from garlic.config import load_config
from garlic.hooks import hook_prompt, hook_session_start, hook_stop
from garlic.setup import install_hooks
from garlic.state import load_state, save_state


def cmd_version(args: argparse.Namespace) -> None:
    """Print the installed version."""
    version = importlib.metadata.version("garlic-cli")
    print(f"garlic {version}")


def cmd_setup(args: argparse.Namespace) -> None:
    """Install garlic hooks into ~/.claude/settings.json."""
    install_hooks()
    print("garlic: hooks installed in ~/.claude/settings.json")


def cmd_status(args: argparse.Namespace) -> None:
    """Show accumulated active coding time today."""
    config = load_config()
    state = load_state(config["reset_hour"])

    minutes = state["accumulated_minutes"]
    hours = int(minutes // 60)
    mins = int(minutes % 60)

    if hours > 0:
        time_str = f"{hours}h {mins:02d}m"
    else:
        time_str = f"{mins}m"

    print(f"{time_str} of active coding today")

    # Show threshold progress
    thresholds = config.get("nudge_thresholds_minutes", [])
    nudges_given = state.get("nudges_given", [])
    for t in sorted(thresholds):
        marker = "x" if t in nudges_given else " "
        t_hours = int(t // 60)
        t_mins = int(t % 60)
        label = f"{t_hours}h {t_mins:02d}m" if t_hours > 0 else f"{t_mins}m"
        print(f"  [{marker}] {label}")

    if state.get("ignored", False):
        print("  (nudging ignored for today)")


def cmd_ignore(args: argparse.Namespace) -> None:
    """Disable nudging for the rest of the day."""
    config = load_config()
    state = load_state(config["reset_hour"])
    state["ignored"] = True
    save_state(state)
    print("garlic: nudging disabled for today (tracking continues)")


def cmd_hook(args: argparse.Namespace) -> None:
    """Handle a Claude Code hook event."""
    dispatch = {
        "session-start": hook_session_start,
        "prompt": hook_prompt,
        "stop": hook_stop,
    }
    dispatch[args.hook_event]()


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="garlic",
        description="Track active coding time with Claude Code",
    )
    sub = parser.add_subparsers(dest="command")

    sub.add_parser("version", help="Show installed version")
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
        "version": cmd_version,
        "setup": cmd_setup,
        "status": cmd_status,
        "ignore": cmd_ignore,
        "hook": cmd_hook,
    }
    dispatch[args.command](args)
