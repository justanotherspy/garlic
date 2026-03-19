# Changelog

All notable changes to this project will be documented in this file.

Format based on [Keep a Changelog](https://keepachangelog.com/).

## [Unreleased]

## [0.1.6] - 2026-03-19

### Added
- `garlic setup` is now interactive by default — prompts for nudge interval, max prompt gap, reset hour, and nudge style with sensible defaults (#10)
- `-y`/`--yes` flag on `garlic setup` to skip prompts and use all defaults
- `garlic version` now checks PyPI for newer releases and suggests upgrading (#11)
- Pre-reset bedtime nudge warns you when it's getting late (#8)
- Definitive final nudge at the last threshold (#5)
- Nudge output prefixed with instruction to relay verbatim (#7)

### Changed
- Nudge every 30 minutes for 4 hours (was variable) (#5)
- Increase default `max_prompt_gap_minutes` from 20 to 40 (#4)
- Gaps exceeding the cap are dropped entirely (#9)

### Fixed
- `garlic setup` writes `settings.json` atomically to prevent corruption

### CI
- Bump `actions/checkout` to v6, `astral-sh/setup-uv` to v7 (#2, #3)
- Add GHA CI workflow and PR template (#1)
- Update README defaults; remind to update README before PRs (#6)

## [0.1.5] - 2026-03-16

### Added
- `garlic set` command to update config values from the CLI
- `garlic reset` command to reset the daily timer (with confirmation)
- Redesigned `garlic status` with progress bars, threshold countdowns, and garlic/vampire emoji
- `garlic ignore` now toggles — run again to resume nudging
- Prominent ignore state display in status output
- Expanded nudge message pools from 5 to 15 messages per style
- CHANGELOG and RELEASING docs

## [0.1.4] - 2026-03-16

### Added
- Show garlic status on session start (SessionStart hook)
- `/garlic` slash command for Claude Code
- Unix-only compatibility note in README

### Changed
- Increase default `max_prompt_gap_minutes` from 10 to 20

## [0.1.3] - 2026-03-16

### Added
- `--debug` flag for hook and setup commands
- Tests for engine logic and state management
- Stop hook now accumulates generation time (full engagement cycle tracking)

### Changed
- Updated README with time model docs, upgrade instructions, version command

## [0.1.2] - 2026-03-16

### Added
- `garlic version` command
- Makefile with build, clean, test, bump-patch, and publish targets

### Fixed
- Hook format: `garlic setup` now writes the correct `matcher` + `hooks` envelope format
- Migrate legacy flat-format hook entries on re-run

## [0.1.0] - 2026-03-16

### Added
- Initial release
- Time tracking via Claude Code hooks (session-start, prompt, stop)
- Configurable nudge thresholds with three personalities: gentle, firm, spicy
- `garlic status` to see daily coding time
- `garlic ignore` to suppress nudges for the day
- Config module (`~/.garlic/config.toml`) with defaults
- State module (`~/.garlic/state.toml`) with file locking
- Zero third-party runtime dependencies
- PyPI packaging with `uv build`
