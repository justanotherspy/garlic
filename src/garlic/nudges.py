"""Hardcoded nudge message pools (gentle/firm/spicy) with random selection."""

import random

GENTLE = [
    "You've been coding for {time}. A short break might feel nice.",
    "Heads up — {time} of active coding today. Stretch your legs?",
    "{time} in the flow. Remember to blink and hydrate.",
    "Friendly nudge: {time} of coding. Your future self will thank you for a break.",
    "You've been at it for {time}. A quick breather can sharpen your focus.",
    "{time} today. Have you looked at something more than 20 feet away recently?",
    "Gentle check-in: {time} of coding. A glass of water might hit right about now.",
    "{time} and counting. Fresh air does wonders for a stuck problem.",
    "You've been focused for {time}. Your neck and shoulders could probably use a stretch.",
    "Hey, {time} of solid work. Step outside for a minute — the code will wait.",
    "{time} in. A five-minute walk can untangle more than an hour of staring.",
    "Just noting: {time} of active coding. Your eyes would love a break from the screen.",
    "{time} today. Sometimes the best debugging happens away from the keyboard.",
    "{time} of coding — not bad. Refill that water bottle?",
    "You've been going for {time}. Even a one-minute stretch makes a difference.",
]

FIRM = [
    "You've been coding for {time}. Time to step away for a bit.",
    "{time} of coding today. Take a real break — not just switching tabs.",
    "That's {time} of active work. Stand up, walk around, come back fresh.",
    "{time} logged. Your brain needs downtime to consolidate what you've learned.",
    "Seriously, {time}. Close the laptop lid for ten minutes.",
    "{time} of work. Diminishing returns are real — take a break now, ship better code later.",
    "You've hit {time}. Your focus isn't as sharp as it was an hour ago. Break time.",
    "{time} in. Go eat something. Real food, not just snacks at your desk.",
    "That's {time}. Step away. The diff will make more sense with fresh eyes.",
    "{time} today. Breaks aren't optional — they're part of doing good work.",
    "{time} of coding. Your back is not going to forgive you if you don't move.",
    "You're at {time}. Take a proper break — walk around the block, not just to the fridge.",
    "{time} logged. The problems you're solving deserve a rested brain.",
    "{time}. You've earned a break. Take it before you start making tired mistakes.",
    "That's {time} of active coding. Close the editor. Stand up. Ten minutes minimum.",
]

SPICY = [
    "{time} of coding. Touch grass. I'm not asking.",
    "You absolute gremlin — {time} straight. Go see the sun.",
    "{time}?! Your chair has a YOU-shaped dent. Get up.",
    "The code will still be broken after a break. {time} is enough.",
    "{time} in. Even the AI thinks you need to chill.",
    "{time}. Your screen tan is coming in nicely. Go outside.",
    "You've been pair-programming with an AI for {time}. That's not a personality.",
    "{time} of coding. If I had legs I'd walk away. You have legs. Use them.",
    "{time}?! I'm a language model and even I think this is excessive.",
    "Congrats on {time}. Your vertebrae are filing a class-action lawsuit.",
    "{time} logged. You know what's a great IDE? A park bench.",
    "Look, {time}. At this point I'm enabling you. Go away.",
    "{time}. The bugs will still be there when you get back. They always are.",
    "You've been at this for {time}. Your future chiropractor thanks you for the job security.",
    "{time} of letting an AI do your thinking. Go think your own thoughts for a bit.",
]

FINAL = [
    "That's {time} today. Time's up — close the laptop and step away.",
    "You've hit {time}. That's your lot for today. Shut it down.",
    "{time}. Session over. The code will still be there tomorrow.",
    "You've done {time} today. Seriously, that's enough. Close it.",
    "{time} of coding. Your brain is full. Log off.",
    "That's {time}. You're done for today — no negotiating with yourself.",
    "{time} logged. Laptop lid down. You've earned it.",
    "Hard stop: {time}. Walk away from the keyboard. You're done.",
    "{time} today. This is not a suggestion — wrap it up.",
    "Final nudge: {time}. The work will keep. You need to stop.",
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


def get_nudge(style: str, accumulated_minutes: float, is_final: bool = False) -> str:
    """Pick a random nudge message from the given style pool.

    If is_final is True, always uses the FINAL pool regardless of style.
    """
    pool = FINAL if is_final else POOLS.get(style, GENTLE)
    time_str = _format_time(accumulated_minutes)
    message = random.choice(pool)
    return message.format(time=time_str)
