---
name: garlic-time
description: Answer questions about how much time the user has spent in Claude Code (today, this week, this month, streaks) and how it splits between their own time and agent time, using the garlic CLI. Use when the user asks how long they have been working, coding or "clauding", whether they should take a break, or for a usage summary.
allowed-tools: Bash(garlic status:*)
---

garlic tracks active Claude Code time on this machine. Read it with the CLI; never estimate from the conversation.

- `garlic status --json` gives today, in minutes. Every figure is deduplicated across parallel sessions:
  - `accumulated_minutes`: total engaged wall-clock time.
  - `user_minutes`: time the user was reading, thinking or typing.
  - `agent_minutes`: time at least one agent was generating.
  - `user_only_minutes`, `agent_only_minutes`, `both_minutes`: the same total split three ways; they add up to it.
  - `overlap_minutes`: time two or more sessions were active at once.
  - `agents_running`: agents generating right now.
  - `next_threshold`, `nudges_given`, `ignored`: where the break nudges stand.
- `garlic status --week` prints the rolling 7-day summary.
- `garlic status --month` prints monthly totals, streaks and averages.

Report the figures plainly. Parallel agents are not extra productivity: never present overlap as a multiplier. Do not change anything: pausing nudges and resetting the day are the user's to do, from the `/garlic` panel.
