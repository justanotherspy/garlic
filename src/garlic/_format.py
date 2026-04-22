"""Shared time formatting utilities."""


def format_duration(minutes: float) -> str:
    """Format minutes as a human-readable duration string.

    Uses floor rounding for consistency — 59.9 minutes displays as "59m", not "1h 00m".
    """
    hours = int(minutes // 60)
    mins = int(minutes % 60)
    if hours > 0:
        return f"{hours}h {mins:02d}m"
    return f"{mins}m"
