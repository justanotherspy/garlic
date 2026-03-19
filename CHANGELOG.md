# Changelog

All notable changes to this project will be documented in this file.

Format based on [Keep a Changelog](https://keepachangelog.com/).


## [0.1.7] - 2026-03-19

### Added
- Adopt conventional commits and git-cliff for automated changelogs (garlic-djp) (#14)

### Other
- Add --defaults flag to `garlic setup` (garlic-wn5) (#12)

## [0.1.6] - 2026-03-19

### Other
- Add GHA CI workflow and PR template (#1)
- Bump astral-sh/setup-uv from 5 to 7 (#2)
- Bump actions/checkout from 4 to 6 (#3)
- Increase default max_prompt_gap_minutes from 20 to 40 (#4)
- Nudge every 30min for 4h; add definitive final nudge (#5)
- Update README defaults; remind to update README before PRs (#6)
- Fix setup command to write settings.json atomically
- Prefix nudge output with instruction to relay verbatim (#7)
- Add pre-reset bedtime nudge (garlic-rj0) (#8)
- Drop gaps exceeding cap; document bedtime and final nudges (#9)
- Make `garlic setup` interactive by default (garlic-v5k) (#10)
- Add update check to `garlic version` (garlic-cjf) (#11)

## [0.1.5] - 2026-03-16

### Other
- Redesign garlic status with visual flair and toggle ignore
- Add set/reset commands, expand nudges, add release docs

## [0.1.4] - 2026-03-16

### Other
- Show garlic status on session start
- Update CLAUDE.md beads integration section
- Increase default max_prompt_gap_minutes from 10 to 20
- Add Unix-only compatibility note to README
- Add /garlic slash command for Claude Code

## [0.1.3] - 2026-03-16

### Other
- Add --debug flag to hook and setup commands
- Add logic correctness tests for engine and state
- Count Claude generation time: accumulate in stop hook
- Update README: time model, upgrade instructions, version command

## [0.1.2] - 2026-03-16

### Other
- Fix hook format: wrap commands in matcher+hooks envelope
- Add Makefile with build, clean, and test targets
- Add publish target to Makefile
- Add garlic version command
- Add tests for garlic version command

## [0.1.0] - 2026-03-16

### Other
- Initial commit: README and CLAUDE.md
- Update README: fix uv install wording, grammar, embedded article link, emoji
- Bd init: initialize beads issue tracking
- Project scaffolding: pyproject.toml, CLI entry point, src layout
- Add .gitignore and remove committed __pycache__ files
- Add config module: load/create ~/.garlic/config.toml with defaults
- Add state module: ~/.garlic/state.toml read/write with file locking
- Add time calculation engine: gap accumulation and threshold checking
- Add nudge messages: gentle/firm/spicy pools with time formatting
- Add CLI commands: hooks, setup, status, and ignore
- Add PyPI metadata, MIT license, and update install instructions
- Add build and twine dev dependencies
