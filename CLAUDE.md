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

## CI / Pull Request workflow
- CI runs automatically on every PR and push to `main` via `.github/workflows/ci.yml`.
- It runs `uv sync` then `uv run pytest` against Python 3.11 and 3.12.
- All PRs should be opened against `main`. The PR template (`.github/pull_request_template.md`) will pre-fill with a summary, test plan, and checklist.
- Do not merge a PR until CI is green.

## Architecture

### What garlic does
Garlic tracks how much time a user spends actively coding with Claude Code each day. It hooks into Claude Code's hook system, accumulates active time, and gently nudges the user to take breaks at configurable thresholds (inspired by Steve Yegge's "AI Vampire" article).

### Runtime files
- `~/.garlic/config.toml` — user configuration
- `~/.garlic/state.toml` — daily tracking state (locked with `fcntl.flock` for concurrent session safety)

### Config defaults (`~/.garlic/config.toml`)
```toml
max_prompt_gap_minutes = 20
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
- **Stop** (Claude finishes responding): compute gap = now − `last_event_time` (generation time). Cap at `max_prompt_gap_minutes`. Add to `accumulated_minutes`. Update `last_event_time` to now.
- **Prompt**: compute gap = now − `last_event_time` (reading/thinking time since Claude stopped). Cap at `max_prompt_gap_minutes`. Add to `accumulated_minutes`. Check thresholds; if a new one is crossed and not already in `nudges_given` and not `ignored`, output a nudge message to stdout and record the threshold.

Both stop and prompt accumulate time — the full engagement cycle is counted: watching Claude generate + reading the response + thinking before the next prompt.

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

# 🚨 SESSION CLOSE PROTOCOL 🚨

**CRITICAL**: Before saying "done" or "complete", you MUST run this checklist:

```
[ ] 1. git status              (check what changed)
[ ] 2. git add <files>         (stage code changes)
[ ] 3. git commit -m "..."     (commit code)
[ ] 4. git push                (push to remote)
```

**NEVER skip this.** Work is not done until pushed.

## Core Rules
- **Default**: Use beads for ALL task tracking (`bd create`, `bd ready`, `bd close`)
- **Prohibited**: Do NOT use TodoWrite, TaskCreate, or markdown files for task tracking
- **Workflow**: Create beads issue BEFORE writing code, mark in_progress when starting
- **Memory**: Use `bd remember "insight"` for persistent knowledge across sessions. Do NOT use MEMORY.md files — they fragment across accounts. Search with `bd memories <keyword>`.
- Persistence you don't need beats lost context
- Git workflow: run `cd .beads/dolt/garlic && dolt push` to sync with dolthub
- Session management: check `bd ready` for available work

## Essential Commands

### Finding Work
- `bd ready` - Show issues ready to work (no blockers)
- `bd list --status=open` - All open issues
- `bd list --status=in_progress` - Your active work
- `bd show <id>` - Detailed issue view with dependencies

### Creating & Updating
- `bd create --title="Summary of this issue" --description="Why this issue exists and what needs to be done" --type=task|bug|feature --priority=2` - New issue
  - Priority: 0-4 or P0-P4 (0=critical, 2=medium, 4=backlog). NOT "high"/"medium"/"low"
- `bd update <id> --status=in_progress` - Claim work
- `bd update <id> --assignee=username` - Assign to someone
- `bd update <id> --title/--description/--notes/--design` - Update fields inline
- `bd close <id>` - Mark complete
- `bd close <id1> <id2> ...` - Close multiple issues at once (more efficient)
- `bd close <id> --reason="explanation"` - Close with reason
- **Tip**: When creating multiple issues/tasks/epics, use parallel subagents for efficiency
- **WARNING**: Do NOT use `bd edit` - it opens $EDITOR (vim/nano) which blocks agents

### Dependencies & Blocking
- `bd dep add <issue> <depends-on>` - Add dependency (issue depends on depends-on)
- `bd blocked` - Show all blocked issues
- `bd show <id>` - See what's blocking/blocked by this issue

### Sync & Collaboration
- `bd dolt push` - Push beads to Dolt remote
- `bd dolt pull` - Pull beads from Dolt remote
- `bd search <query>` - Search issues by keyword

### Project Health
- `bd stats` - Project statistics (open/closed/blocked counts)
- `bd doctor` - Check for issues (sync problems, missing hooks)

## Common Workflows

**Starting work:**
```bash
bd ready           # Find available work
bd show <id>       # Review issue details
bd update <id> --status=in_progress  # Claim it
```

**Completing work:**
```bash
bd close <id1> <id2> ...    # Close all completed issues at once
git add . && git commit -m "..."  # Commit code changes
git push                    # Push to remote
```

**Creating dependent work:**
```bash
# Run bd create commands in parallel (use subagents for many items)
bd create --title="Implement feature X" --description="Why this issue exists and what needs to be done" --type=feature
bd create --title="Write tests for X" --description="Why this issue exists and what needs to be done" --type=task
bd dep add beads-yyy beads-xxx  # Tests depend on Feature (Feature blocks tests)
```

<!-- END BEADS INTEGRATION -->
