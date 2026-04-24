"""CLI entry point for garlic."""

import argparse
import importlib.metadata
import sys
from datetime import datetime, timedelta

from garlic._format import format_duration
from garlic.config import DEFAULTS, load_config, save_config
from garlic.hooks import hook_prompt, hook_session_end, hook_session_start, hook_stop
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
    default_interval = DEFAULTS["nudge_thresholds_minutes"][0]
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

    if args.defaults:
        if not args.yes:
            try:
                answer = input(
                    "garlic: overwrite config with built-in defaults? [y/N] "
                )
            except (EOFError, KeyboardInterrupt):
                print("\ngarlic: setup cancelled")
                sys.exit(1)
            if answer.strip().lower() not in ("y", "yes"):
                print("garlic: setup cancelled")
                return
        config_overrides = dict(DEFAULTS)
    elif not args.yes:
        try:
            config_overrides = _prompt_config()
        except (EOFError, KeyboardInterrupt):
            print("\ngarlic: setup cancelled")
            sys.exit(1)

    install_hooks(debug=args.debug, config_overrides=config_overrides)
    mode = " (debug mode)" if args.debug else ""
    print(f"garlic: hooks installed in ~/.claude/settings.json{mode}")
    print("garlic: /garlic slash command installed in ~/.claude/commands/")
    print("garlic: nudge-relay instruction installed in ~/.claude/CLAUDE.md")
    if args.defaults:
        print("garlic: config reset to built-in defaults")


_MONTH_NAMES = (
    "January", "February", "March", "April", "May", "June",
    "July", "August", "September", "October", "November", "December",
)
_MONTH_ABBR = (
    "Jan", "Feb", "Mar", "Apr", "May", "Jun",
    "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
)
_WEEKDAY_ABBR = ("Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun")


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
    time_str = format_duration(minutes)
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
        # Crossed thresholds on one compact line
        crossed = [t for t in thresholds if t in nudges_given]
        if crossed:
            marks = "  ".join(f"\u2716 {format_duration(t)}" for t in crossed)
            print(f"  {marks}")

        # All thresholds passed
        if all(t in nudges_given for t in thresholds):
            print(f"\n  \U0001f9db You've crossed every threshold. Respect.")
        else:
            # Next nudge progress bar
            next_t = next((t for t in thresholds if t not in nudges_given), None)
            if next_t is not None:
                prev_t = max((t for t in thresholds if t in nudges_given), default=0)
                span = next_t - prev_t
                progress = (minutes - prev_t) / span if span > 0 else 1.0
                bar = _progress_bar(progress)
                remaining = format_duration(max(next_t - minutes, 0))
                print(f"  {bar} {format_duration(next_t)} ({remaining} to go)")

            # Day progress bar (0 to max threshold)
            max_t = thresholds[-1]
            day_fraction = minutes / max_t if max_t > 0 else 1.0
            day_bar = _progress_bar(day_fraction)
            print(f"  {day_bar} {format_duration(max_t)} day")


def cmd_week(args: argparse.Namespace) -> None:
    """Show a rolling 7-day usage summary."""
    config = load_config()
    state = load_state(config["reset_hour"])

    # Build lookup from history
    history_map: dict[str, float] = {}
    for entry in state.get("history", []):
        history_map[entry["date"]] = entry["minutes"]

    # Today's date (shifted by reset_hour)
    now = datetime.now()
    if now.hour < config["reset_hour"]:
        now -= timedelta(days=1)
    today_str = now.strftime("%Y-%m-%d")

    # Build 7-day window (oldest first)
    days: list[tuple[str, float]] = []
    for i in range(6, -1, -1):
        d = now - timedelta(days=i)
        date_str = d.strftime("%Y-%m-%d")
        if date_str == today_str:
            minutes = state["accumulated_minutes"]
        else:
            minutes = history_map.get(date_str, 0.0)
        days.append((date_str, minutes))

    # Daily target = max nudge threshold
    thresholds = config.get("nudge_thresholds_minutes", [])
    target = max(thresholds) if thresholds else 240
    bar_max = target  # bars are relative to the daily target

    print("\U0001f9c4 Weekly usage (last 7 days)\n")

    total = 0.0
    under_target = 0
    for date_str, minutes in days:
        d = datetime.strptime(date_str, "%Y-%m-%d")
        day_label = d.strftime("%a %m/%d")
        time_str = format_duration(minutes)
        fraction = minutes / bar_max if bar_max > 0 else 0.0
        bar = _progress_bar(fraction, width=16)
        marker = "  \u2190 today" if date_str == today_str else ""
        print(f"  {day_label}   {time_str:>7s}  {bar}{marker}")
        total += minutes
        if minutes < target:
            under_target += 1

    target_str = format_duration(target)
    total_str = format_duration(total)
    print(f"\n  Total: {total_str} \u00b7 {under_target} of 7 days under {target_str} target")


