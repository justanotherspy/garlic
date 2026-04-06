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
- **All tests must use the shared `garlic_env` fixture** from `tests/conftest.py`. This redirects `GARLIC_DIR`, `CONFIG_PATH`, and `STATE_PATH` to a temporary directory so tests never touch the user's real `~/.garlic/`. If a test needs custom state or config, override the file contents after requesting the fixture — never patch paths manually.
- **Never run garlic commands (e.g. `save_state`, `garlic status`) against real paths in tests or ad-hoc scripts.** The round-trip test that corrupted `~/.garlic/state.toml` is why this rule exists.

## Commit messages

Use **Conventional Commits** format. This is required — `git-cliff` parses commit messages to generate the changelog automatically.

```
<type>(<scope>): <description>
```

Types: `feat`, `fix`, `docs`, `refactor`, `test`, `chore`, `ci`, `perf`, `style`

Examples:
- `feat(setup): add --defaults flag to overwrite config`
- `fix(engine): drop gaps exceeding cap`
- `docs: update README with new defaults`
- `chore(release): v0.1.7` (used only by `make release`)

Include the bead ID in the description when working on a bead (e.g. `feat(setup): add interactive prompts (garlic-v5k)`).

## Development workflow (bead → PR → merge)

1. **Claim the bead**: `bd update <id> --status=in_progress`
2. **Ensure main is up to date**: `git checkout main && git pull`
3. **Create a feature branch**: `git checkout -b <bead-id>/<short-description>`
4. **Implement, test, commit** using conventional commit messages. Update `README.md` if any user-facing behaviour changed.
5. **Push and open a PR**: branch name and PR title must include the bead ID (e.g. `garlic-abc`). One bead per PR.
6. **PR description** must have two sections: **Goal** (the problem/feature from the bead) and **Solution** (how it was implemented).
7. **Wait for CI** — do not merge until green.
8. **After the user confirms the PR is merged**: `bd close <id> --reason="PR #N merged"`, then `git checkout main && git pull`.

**Never push code directly to main.** The only exception is `make release`, which pushes release metadata (changelog, version bump) directly.

## CI
- CI runs automatically on every PR and push to `main` via `.github/workflows/ci.yml`.
- It runs `uv sync` then `uv run pytest` against Python 3.11 and 3.12.

## Releasing

Run `make release` from a clean, up-to-date main branch. It will:
1. Checkout and pull latest main
2. Run tests
3. Bump the patch version
4. Auto-generate `CHANGELOG.md` via `git-cliff`
5. Commit, tag, and push to main
6. Create a GitHub release with notes extracted from the changelog
7. Publish to PyPI

This is the **only** workflow that pushes directly to main.

## Architecture

### What garlic does
Garlic tracks how much time a user spends actively coding with Claude Code each day. It hooks into Claude Code's hook system, accumulates active time, and gently nudges the user to take breaks at configurable thresholds (inspired by Steve Yegge's "AI Vampire" article).

### Runtime files
- `~/.garlic/config.toml` — user configuration
- `~/.garlic/state.toml` — daily tracking state (locked with `fcntl.flock` for concurrent session safety)

### Config defaults (`~/.garlic/config.toml`)
```toml
max_prompt_gap_minutes = 40
reset_hour = 2
nudge_thresholds_minutes = [30, 60, 90, 120, 150, 180, 210, 240]
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
- **Stop** (Claude finishes responding): compute gap = now − `last_event_time` (generation time). Always counted in full — generation time is unambiguously active coding. Add to `accumulated_minutes`. Update `last_event_time` to now.
- **Prompt**: compute gap = now − `last_event_time` (reading/thinking time since Claude stopped). If gap exceeds `max_prompt_gap_minutes`, drop it (count 0 — user was away). Otherwise add to `accumulated_minutes`. Check thresholds; if a new one is crossed and not already in `nudges_given` and not `ignored`, output a nudge message to stdout and record the threshold.

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

<!-- BEGIN BEADS INTEGRATION v:1 profile:full hash:f65d5d33 -->
## Issue Tracking with bd (beads)

**IMPORTANT**: This project uses **bd (beads)** for ALL issue tracking. Do NOT use markdown TODOs, task lists, or other tracking methods.

### Why bd?

- Dependency-aware: Track blockers and relationships between issues
- Git-friendly: Dolt-powered version control with native sync
- Agent-optimized: JSON output, ready work detection, discovered-from links
- Prevents duplicate tracking systems and confusion

### Quick Start

**Check for ready work:**

```bash
bd ready --json
```

**Create new issues:**

```bash
bd create "Issue title" --description="Detailed context" -t bug|feature|task -p 0-4 --json
bd create "Issue title" --description="What this issue is about" -p 1 --deps discovered-from:bd-123 --json
```

**Claim and update:**

```bash
bd update <id> --claim --json
bd update bd-42 --priority 1 --json
```

**Complete work:**

```bash
bd close bd-42 --reason "Completed" --json
```

### Issue Types

- `bug` - Something broken
- `feature` - New functionality
- `task` - Work item (tests, docs, refactoring)
- `epic` - Large feature with subtasks
- `chore` - Maintenance (dependencies, tooling)

### Priorities

- `0` - Critical (security, data loss, broken builds)
- `1` - High (major features, important bugs)
- `2` - Medium (default, nice-to-have)
- `3` - Low (polish, optimization)
- `4` - Backlog (future ideas)

### Workflow for AI Agents

1. **Check ready work**: `bd ready` shows unblocked issues
2. **Claim your task atomically**: `bd update <id> --claim`
3. **Work on it**: Implement, test, document
4. **Discover new work?** Create linked issue:
   - `bd create "Found bug" --description="Details about what was found" -p 1 --deps discovered-from:<parent-id>`
5. **Complete**: `bd close <id> --reason "Done"`

### Quality
- Use `--acceptance` and `--design` fields when creating issues
- Use `--validate` to check description completeness

### Lifecycle
- `bd defer <id>` / `bd supersede <id>` for issue management
- `bd stale` / `bd orphans` / `bd lint` for hygiene
- `bd human <id>` to flag for human decisions
- `bd formula list` / `bd mol pour <name>` for structured workflows

### Auto-Sync

bd automatically syncs via Dolt:

- Each write auto-commits to Dolt history
- Use `bd dolt push`/`bd dolt pull` for remote sync
- No manual export/import needed!

### Important Rules

- ✅ Use bd for ALL task tracking
- ✅ Always use `--json` flag for programmatic use
- ✅ Link discovered work with `discovered-from` dependencies
- ✅ Check `bd ready` before asking "what should I work on?"
- ❌ Do NOT create markdown TODO lists
- ❌ Do NOT use external issue trackers
- ❌ Do NOT duplicate tracking systems

For more details, see README.md and docs/QUICKSTART.md.

## Session Completion

**When ending a work session**, you MUST complete ALL steps below. Work is NOT complete until `git push` succeeds.

**MANDATORY WORKFLOW:**

1. **File issues for remaining work** - Create issues for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **PUSH TO REMOTE** - This is MANDATORY:
   ```bash
   git pull --rebase
   bd dolt push
   git push
   git status  # MUST show "up to date with origin"
   ```
5. **Clean up** - Clear stashes, prune remote branches
6. **Verify** - All changes committed AND pushed
7. **Hand off** - Provide context for next session

**CRITICAL RULES:**
- Work is NOT complete until `git push` succeeds
- NEVER stop before pushing - that leaves work stranded locally
- NEVER say "ready to push when you are" - YOU must push
- If push fails, resolve and retry until it succeeds

<!-- END BEADS INTEGRATION -->
