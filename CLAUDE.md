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
