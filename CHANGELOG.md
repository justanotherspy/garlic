# Changelog

All notable changes to this project will be documented in this file.

Format based on [Keep a Changelog](https://keepachangelog.com/).

## [Unreleased]

### Added
- `garlic set` command to update config values from the CLI
- `garlic reset` command to reset the daily timer (with confirmation)
- Redesigned `garlic status` with progress bars, threshold countdowns, and garlic/vampire emoji
- `garlic ignore` now toggles — run again to resume nudging
- Prominent ignore state display in status output
- Expanded nudge message pools from 5 to 15 messages per style
- CHANGELOG and RELEASING docs

## [0.1.3] - 2026-03-16

### Added
- Show garlic status on session start (SessionStart hook)
- `/garlic` slash command for Claude Code
- Unix-only compatibility note in README

### Changed
- Increase default `max_prompt_gap_minutes` from 10 to 20

## [0.1.2] - 2026-03-16

### Added
- `--debug` flag for hook and setup commands
- Tests for engine logic and state management
- Stop hook now accumulates generation time (full engagement cycle tracking)

### Changed
- Updated README with time model docs, upgrade instructions, version command

## [0.1.1] - 2026-03-16

### Added
- `garlic version` command
- Makefile with build, clean, test, bump-patch, and publish targets
- PyPI publish workflow

## [0.1.0] - 2026-03-16

### Added
- Initial release
- Config module (`~/.garlic/config.toml`) with defaults
- State module (`~/.garlic/state.toml`) with file locking
- Time calculation engine with gap accumulation and threshold checking
- Nudge message pools (gentle, firm, spicy)
- CLI commands: `garlic setup`, `garlic status`, `garlic ignore`
- Hook handlers: session-start, prompt, stop
- PyPI packaging with `uv build`
