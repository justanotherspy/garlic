# Garlic 🧄 — the AI Vampire 🧛 Warding Tool

[![CI](https://github.com/justanotherspy/garlic/workflows/CI/badge.svg)](https://github.com/justanotherspy/garlic/actions/workflows/ci.yml)
[![Crates.io version](https://img.shields.io/crates/v/garlic-ward.svg)](https://crates.io/crates/garlic-ward)
[![MSRV](https://img.shields.io/badge/MSRV-1.89-blue.svg)](https://blog.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Crates.io downloads](https://img.shields.io/crates/d/garlic-ward.svg)](https://crates.io/crates/garlic-ward)

Garlic is used to ward off vampires. According to Steve Yegge, AI tools have a vampiric effect on us, draining us of energy and making us tired and exhausted. Not because they are not good at coding, or do not make us much more productive, but simply because we get dopamine for getting stuff done quicker, leading us to work longer and think harder. In short, we need to touch grass. Instead of going hard for 12 hours straight with our coding agent of choice and burning ourselves out to only create value for our employer, we should be mindful of the $/hr formula and consider a new balance. He estimates there are no more than 3-4 hours of good work that we can do in a day with all this uplift without burning our own candles a little too brightly. As someone quite sensitive to the effects of extended dopamine release on the mind and body, I tend to agree with him. So I created `garlic`, a CLI tool that helps you keep the draining to a minimum and maintain your own energy levels so we can continue to be healthy little worker bees for years to come.

The idea came from [this article by Steve Yegge](https://steve-yegge.medium.com/the-ai-vampire-eda6e4f07163).

## How does it work?

`garlic` hooks into Claude Code using its [hooks system](https://docs.anthropic.com/en/docs/claude-code/hooks). It tracks three events:

- **Session start** — when you open a new Claude Code session
- **Prompt submit** — when you send a message to Claude
- **Stop** — when Claude finishes responding

From these events, garlic estimates how much time you have spent actively coding each day. It works across multiple concurrent Claude Code sessions by sharing a single state file with file locking.

The time model counts your full engagement cycle: the time Claude spends generating a response (up to a configurable cap — 2 hours by default — to guard against hung processes or forgotten sessions inflating your daily total), plus the time you spend reading it and thinking before your next prompt. If your thinking time exceeds 40 minutes (configurable), garlic assumes you stepped away and counts nothing for that gap. Gaps within the limit are counted in full. The limit is intentionally generous: it covers the time you spend reading docs, answering a Slack message, checking email, or getting back into context — adjacent work that's still part of your coding session.

Internally each cycle is tracked as **intervals** tagged by session: a UserPrompt→Stop span is **agent time** (the agent generating) and a Stop→UserPrompt span is **user time** (you reading, thinking, or typing). `garlic status` shows the split so you can see how an hour divides between agent work and your own planning/review. Your daily total is the **union** of these intervals across all sessions — so running two agents in parallel for an hour counts as one hour of engagement, not two. When sessions do overlap, garlic reports the concurrent time as a neutral fact (e.g. `12m with 2+ sessions running at once`); it is *never* weighted as "more productive," because babysitting multiple agents at once is more draining, not less — exactly the kind of overwork garlic is here to ward off.

As you approach configurable thresholds (every 30 minutes up to 4 hours by default), garlic asks Claude to gently nudge you to consider taking a break. You choose how it nudges — `gentle`, `firm`, or `spicy`. Each threshold only fires once, so you won't be nagged on every prompt. The final threshold delivers a more definitive "session over" message.

If you're still coding in the hour before the daily reset (1 AM by default, when `reset_hour` is 2), garlic sends a bedtime nudge — a distinct "wrap up and get some sleep" message that fires once per night.

## Compatibility

- macOS, Linux, and WSL
- Native Windows is not officially tested

`garlic` is a single self-contained binary — no runtime (Python, Node, etc.) is required to run it.

## Setup

Install garlic from [crates.io](https://crates.io/crates/garlic-ward) with [cargo](https://doc.rust-lang.org/cargo/):

```bash
cargo install garlic-ward
```

Or, without a Rust toolchain, grab a prebuilt binary with [cargo-binstall](https://github.com/cargo-bins/cargo-binstall):

```bash
cargo binstall garlic-ward
```

On macOS you can install the prebuilt binary with [Homebrew](https://brew.sh):

```bash
brew install --cask justanotherspy/tap/garlic
```

Or tap once, then install by short name:

```bash
brew tap justanotherspy/tap
brew install --cask garlic
```

Upgrade later with `brew upgrade --cask garlic`. The cask is republished to
[`justanotherspy/homebrew-tap`](https://github.com/justanotherspy/homebrew-tap)
automatically on every release. (The cask is macOS-only; on Linux/WSL use
`cargo install garlic-ward`, `cargo binstall garlic-ward`, or a prebuilt binary.)

You can also download a prebuilt binary for your platform from the [latest release](https://github.com/justanotherspy/garlic/releases) and put `garlic` on your `PATH`.

Run setup to install the Claude Code hooks:

```bash
garlic setup
```

Setup interactively prompts for key preferences (nudge interval, max prompt gap, reset hour, nudge style) with sensible defaults — just press Enter to accept them all. To skip prompts entirely and use defaults, pass `-y`:

```bash
garlic setup -y
```

This does two things:
1. Creates `~/.garlic/config.toml` (with your chosen settings or sensible defaults)
2. Adds garlic's hooks to `~/.claude/settings.json` so they run across all your projects

To reset your config to the latest built-in defaults (useful after upgrading):

```bash
garlic setup --defaults
```

Setup is idempotent — safe to run again if you need to repair or update hooks.

## Migrating from the Python version

Earlier releases of garlic shipped as a Python package (`garlic-cli`) installed with [uv](https://github.com/astral-sh/uv). The current release is a self-contained Rust binary (`garlic-ward`) with no Python runtime required. To migrate, first uninstall the old Python tool, then install the Rust version and re-run setup:

```bash
uv tool uninstall garlic-cli
cargo install garlic-ward   # or: cargo binstall garlic-ward
garlic setup
```

Your existing `~/.garlic/` config and tracking state are preserved, so you keep your settings and daily totals across the switch. Re-running `garlic setup` refreshes the hooks in `~/.claude/settings.json` to point at the new binary.

## Upgrading

```bash
cargo install garlic-ward --force   # or: cargo binstall garlic-ward
```

`garlic version` checks crates.io once a day and tells you when a newer release is available.

Then re-run `garlic setup` to update your hooks if the release notes mention hook changes.

## Usage

```bash
# Check your installed version
garlic version

# See how long you have been Clauding today (with the agent/user split)
garlic status

# Output status as JSON (for scripting and statusline integrations)
garlic status --json

# Output a compact single-line string for the Claude Code status bar
garlic statusline

# See your rolling 7-day usage summary
garlic status --week

# See monthly totals, streaks, and averages
garlic status --month

# Disable nudging for the rest of the day (tracking continues)
garlic ignore

# Update a config value without editing the file
garlic set nudge_style=spicy
garlic set max_prompt_gap_minutes=60

# Reset the daily timer to zero
garlic reset

# Push locally-tracked time to the shared backend (for cron/manual sync)
garlic sync
```

### Slash command

After running `garlic setup`, you can use `/garlic` directly in Claude Code without leaving the conversation. It supports all garlic subcommands:

- `/garlic` or `/garlic status` — show today's accumulated time (and the agent/user split)
- `/garlic status --week` — show rolling 7-day summary
- `/garlic status --month` — show monthly totals and streaks
- `/garlic ignore` — disable nudging for the rest of the day

### Status line

To wire garlic into Claude Code's built-in status bar, run this in Claude Code:

```
/statusline add the output of the `garlic statusline` command to our status line
```

This shows a single-line readout like `🧄 2h 15m / 4h` — your accumulated time and daily target — refreshed on each Claude Code event.

## Configuration

Edit `~/.garlic/config.toml` to customize:

```toml
# Max thinking time (minutes) between Claude stopping and your next
# prompt that still counts as active coding. If you take longer than
# this, garlic assumes you stepped away and counts nothing for that gap.
max_prompt_gap_minutes = 40

# Max generation time (minutes) to count per response. If Claude runs
# longer than this (hung process, forgotten session), the time is
# clamped to this cap instead of inflating the daily total.
max_generation_minutes = 120

# Hour of day (0-23) when the daily timer resets.
reset_hour = 2

# Accumulated minutes at which garlic will nudge you.
# Each threshold fires only once per day. The final threshold uses
# a more definitive "session over" message.
nudge_thresholds_minutes = [30, 60, 90, 120, 150, 180, 210, 240]

# Nudge personality: "gentle", "firm", or "spicy".
nudge_style = "gentle"
```

## Shared state backend (self-hosted)

By default garlic tracks time in a local `~/.garlic/state.toml` and never talks
to the network. If you run Claude Code across several machines or ephemeral
cloud environments and want **one shared daily total**, you can self-host the
garlic backend — a small Rust HTTP service backed by Redis — and point every
client at it:

```bash
export GARLIC_URL="https://garlic.example.com"
export GARLIC_TOKEN="<a token configured on your backend>"
```

Every client using the same `GARLIC_TOKEN` shares one set of totals. The
service is fully self-hostable (it just needs a Redis connection string) and
ships with a Dockerfile and `docker-compose.yml`. See
[`backend/README.md`](backend/README.md) for the deployment guide and the
complete REST API contract.

### Local-first, sync later

garlic is **local-first**, even with a backend configured: hooks always account
time into `~/.garlic/state.toml` and **never block on the network**. Tracking
keeps working when the host is offline or the backend is unreachable — nothing
is lost, it's just synced later. The day's closed intervals (absolute time
spans) are the unit of sync, and the backend **merges** each client's intervals
by union — so two machines are just more sessions to union into one total, never
double-counted.

Who pushes that local outbox to the backend is decoupled from the hooks:

- **`garlic status`** flushes on the way through, then shows the merged
  (cross-machine) total. If the backend is unreachable it falls back to your
  local total and notes it — it never errors out.
- **`garlic sync`** is an explicit drain. On a laptop, schedule it with cron (or
  launchd) so a **separate process — outside any network-restricted agent
  sandbox — periodically reconciles totals** without ever slowing a hook:

  ```bash
  # crontab -e — sync every 5 minutes
  */5 * * * * GARLIC_URL=https://garlic.example.com GARLIC_TOKEN=… garlic sync
  ```

- **`GARLIC_SYNC=blocking`** makes the hooks themselves flush synchronously. Use
  it on ephemeral/managed hosts (CI, cloud agents) where the container may be
  reclaimed before any `status` or cron runs, so there's no "later" left to sync
  in. In this mode the prompt hook also nudges from the merged total, so a break
  threshold fires once across machines.

`garlic statusline` always reads local state so the status bar stays instant;
run `garlic status` for the shared cross-machine total. `ignore` and `reset`
apply locally and, when a backend is configured, mirror to it too.

The backend owns *state*; your local `~/.garlic/config.toml` still drives time
accounting and the nudge wording, and is sent with each request.

> **Nudges:** in the default (non-blocking) mode, break nudges are evaluated
> against *this machine's* local total, so hooks stay instant and
> offline-tolerant; the shared total is what `garlic status` and the backend
> reconcile. On a single machine the two are identical — only set
> `GARLIC_SYNC=blocking` if you want nudge thresholds shared across machines in
> real time.

## Project layout

`garlic` is a Rust crate (`garlic-ward`) that builds a single binary named `garlic`. Source modules in `src/`:

- `cli.rs` — CLI parsing (clap) and subcommand dispatch
- `commands.rs` — implementations of `status` (with `--week`/`--month` summaries), `statusline`, `set`, `reset`, `ignore`, `sync`, `setup`, `version`
- `intervals.rs` — interval types (agent/user spans tagged by session) and the sweep-line that derives daily totals, the agent/user split, and concurrency
- `config.rs` — loads/creates `~/.garlic/config.toml` with defaults
- `state.rs` — reads/writes `~/.garlic/state.toml` under an advisory file lock, handles daily reset
- `engine.rs` — gap calculation, accumulation, threshold checking
- `nudges.rs` — hardcoded gentle/firm/spicy message pools
- `hooks.rs` — handlers for the `session-start`, `prompt`, `stop`, and `session-end` hook subcommands
- `setup.rs` — installs/updates hooks in `~/.claude/settings.json`
- `version.rs` — daily crates.io update check
- `remote.rs` — client for the optional shared-state backend (`$GARLIC_URL`/`$GARLIC_TOKEN`)
- `sync.rs` — local-first sync policy: pushes the day's intervals to the backend (`garlic sync`, or inline when `GARLIC_SYNC=blocking`)
- `paths.rs` — resolves `~/.garlic` (overridable via `$GARLIC_DIR`) and `~/.claude`

Runtime state lives in `~/.garlic/`: `config.toml` (settings) and `state.toml` (daily tracking, file-locked for concurrent sessions). Set `$GARLIC_DIR` to relocate it.

Claude Code hooks written to `~/.claude/settings.json` by `garlic setup`:

- **SessionStart** (matcher `"startup"`) → `garlic hook session-start`
- **UserPromptSubmit** → `garlic hook prompt`
- **Stop** → `garlic hook stop`
- **SessionEnd** → `garlic hook session-end` — finalizes in-flight generation time and clears `last_event_time` so a crashed or killed session can't leak time into the next one

Each hook reads JSON from stdin and either writes plain text to stdout (a nudge) or exits silently.

The optional self-hosted state backend lives in [`backend/`](backend/) — a Rust
(axum + Redis) HTTP service that ports garlic's state engine server-side so
totals can be shared across environments. It is independent of the CLI package
and documented in [`backend/README.md`](backend/README.md).

## Things I should know?

**No prompt injection risk.** The nudge messages output by garlic's hooks are hardcoded in the project. There is no mechanism for external input to influence what gets sent to your agent. You can audit every possible message in [`src/nudges.rs`](src/nudges.rs).

**Minimal, audited dependencies.** Rust's standard library has no TOML parser, argument parser, or HTTP client, so garlic relies on a small set of widely-used crates (clap, serde, toml, chrono, rand, ureq, tempfile, dirs). They are pinned via the committed `Cargo.lock`, and CI runs `cargo audit` against the RustSec advisory database on every push. garlic runs on every prompt you send, so the supply chain is kept small and auditable on purpose.

**No data leaves your machine by default.** All state lives in `~/.garlic/` and is never transmitted anywhere. There are two opt-in exceptions: the daily update check, which fetches the latest version number from crates.io when you run `garlic version`; and the shared-state backend, which (only when you set `$GARLIC_URL`/`$GARLIC_TOKEN`) sends your time-tracking events to the server you choose to self-host.

**Built with Claude.** This project was built with Claude Code, which is fitting given what it does.

## Release process

- Run `make release BUMP=patch|minor|major` (bumps `Cargo.toml`, opens a PR; needs `cargo install cargo-edit`)
- Merge the release PR
- Release drafter updates the draft GitHub release
- Publish the draft release **as a pre-release**
- The release workflow then:
  - publishes `garlic-ward` to crates.io (via Trusted Publishing / OIDC) and uploads prebuilt binaries for Linux and macOS to the release;
  - regenerates the Homebrew cask from the macOS binaries and pushes it to [`justanotherspy/homebrew-tap`](https://github.com/justanotherspy/homebrew-tap) (`Casks/garlic.rb`); and
  - promotes the release to **Latest** (clears the pre-release flag) once the binaries and cask are in place, so the "Latest" badge and the tap never point at a half-published release.

The cask push requires a `HOMEBREW_TAP_GITHUB_TOKEN` repository secret — a token
with `contents:write` on `justanotherspy/homebrew-tap`. The cask template lives
at [`.github/homebrew/garlic-cask.rb.tmpl`](.github/homebrew/garlic-cask.rb.tmpl).
