"""Hardcoded nudge message pools (gentle/firm/spicy) with random selection."""

import random

GENTLE = [
    "You've been coding for {time}. A short break might feel nice.",
    "Heads up — {time} of active coding today. Stretch your legs?",
    "{time} in the flow. Remember to blink and hydrate.",
    "Friendly nudge: {time} of coding. Your future self will thank you for a break.",
    "You've been at it for {time}. A quick breather can sharpen your focus.",
]

FIRM = [
    "You've been coding for {time}. Time to step away for a bit.",
    "{time} of coding today. Take a real break — not just switching tabs.",
    "That's {time} of active work. Stand up, walk around, come back fresh.",
    "{time} logged. Your brain needs downtime to consolidate what you've learned.",
    "Seriously, {time}. Close the laptop lid for ten minutes.",
]

SPICY = [
    "{time} of coding. Touch grass. I'm not asking.",
    "You absolute gremlin — {time} straight. Go see the sun.",
    "{time}?! Your chair has a YOU-shaped dent. Get up.",
    "The code will still be broken after a break. {time} is enough.",
    "{time} in. Even the AI thinks you need to chill.",
]

POOLS = {
    "gentle": GENTLE,
    "firm": FIRM,
    "spicy": SPICY,
}


def _format_time(minutes: float) -> str:
    """Format accumulated minutes as a human-readable string."""
    if minutes < 60:
        rounded = round(minutes)
        return f"~{rounded} minutes" if rounded != 1 else "~1 minute"
    hours = minutes / 60
    if hours < 1.5:
        return "~1 hour"
    return f"~{round(hours)} hours"


def get_nudge(style: str, accumulated_minutes: float) -> str:
    """Pick a random nudge message from the given style pool."""
    pool = POOLS.get(style, GENTLE)
    time_str = _format_time(accumulated_minutes)
    message = random.choice(pool)
    return message.format(time=time_str)
