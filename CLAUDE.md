# CLAUDE.md — garlic developer guide

## Package management
- Use **`uv`** for all package management. Never use `pip`, `pip3`, or `pipx`.
- Install dependencies: `uv sync`
- Run commands in the project environment: `uv run <command>`
- Add a dependency: `uv add <package>`
- Add a dev dependency: `uv add --dev <package>`

## Project philosophy
- **Standard library first.** Prefer Python's stdlib over third-party packages wherever possible. This keeps the supply chain small and the install footprint minimal — intentional choices for a tool people trust to run on every prompt.
- When a third-party dependency is genuinely needed, discuss it first; the bar is high.

## Project structure
- Config lives in `pyproject.toml` (no `setup.py` or `setup.cfg`).
- Source code uses a `src/` layout: `src/garlic/`.
- The CLI entry point is declared as a `[project.scripts]` entry in `pyproject.toml`.

## Testing
- Run tests with `uv run pytest`.
- Keep tests fast; avoid network calls in unit tests.
- Add pytest as a dev dependency: `uv add --dev pytest`

## Architecture

### What garlic does
Garlic tracks how much time a user spends actively coding with Claude Code each day. It hooks into Claude Code's hook system, accumulates active time, and gently nudges the user to take breaks at configurable thresholds (inspired by Steve Yegge's "AI Vampire" article).

### Runtime files
- `~/.garlic/config.toml` — user configuration
- `~/.garlic/state.toml` — daily tracking state (locked with `fcntl.flock` for concurrent session safety)

### Config defaults (`~/.garlic/config.toml`)
```toml
max_prompt_gap_minutes = 10
reset_hour = 2
nudge_thresholds_minutes = [60, 120, 180, 240]
nudge_style = "gentle"
```

### State file (`~/.garlic/state.toml`)
```toml
date = "2026-03-16"
accumulated_minutes = 0.0
last_event_time = 1710567890.123
nudges_given = []
ignored = false
```
When `date` doesn't match the current day (accounting for `reset_hour`), state resets.

### CLI subcommands
- `garlic setup` — install hooks into `~/.claude/settings.json` (idempotent)
- `garlic status` — show accumulated active time today
- `garlic ignore` — disable nudging for the day (tracking continues)
- `garlic hook session-start` — called by SessionStart hook, reads JSON from stdin
- `garlic hook prompt` — called by UserPromptSubmit hook, reads JSON from stdin, outputs nudge if threshold crossed
- `garlic hook stop` — called by Stop hook, reads JSON from stdin, updates last_event_time

### Source modules (`src/garlic/`)
- `__init__.py`
- `cli.py` — argparse entry point, subcommand dispatch
- `config.py` — load/create config with defaults, uses `tomllib` for reading
- `state.py` — state file read/write with `fcntl.flock`, day reset logic
- `engine.py` — time gap calculation, accumulation, threshold checking
- `nudges.py` — hardcoded message pools (gentle/firm/spicy), random selection
- `hooks.py` — hook subcommand handlers (session-start, prompt, stop)
- `setup.py` — install/update hooks in `~/.claude/settings.json`

### Time tracking model
- **Session start**: record timestamp as `last_event_time`
- **Stop** (Claude finishes responding): update `last_event_time` to now (no accumulation, no cap — just moves the marker so the next gap measures from when Claude stopped, not from when the user prompted)
- **Prompt**: compute gap = now − `last_event_time`. If gap > `max_prompt_gap_minutes`, cap it. Add capped gap to `accumulated_minutes`. Check thresholds; if a new one is crossed and not already in `nudges_given` and not `ignored`, output a nudge message to stdout and record the threshold.

### Claude Code hooks installed by `garlic setup`
Hooks go in `~/.claude/settings.json` under the `hooks` key:
- **SessionStart** (matcher: `"startup"`) → `garlic hook session-start`
- **UserPromptSubmit** → `garlic hook prompt`
- **Stop** → `garlic hook stop`

Each hook reads JSON from stdin (contains `session_id`, `cwd`, etc.) and returns plain text on stdout (for nudges) or exits silently.

### Zero third-party dependencies
- `tomllib` (stdlib, Python 3.11+) for reading TOML
- Manual TOML writing (simple key-value format, no library needed)
- `fcntl` for file locking
- `argparse` for CLI
- `json` for hook stdin parsing

### Nudge styles
Three hardcoded message pools: `gentle`, `firm`, `spicy`. Configurable via `nudge_style` in config. Messages reference accumulated time (e.g., "~2 hours"). One random message per threshold crossing.

<!-- BEGIN BEADS INTEGRATION -->
## Issue Tracking

This project uses **bd (beads)** for issue tracking.
Run `bd prime` for workflow context.

**Quick reference:**
- `bd ready` - Find unblocked work
- `bd create "Title" --type task --priority 2` - Create issue
- `bd close <id>` - Complete work
- `bd dolt push` - Push beads to remote

For full workflow details: `bd prime`
<!-- END BEADS INTEGRATION -->
