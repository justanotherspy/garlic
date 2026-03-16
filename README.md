# Garlic 🧄 — the AI Vampire 🧛 Warding Tool

Garlic is used to ward off vampires. According to Steve Yegge, AI tools have a vampiric effect on us, draining us of energy and making us tired and exhausted. Not because they are not good at coding, or do not make us much more productive, but simply because we get dopamine for getting stuff done quicker, leading us to work longer and think harder. In short, we need to touch grass. Instead of going hard for 12 hours straight with our coding agent of choice and burning ourselves out to only create value for our employer, we should be mindful of the $/hr formula and consider a new balance. He estimates there are no more than 3-4 hours of good work that we can do in a day with all this uplift without burning our own candles a little too brightly. As someone quite sensitive to the effects of extended dopamine release on the mind and body, I tend to agree with him. So I created `garlic`, a CLI tool that helps you keep the draining to a minimum and maintain your own energy levels so we can continue to be healthy little worker bees for years to come.

The idea came from [this article by Steve Yegge](https://steve-yegge.medium.com/the-ai-vampire-eda6e4f07163).

## How does it work?

`garlic` hooks into Claude Code using its [hooks system](https://docs.anthropic.com/en/docs/claude-code/hooks). It tracks three events:

- **Session start** — when you open a new Claude Code session
- **Prompt submit** — when you send a message to Claude
- **Stop** — when Claude finishes responding

From these events, garlic estimates how much time you have spent actively coding each day. It works across multiple concurrent Claude Code sessions by sharing a single state file with file locking.

The time model counts your full engagement cycle: the time Claude spends generating a response, plus the time you spend reading it and thinking before your next prompt. Each gap is capped at 20 minutes by default — if you step away for an hour, garlic assumes you spent about 20 minutes getting back up to speed rather than counting the full absence. This keeps the estimate honest without needing to spy on your screen.

As you approach configurable thresholds (1 hour, 2 hours, etc.), garlic asks Claude to gently nudge you to consider taking a break. You choose how it nudges — `gentle`, `firm`, or `spicy`. Each threshold only fires once, so you won't be nagged on every prompt.

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

This does two things:
1. Creates `~/.garlic/config.toml` with sensible defaults
2. Adds garlic's hooks to `~/.claude/settings.json` so they run across all your projects

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

# Disable nudging for the rest of the day (tracking continues)
garlic ignore
```

## Configuration

Edit `~/.garlic/config.toml` to customize:

```toml
# Max time (minutes) to attribute to a single gap between events.
# If you step away for an hour, garlic assumes you spent this many
# minutes getting back up to speed rather than counting the full gap.
max_prompt_gap_minutes = 20

# Hour of day (0-23) when the daily timer resets.
reset_hour = 2

# Accumulated minutes at which garlic will nudge you.
# Each threshold fires only once per day.
nudge_thresholds_minutes = [60, 120, 180, 240]

# Nudge personality: "gentle", "firm", or "spicy".
nudge_style = "gentle"
```

## Things I should know?

**No prompt injection risk.** The nudge messages output by garlic's hooks are hardcoded in the project. There is no mechanism for external input to influence what gets sent to your agent. You can audit every possible message in [`src/garlic/nudges.py`](src/garlic/nudges.py).

**No third-party dependencies.** Garlic uses only the Python standard library. This is an intentional choice — it runs on every prompt you send, so the supply chain should be as small and auditable as possible.

**No data leaves your machine.** All state lives in `~/.garlic/` and is never transmitted anywhere.

**Built with Claude.** This project was built with Claude Code, which is fitting given what it does.
