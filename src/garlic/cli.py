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
    install_hooks(debug=args.debug)
    mode = " (debug mode)" if args.debug else ""
    print(f"garlic: hooks installed in ~/.claude/settings.json{mode}")
    print("garlic: /garlic slash command installed in ~/.claude/commands/")


def _format_duration(minutes: float) -> str:
    """Format minutes as a human-readable duration string."""
    hours = int(minutes // 60)
    mins = int(minutes % 60)
    if hours > 0:
        return f"{hours}h {mins:02d}m"
    return f"{mins}m"


def _progress_bar(fraction: float, width: int = 20) -> str:
    """Build an ASCII progress bar. fraction is 0.0–1.0+."""
    filled = min(int(fraction * width), width)
    bar = "\u2588" * filled + "\u2591" * (width - filled)
    return bar


def cmd_status(args: argparse.Namespace) -> None:
    """Show accumulated active coding time today."""
    config = load_config()
    state = load_state(config["reset_hour"])

    minutes = state["accumulated_minutes"]
    time_str = _format_duration(minutes)
    thresholds = sorted(config.get("nudge_thresholds_minutes", []))
    nudges_given = state.get("nudges_given", [])
    ignored = state.get("ignored", False)

    # Header — ignored state is front and center if active
    if ignored:
        print(f"\U0001f9db {time_str} of active coding today (nudging paused)")
        print(f"   run 'garlic ignore' to resume")
    else:
        # Pick icon based on progress toward next threshold
        next_t = next((t for t in thresholds if t not in nudges_given), None)
        if next_t is not None:
            fraction = minutes / next_t if next_t > 0 else 1.0
        else:
            fraction = 1.0

        if fraction < 0.5:
            icon = "\U0001f9c4"  # garlic — safe zone
        elif fraction < 0.85:
            icon = "\U0001f9c4"  # garlic — still ok
        else:
            icon = "\U0001f9db"  # vampire — getting close

        print(f"{icon} {time_str} of active coding today")

    # Threshold progress
    if thresholds:
        print()
        for t in thresholds:
            passed = t in nudges_given
            label = _format_duration(t)
            if passed:
                print(f"  \u2716 {label}")
            elif minutes < t:
                fraction = minutes / t if t > 0 else 1.0
                bar = _progress_bar(fraction)
                remaining = _format_duration(t - minutes)
                print(f"  {bar} {label} ({remaining} to go)")
            else:
                # Crossed but not yet in nudges_given (edge case)
                print(f"  \u2716 {label}")

        # If all thresholds passed
        if all(t in nudges_given for t in thresholds):
            print(f"\n  \U0001f9db You've crossed every threshold. Respect.")


def cmd_ignore(args: argparse.Namespace) -> None:
    """Toggle nudging for the rest of the day."""
    config = load_config()
    state = load_state(config["reset_hour"])
    if state.get("ignored", False):
        state["ignored"] = False
        save_state(state)
        print("garlic: nudging resumed")
    else:
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
    dispatch[args.hook_event](debug=args.debug)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="garlic",
        description="Track active coding time with Claude Code",
    )
    sub = parser.add_subparsers(dest="command")

    sub.add_parser("version", help="Show installed version")

    setup_parser = sub.add_parser("setup", help="Install hooks into ~/.claude/settings.json")
    setup_parser.add_argument(
        "--debug", action="store_true", help="Install hooks with debug logging"
    )

    sub.add_parser("status", help="Show accumulated active time today")
    sub.add_parser("ignore", help="Disable nudging for the day")

    hook_parser = sub.add_parser("hook", help="Handle a Claude Code hook event")
    hook_parser.add_argument(
        "hook_event",
        choices=["session-start", "prompt", "stop"],
        help="Which hook event to handle",
    )
    hook_parser.add_argument(
        "--debug", action="store_true", help="Log gap calculations to stderr"
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
