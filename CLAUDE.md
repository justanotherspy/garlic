# CLAUDE.md

See @README.md for what garlic does, its config, and the project layout.

## Package management
Use `uv` — never `pip`/`pip3`/`pipx`. `uv sync` to install, `uv run <cmd>` to run, `uv add [--dev] <pkg>` to add deps.
Read @Makefile for make targets

## Philosophy
**Standard library first.** Third-party runtime deps have a high bar — discuss before adding.

## Testing
- Run: `uv run pytest`. Keep tests fast; no network calls.
- **All tests must use the `garlic_env` fixture** from `tests/conftest.py`. It redirects `GARLIC_DIR`/`CONFIG_PATH`/`STATE_PATH` to a tmpdir so tests never touch `~/.garlic/`. Override file contents after requesting the fixture — never patch paths manually.
- **Never run garlic commands against real paths** in tests or ad-hoc scripts. (A round-trip test once corrupted `~/.garlic/state.toml`.)

## Commits
Use **Conventional Commits** — `git-cliff` parses them to build the changelog. Format: `<type>(<scope>): <description>`. Types: `feat`, `fix`, `docs`, `refactor`, `test`, `chore`, `ci`, `perf`, `style`. Include the Linear ID when there is one: `feat(setup): add prompts (JUS-42)`.

## Workflow

**Always start from latest main.** Before any code work:
```bash
git checkout main && git pull
git checkout -b <issue-id>/<short-description>
```

**Issue tracking: Linear only.** This project uses [Linear](https://linear.app/justanotherspy) as its issue tracker — **never GitHub Issues**. When searching for MCP tools to look up or update issues, always use the Linear MCP tools, not the GitHub `mcp__github__issue_*` tools.

**If a Linear ticket is supplied:**
1. Read the ticket via the Linear MCP tools.
2. If it isn't already "In Progress", move it to "In Progress" *before* writing code or opening a PR.
3. Use the ID in the branch, commits, PR title (`JUS-XX: <description>`), and PR body (link to `https://linear.app/justanotherspy/issue/JUS-XX`).

Not every PR has a ticket (dependabot, small ad-hoc fixes) — that's fine. If one is supplied, it must be used.

**PR:**
- Always open as a draft PR
- Title starts with the Linear ID when there is one.
- Body has **Goal** and **Solution** sections. No **Test Plan** section
- One issue per PR.
- Update `README.md` in feature PRs so docs stay current.

**Never push directly to main.**

## CI
Pin all actions to full commit SHAs.

**When a CI check fails**, always fetch the logs before drawing conclusions:
```bash
# List failed runs for the PR
gh run list --branch <branch> --status failure --repo justanotherspy/garlic

# View the summary and failed steps of a specific run
gh run view <run-id> --repo justanotherspy/garlic

# Stream the full failed-step logs
gh run view <run-id> --log-failed --repo justanotherspy/garlic
```
Always pass `--repo justanotherspy/garlic` — the local git remote is not a GitHub host so gh cannot infer it. Read the actual error output — don't guess the cause from the check name alone.

## Releasing
1. `make release BUMP=patch|minor|major` opens a version-bump + changelog PR.
2. Merge it — release-drafter updates a draft GitHub release.
3. Publish the draft → Actions ships to PyPI via OIDC.
