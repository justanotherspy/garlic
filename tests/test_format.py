"""Tests for the shared time formatting helper."""

import pytest

from garlic._format import format_duration


@pytest.mark.parametrize(
    "minutes,expected",
    [
        # Minutes only (under 1 hour)
        (0, "0m"),
        (1, "1m"),
        (30, "30m"),
        (59, "59m"),
        # Hours + minutes
        (60, "1h 00m"),
        (61, "1h 01m"),
        (90, "1h 30m"),
        (119, "1h 59m"),
        (120, "2h 00m"),
        (150, "2h 30m"),
        # Fractional minutes — floor, not round (the bug fix)
        (59.5, "59m"),
        (59.8, "59m"),
        (59.9, "59m"),
        (60.1, "1h 00m"),
        (60.9, "1h 00m"),
        (119.9, "1h 59m"),
        (120.1, "2h 00m"),
        # Large values
        (240, "4h 00m"),
        (500, "8h 20m"),
    ],
)
def test_format_duration(minutes: float, expected: str) -> None:
    assert format_duration(minutes) == expected
