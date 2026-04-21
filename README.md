# Garlic 🧄 — the AI Vampire 🧛 Warding Tool

Garlic is used to ward off vampires. According to Steve Yegge, AI tools have a vampiric effect on us, draining us of energy and making us tired and exhausted. Not because they are not good at coding, or do not make us much more productive, but simply because we get dopamine for getting stuff done quicker, leading us to work longer and think harder. In short, we need to touch grass. Instead of going hard for 12 hours straight with our coding agent of choice and burning ourselves out to only create value for our employer, we should be mindful of the $/hr formula and consider a new balance. He estimates there are no more than 3-4 hours of good work that we can do in a day with all this uplift without burning our own candles a little too brightly. As someone quite sensitive to the effects of extended dopamine release on the mind and body, I tend to agree with him. So I created `garlic`, a CLI tool that helps you keep the draining to a minimum and maintain your own energy levels so we can continue to be healthy little worker bees for years to come.

The idea came from [this article by Steve Yegge](https://steve-yegge.medium.com/the-ai-vampire-eda6e4f07163).

## How does it work?

`garlic` hooks into Claude Code using its [hooks system](https://docs.anthropic.com/en/docs/claude-code/hooks). It tracks three events:

- **Session start** — when you open a new Claude Code session
- **Prompt submit** — when you send a message to Claude
- **Stop** — when Claude finishes responding

From these events, garlic estimates how much time you have spent actively coding each day. It works across multiple concurrent Claude Code sessions by sharing a single state file with file locking.

The time model counts your full engagement cycle: the time Claude spends generating a response (up to a configurable cap — 2 hours by default — to guard against hung processes or forgotten sessions inflating your daily total), plus the time you spend reading it and thinking before your next prompt. If your thinking time exceeds 40 minutes (configurable), garlic assumes you stepped away and counts nothing for that gap. Gaps within the limit are counted in full. The limit is intentionally generous: it covers the time you spend reading docs, answering a Slack message, checking email, or getting back into context — adjacent work that's still part of your coding session.

As you approach configurable thresholds (every 30 minutes up to 4 hours by default), garlic asks Claude to gently nudge you to consider taking a break. You choose how it nudges — `gentle`, `firm`, or `spicy`. Each threshold only fires once, so you won't be nagged on every prompt. The final threshold delivers a more definitive "session over" message.

If you're still coding in the hour before the daily reset (1 AM by default, when `reset_hour` is 2), garlic sends a bedtime nudge — a distinct "wrap up and get some sleep" message that fires once per night.

## Compatibility

- Python 3.11+
- macOS, Linux, and WSL
- Not supported on native Windows (`fcntl` is unavailable)

## Setup

Install garlic with [uv](https://docs.astral.sh/uv/):

```bash
uv tool install garlic-cli
```

Run setup to install the Claude Code hooks:

```bash
garlic setup
```

Setup interactively prompts for key preferences (nudge interval, max prompt gap, reset hour, nudge style) with sensible defaults — just press Enter to accept them all. To skip prompts entirely and use defaults, pass `-y`:

```bash
garlic setup -y
```

This does three things:
1. Creates `~/.garlic/config.toml` (with your chosen settings or sensible defaults)
2. Adds garlic's hooks to `~/.claude/settings.json` so they run across all your projects
3. Appends a nudge-relay instruction to `~/.claude/CLAUDE.md` so Claude always relays garlic's nudge messages verbatim as the last line of its response

To reset your config to the latest built-in defaults (useful after upgrading):

```bash
garlic setup --defaults
```

Setup is idempotent — safe to run again if you need to repair or update hooks.

## Upgrading

```bash
uv tool install garlic-cli --upgrade
```

Then re-run `garlic setup` to update your hooks if the release notes mention hook changes.

## Usage

```bash
# Check your installed version
garlic version

# See how long you have been Clauding today
garlic status

# See your rolling 7-day usage summary
garlic week

# See monthly totals, streaks, and averages
garlic stats

# Disable nudging for the rest of the day (tracking continues)
garlic ignore

# Update a config value without editing the file
garlic set nudge_style=spicy
garlic set max_prompt_gap_minutes=60

# Reset the daily timer to zero
garlic reset
```

### Slash command

After running `garlic setup`, you can use `/garlic` directly in Claude Code to check your status without leaving the conversation.

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

## Things I should know?

**No prompt injection risk.** The nudge messages output by garlic's hooks are hardcoded in the project. There is no mechanism for external input to influence what gets sent to your agent. You can audit every possible message in [`src/garlic/nudges.py`](src/garlic/nudges.py).

**No runtime dependencies.** Garlic uses only the Python standard library at runtime. Development dependencies (pytest, build, twine) exist for testing and releasing, but are not shipped with the package. This is an intentional choice — garlic runs on every prompt you send, so the supply chain should be as small and auditable as possible.

**No data leaves your machine.** All state lives in `~/.garlic/` and is never transmitted anywhere.

**Built with Claude.** This project was built with Claude Code, which is fitting given what it does.
