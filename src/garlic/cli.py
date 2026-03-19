"""CLI entry point for garlic."""

import argparse
import importlib.metadata
import sys

from garlic.config import DEFAULTS, load_config, save_config
from garlic.hooks import hook_prompt, hook_session_start, hook_stop
from garlic.setup import install_hooks
from garlic.state import load_state, save_state


def _check_latest_version(current: str) -> str | None:
    """Fetch the latest garlic-cli version from PyPI. Returns it if newer, else None."""
    import json as _json
    import urllib.request

    try:
        req = urllib.request.Request(
            "https://pypi.org/pypi/garlic-cli/json",
            headers={"Accept": "application/json"},
        )
        with urllib.request.urlopen(req, timeout=3) as resp:
            data = _json.loads(resp.read())
        latest = data["info"]["version"]
        if _parse_version(latest) > _parse_version(current):
            return latest
    except Exception:
        pass
    return None


def _parse_version(v: str) -> tuple[int, ...]:
    """Parse a PEP 440-ish version string into a comparable tuple."""
    parts: list[int] = []
    for seg in v.split("."):
        digits = ""
        for ch in seg:
            if ch.isdigit():
                digits += ch
            else:
                break
        parts.append(int(digits) if digits else 0)
    return tuple(parts)


def cmd_version(args: argparse.Namespace) -> None:
    """Print the installed version and check for updates."""
    version = importlib.metadata.version("garlic-cli")
    print(f"garlic {version}")
    latest = _check_latest_version(version)
    if latest:
        print(f"  update available: {latest}")
        print(f"  run: uv tool upgrade garlic-cli")


def _prompt_config() -> dict[str, object]:
    """Interactively prompt the user for config values, returning overrides."""
    overrides: dict[str, object] = {}

    print("garlic setup — configure your preferences (press Enter for defaults)\n")

    # nudge_thresholds_minutes — ask as a single "nudge interval" for simplicity
    default_interval = 30
    raw = input(f"  Nudge interval in minutes [{default_interval}]: ").strip()
    if raw:
        try:
            interval = int(raw)
            if interval < 1:
                raise ValueError
            cap = DEFAULTS["nudge_thresholds_minutes"][-1]
            overrides["nudge_thresholds_minutes"] = list(
                range(interval, cap + 1, interval)
            )
        except ValueError:
            print(f"  (invalid, using default {default_interval})")

    # max_prompt_gap_minutes
    default_gap = DEFAULTS["max_prompt_gap_minutes"]
    raw = input(f"  Max prompt gap in minutes [{default_gap}]: ").strip()
    if raw:
        try:
            gap = int(raw)
            if gap < 1:
                raise ValueError
            overrides["max_prompt_gap_minutes"] = gap
        except ValueError:
            print(f"  (invalid, using default {default_gap})")

    # reset_hour
    default_reset = DEFAULTS["reset_hour"]
    raw = input(f"  Daily reset hour (0-23) [{default_reset}]: ").strip()
    if raw:
        try:
            hour = int(raw)
            if not (0 <= hour <= 23):
                raise ValueError
            overrides["reset_hour"] = hour
        except ValueError:
            print(f"  (invalid, using default {default_reset})")

    # nudge_style
    default_style = DEFAULTS["nudge_style"]
    raw = input(
        f"  Nudge style (gentle/firm/spicy) [{default_style}]: "
    ).strip().lower()
    if raw:
        if raw in VALID_NUDGE_STYLES:
            overrides["nudge_style"] = raw
        else:
            print(f"  (invalid, using default {default_style})")

    print()
    return overrides


def cmd_setup(args: argparse.Namespace) -> None:
    """Install garlic hooks into ~/.claude/settings.json."""
    config_overrides: dict[str, object] = {}

    if not args.yes:
        try:
            config_overrides = _prompt_config()
        except (EOFError, KeyboardInterrupt):
            print("\ngarlic: setup cancelled")
            sys.exit(1)

    install_hooks(debug=args.debug, config_overrides=config_overrides)
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


VALID_NUDGE_STYLES = ("gentle", "firm", "spicy")


def _parse_config_value(key: str, raw: str) -> object:
    """Parse and validate a config value string for the given key."""
    if key == "nudge_style":
        if raw not in VALID_NUDGE_STYLES:
            raise ValueError(
                f"nudge_style must be one of: {', '.join(VALID_NUDGE_STYLES)}"
            )
        return raw
    if key == "nudge_thresholds_minutes":
        try:
            return [int(x.strip()) for x in raw.split(",")]
        except ValueError:
            raise ValueError(
                "nudge_thresholds_minutes must be comma-separated integers (e.g. 60,120,180)"
            )
    if key in ("max_prompt_gap_minutes", "reset_hour"):
        try:
            return int(raw)
        except ValueError:
            raise ValueError(f"{key} must be an integer")
    raise ValueError(
        f"unknown config key: {key}. Valid keys: {', '.join(DEFAULTS)}"
    )


def cmd_set(args: argparse.Namespace) -> None:
    """Update a config value."""
    if "=" not in args.assignment:
        print(f"garlic: expected KEY=VALUE, got '{args.assignment}'", file=sys.stderr)
        sys.exit(1)

    key, raw = args.assignment.split("=", 1)
    key = key.strip()
    raw = raw.strip()

    try:
        value = _parse_config_value(key, raw)
    except ValueError as e:
        print(f"garlic: {e}", file=sys.stderr)
        sys.exit(1)

    config = load_config()
    config[key] = value
    save_config(config)
    print(f"garlic: set {key} = {value}")


def cmd_reset(args: argparse.Namespace) -> None:
    """Reset the daily timer after user confirmation."""
    if not args.yes:
        try:
            answer = input("garlic: reset timer to zero for today? [y/N] ")
        except (EOFError, KeyboardInterrupt):
            print()
            sys.exit(1)
        if answer.strip().lower() not in ("y", "yes"):
            print("garlic: reset cancelled")
            return

    config = load_config()
    state = load_state(config["reset_hour"])
    state["accumulated_minutes"] = 0.0
    state["nudges_given"] = []
    state["ignored"] = False
    state["last_event_time"] = 0.0
    save_state(state)
    print("garlic: timer reset for today")


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
    setup_parser.add_argument(
        "-y", "--yes", action="store_true",
        help="Skip interactive prompts and use all defaults",
    )

    sub.add_parser("status", help="Show accumulated active time today")
    sub.add_parser("ignore", help="Toggle nudging for the day")

    set_parser = sub.add_parser("set", help="Update a config value (KEY=VALUE)")
    set_parser.add_argument(
        "assignment", help="Config assignment (e.g. nudge_style=spicy)"
    )

    reset_parser = sub.add_parser("reset", help="Reset daily timer to zero")
    reset_parser.add_argument(
        "-y", "--yes", action="store_true", help="Skip confirmation prompt"
    )

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
        "set": cmd_set,
        "reset": cmd_reset,
        "hook": cmd_hook,
    }
    dispatch[args.command](args)