def cmd_stats(args: argparse.Namespace) -> None:
    """Show higher-level stats: monthly totals, streaks, averages."""
    config = load_config()
    state = load_state(config["reset_hour"])

    # Build date -> minutes map, including today's in-progress minutes
    history_map: dict[str, float] = {}
    for entry in state.get("history", []):
        history_map[entry["date"]] = entry["minutes"]

    now = datetime.now()
    if now.hour < config["reset_hour"]:
        now -= timedelta(days=1)
    today_str = now.strftime("%Y-%m-%d")
    today_minutes = state["accumulated_minutes"]
    if today_minutes > 0:
        history_map[today_str] = today_minutes

    # This calendar month
    month_prefix = now.strftime("%Y-%m")
    month_entries = [m for d, m in history_map.items() if d.startswith(month_prefix)]
    month_total = sum(month_entries)
    month_active = [m for m in month_entries if m > 0]
    month_avg = sum(month_active) / len(month_active) if month_active else 0.0

    # Busiest day across all recorded history
    busiest: tuple[str, float] | None = None
    for d, m in history_map.items():
        if m > 0 and (busiest is None or m > busiest[1]):
            busiest = (d, m)

    # Streaks — based on dates with any recorded time
    active_dates = {d for d, m in history_map.items() if m > 0}

    # Current streak: consecutive days ending today (or yesterday if today is 0)
    current_streak = 0
    cursor = now
    if cursor.strftime("%Y-%m-%d") not in active_dates:
        cursor -= timedelta(days=1)
    while cursor.strftime("%Y-%m-%d") in active_dates:
        current_streak += 1
        cursor -= timedelta(days=1)

    # Longest streak: scan sorted active dates for consecutive runs
    longest_streak = 0
    if active_dates:
        sorted_dts = sorted(datetime.strptime(s, "%Y-%m-%d") for s in active_dates)
        longest_streak = run = 1
        for i in range(1, len(sorted_dts)):
            if (sorted_dts[i] - sorted_dts[i - 1]).days == 1:
                run += 1
                longest_streak = max(longest_streak, run)
            else:
                run = 1

    # Rolling 30 days (inclusive of today)
    thirty_ago = now - timedelta(days=29)
    rolling_total = 0.0
    rolling_active = 0
    for d_str, m in history_map.items():
        d_dt = datetime.strptime(d_str, "%Y-%m-%d")
        if thirty_ago.date() <= d_dt.date() <= now.date():
            rolling_total += m
            if m > 0:
                rolling_active += 1

    # Output — locale-independent via hardcoded English names
    month_label = f"{_MONTH_NAMES[now.month - 1]} {now.year}"
    print(f"\U0001f9c4 Stats — {month_label}\n")
    print(
        f"  This month:      {format_duration(month_total):>8s}"
        f"   ({len(month_active)} active day{'s' if len(month_active) != 1 else ''})"
    )
    if month_active:
        print(f"  Daily avg:       {format_duration(month_avg):>8s}   (active days only)")
    if busiest is not None:
        b_dt = datetime.strptime(busiest[0], "%Y-%m-%d")
        b_label = (
            f"{_WEEKDAY_ABBR[b_dt.weekday()]} "
            f"{_MONTH_ABBR[b_dt.month - 1]} {b_dt.day:02d}"
        )
        print(f"  Busiest day:     {format_duration(busiest[1]):>8s}   ({b_label})")
    print(
        f"  Current streak:  {current_streak:>5d} day{'s' if current_streak != 1 else ''}"
    )
    print(
        f"  Longest streak:  {longest_streak:>5d} day{'s' if longest_streak != 1 else ''}"
    )
    print()
    print(
        f"  Rolling 30d:     {format_duration(rolling_total):>8s}"
        f"   ({rolling_active} active day{'s' if rolling_active != 1 else ''})"
    )
    # Honesty footer: garlic only retains the last 30 days of per-day history
    # (see HISTORY_MAX in state.py), so busiest-day and streaks are bounded
    # by that window — they cannot reflect activity older than ~30 days.
    print("\n  Note: based on the last 30 days of history.")


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
            values = [int(x.strip()) for x in raw.split(",")]
        except ValueError:
            raise ValueError(
                "nudge_thresholds_minutes must be comma-separated integers (e.g. 60,120,180)"
            )
        if not values:
            raise ValueError("nudge_thresholds_minutes must not be empty")
        if any(v <= 0 for v in values):
            raise ValueError(
                "nudge_thresholds_minutes values must be positive integers"
            )
        if values != sorted(values):
            raise ValueError(
                "nudge_thresholds_minutes must be in ascending order "
                f"(got {','.join(map(str, values))}; did you mean {','.join(map(str, sorted(values)))}?)"
            )
        if len(values) != len(set(values)):
            raise ValueError("nudge_thresholds_minutes must not contain duplicates")
        return values
    if key in ("max_prompt_gap_minutes", "max_generation_minutes", "reset_hour"):
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
    state["bedtime_nudge_given"] = False
    save_state(state)
    print("garlic: timer reset for today")


def cmd_hook(args: argparse.Namespace) -> None:
    """Handle a Claude Code hook event."""
    dispatch = {
        "session-start": hook_session_start,
        "prompt": hook_prompt,
        "stop": hook_stop,
        "session-end": hook_session_end,
    }
    dispatch[args.hook_event](debug=args.debug)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="garlic",
        description="Track active coding time with Claude Code",
    )
    parser.add_argument(
        "--version",
        action="version",
        version=f"garlic {importlib.metadata.version('garlic-cli')}",
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
    setup_parser.add_argument(
        "--defaults", action="store_true",
        help="Overwrite existing config with built-in defaults",
    )

    sub.add_parser("status", help="Show accumulated active time today")
    sub.add_parser("week", help="Show rolling 7-day usage summary")
    sub.add_parser("stats", help="Show monthly totals, streaks, and averages")
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
        choices=["session-start", "prompt", "stop", "session-end"],
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
        "week": cmd_week,
        "stats": cmd_stats,
        "ignore": cmd_ignore,
        "set": cmd_set,
        "reset": cmd_reset,
        "hook": cmd_hook,
    }
    dispatch[args.command](args)
