# CLAUDE.md

See @README.md for what garlic does, its config, and the project layout.

## Package management
This is a Rust crate built with `cargo`. `cargo build` to compile, `cargo run -- <args>` to run, `cargo add [--dev] <crate>` to add deps, `cargo test` to test. Commit `Cargo.lock`.
Read @Makefile for make targets.

## Philosophy
**Standard library first.** New runtime dependencies have a high bar — discuss before adding. Rust's std lacks a TOML parser, arg parser, and HTTP client, so a small, audited set of crates is unavoidable; keep that set minimal. `cargo audit` runs in CI.

## Testing
- Run: `cargo test`. Keep tests fast; no external network calls (the version-check tests use a local mock server).
- Before pushing, also run `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` — CI enforces both.
- **Tests must never touch the real `~/.garlic/` or `~/.claude/`.** Unit tests build a `Paths`/`ClaudePaths` rooted at a `tempfile::TempDir`; integration tests set the `GARLIC_DIR` (and `CLAUDE_HOME`) env var to a temp directory. (A round-trip test once corrupted `~/.garlic/state.toml`.)
- Engine and date logic take an injected clock (`now`) so they're deterministic — pass a fixed value rather than reading the wall clock.

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
- **Code review workflow**: After opening as draft, the maintainer may mark the PR ready for review to trigger the Claude Code Review workflow. This is expected — PRs being non-draft at review time is intentional workflow, not a mistake.

**Never push directly to main.**

## CI
Pin all actions to full commit SHAs.

**When a CI check fails**, always fetch the logs before drawing conclusions:
```bash
# List failed runs for the PR
gh run list --branch <branch> --status failure --repo justanotherspy/garlic

# View the summary and failed steps of a specific run
gh run view <run-id> --repo justanotherspy/garlic
```
Read the actual error output — don't guess the cause from the check name alone.

## Releasing
1. `make release BUMP=patch|minor|major` bumps `Cargo.toml` and opens a version-bump PR (needs `cargo install cargo-edit`).
2. Merge it — release-drafter updates a draft GitHub release.
3. Publish the draft → Actions publishes `garlic-cli` to crates.io (Trusted Publishing / OIDC) and uploads prebuilt Linux/macOS binaries to the release.
