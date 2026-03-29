"""Tests for garlic.nudges."""

from garlic.nudges import FINAL, POOLS, _format_time, get_nudge


def test_format_time_minutes():
    assert _format_time(30) == "30m"
    assert _format_time(1) == "1m"


def test_format_time_hours():
    assert _format_time(60) == "1h 00m"
    assert _format_time(89) == "1h 29m"
    assert _format_time(90) == "1h 30m"
    assert _format_time(120) == "2h 00m"
    assert _format_time(185) == "3h 05m"


def test_get_nudge_contains_time():
    msg = get_nudge("gentle", 120)
    assert "2h 00m" in msg


def test_get_nudge_all_styles():
    for style in ("gentle", "firm", "spicy"):
        msg = get_nudge(style, 60)
        assert "1h 00m" in msg
        assert len(msg) > 10


def test_get_nudge_unknown_style_falls_back_to_gentle():
    msg = get_nudge("unknown", 60)
    # Should still produce a valid message from the gentle pool
    assert "1h 00m" in msg


def test_get_nudge_is_final_uses_final_pool():
    # Run several times to confirm it always comes from FINAL
    for _ in range(20):
        msg = get_nudge("gentle", 240, is_final=True)
        assert any(msg == m.format(time="4h 00m") for m in FINAL)


def test_all_pool_messages_have_placeholder():
    for style, pool in POOLS.items():
        for msg in pool:
            assert "{time}" in msg, f"Missing {{time}} in {style}: {msg}"
    for msg in FINAL:
        assert "{time}" in msg, f"Missing {{time}} in FINAL: {msg}"
