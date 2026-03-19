"""Tests for garlic.engine."""

from unittest.mock import patch

from garlic.engine import check_bedtime, handle_prompt, handle_session_start, handle_stop


def _make_config(**overrides):
    config = {
        "max_prompt_gap_minutes": 10,
        "nudge_thresholds_minutes": [60, 120, 180, 240],
    }
    config.update(overrides)
    return config


def _make_state(**overrides):
    state = {
        "date": "2026-03-16",
        "accumulated_minutes": 0.0,
        "last_event_time": 0.0,
        "nudges_given": [],
        "ignored": False,
    }
    state.update(overrides)
    return state


def test_handle_prompt_accumulates_time():
    """Prompt accumulates gap time capped at max_prompt_gap_minutes."""
    now = 1710567900.0
    # Last event was 5 minutes ago
    state = _make_state(last_event_time=now - 300)
    config = _make_config()

    with patch("garlic.engine.time") as mock_time:
        mock_time.time.return_value = now
        handle_prompt(state, config)

    assert abs(state["accumulated_minutes"] - 5.0) < 0.01
    assert state["last_event_time"] == now


def test_handle_prompt_drops_large_gap():
    """Gaps larger than max_prompt_gap_minutes are dropped (user was away)."""
    now = 1710567900.0
    # Last event was 30 minutes ago
    state = _make_state(last_event_time=now - 1800)
    config = _make_config(max_prompt_gap_minutes=10)

    with patch("garlic.engine.time") as mock_time:
        mock_time.time.return_value = now
        handle_prompt(state, config)

    assert state["accumulated_minutes"] == 0.0


def test_handle_prompt_first_event_no_accumulation():
    """First prompt (last_event_time=0) doesn't accumulate."""
    now = 1710567900.0
    state = _make_state(last_event_time=0.0)
    config = _make_config()

    with patch("garlic.engine.time") as mock_time:
        mock_time.time.return_value = now
        handle_prompt(state, config)

    assert state["accumulated_minutes"] == 0.0
    assert state["last_event_time"] == now


def test_handle_prompt_crosses_threshold():
    """Returns threshold when accumulated time crosses it."""
    now = 1710567900.0
    # Already at 58 minutes, 5-minute gap will cross 60
    state = _make_state(accumulated_minutes=58.0, last_event_time=now - 300)
    config = _make_config()

    with patch("garlic.engine.time") as mock_time:
        mock_time.time.return_value = now
        result = handle_prompt(state, config)

    assert result == 60
    assert 60 in state["nudges_given"]


def test_handle_prompt_no_duplicate_nudge():
    """Already-given thresholds are not returned again."""
    now = 1710567900.0
    state = _make_state(
        accumulated_minutes=58.0,
        last_event_time=now - 300,
        nudges_given=[60],
    )
    config = _make_config()

    with patch("garlic.engine.time") as mock_time:
        mock_time.time.return_value = now
        result = handle_prompt(state, config)

    assert result is None


def test_handle_prompt_crosses_multiple_returns_highest():
    """When multiple thresholds are crossed, returns the highest new one."""
    now = 1710567900.0
    # At 55 min, 10-min gap -> 65 min, crosses 60. But if we start at 115...
    state = _make_state(accumulated_minutes=115.0, last_event_time=now - 600)
    config = _make_config()

    with patch("garlic.engine.time") as mock_time:
        mock_time.time.return_value = now
        result = handle_prompt(state, config)

    # Crosses both 60 and 120; returns highest (120)
    assert result == 120
    assert 120 in state["nudges_given"]


def test_handle_stop_accumulates_generation_time():
    """Stop accumulates the gap since last event (generation time)."""
    now = 1710567900.0
    # Last event was 3 minutes ago (e.g. user submitted prompt)
    state = _make_state(accumulated_minutes=30.0, last_event_time=now - 180)
    config = _make_config()

    with patch("garlic.engine.time") as mock_time:
        mock_time.time.return_value = now
        handle_stop(state, config)

    assert state["last_event_time"] == now
    assert abs(state["accumulated_minutes"] - 33.0) < 0.01  # 30 + 3


def test_handle_stop_counts_long_generation_in_full():
    """Stop always counts generation time in full (no cap)."""
    now = 1710567900.0
    state = _make_state(accumulated_minutes=0.0, last_event_time=now - 3600)
    config = _make_config(max_prompt_gap_minutes=10)

    with patch("garlic.engine.time") as mock_time:
        mock_time.time.return_value = now
        handle_stop(state, config)

    assert abs(state["accumulated_minutes"] - 60.0) < 0.01  # full 60 minutes


def test_handle_stop_no_accumulation_without_last_event():
    """Stop with no prior last_event_time doesn't accumulate."""
    now = 1710567900.0
    state = _make_state(accumulated_minutes=0.0, last_event_time=0.0)
    config = _make_config()

    with patch("garlic.engine.time") as mock_time:
        mock_time.time.return_value = now
        handle_stop(state, config)

    assert state["last_event_time"] == now
    assert state["accumulated_minutes"] == 0.0


def test_handle_session_start_records_timestamp():
    """Session start records timestamp."""
    now = 1710567900.0
    state = _make_state()

    with patch("garlic.engine.time") as mock_time:
        mock_time.time.return_value = now
        handle_session_start(state)

    assert state["last_event_time"] == now


# ---------------------------------------------------------------------------
# Multi-event sequence tests
# ---------------------------------------------------------------------------

def _at(base, minutes):
    """Return a timestamp that is `minutes` after `base`."""
    return base + minutes * 60


def test_sequence_prompt_stop_prompt():
    """Full cycle: prompt gap + generation time + next prompt gap all accumulate."""
    base = 1710567900.0
    config = _make_config()

    # session-start at T=0
    state = _make_state()
    with patch("garlic.engine.time") as mt:
        mt.time.return_value = _at(base, 0)
        handle_session_start(state)

    # prompt at T=3 → gap from session-start: 3m, total=3
    with patch("garlic.engine.time") as mt:
        mt.time.return_value = _at(base, 3)
        handle_prompt(state, config)
    assert abs(state["accumulated_minutes"] - 3.0) < 0.01

    # stop at T=5 → generation took 2m, total=5
    with patch("garlic.engine.time") as mt:
        mt.time.return_value = _at(base, 5)
        handle_stop(state, config)
    assert abs(state["accumulated_minutes"] - 5.0) < 0.01
    assert abs(state["last_event_time"] - _at(base, 5)) < 0.01

    # prompt at T=7 → reading/thinking took 2m, total=7
    with patch("garlic.engine.time") as mt:
        mt.time.return_value = _at(base, 7)
        handle_prompt(state, config)
    assert abs(state["accumulated_minutes"] - 7.0) < 0.01


def test_sequence_multiple_prompts_accumulate():
    """Several back-to-back prompts accumulate correctly."""
    base = 1710567900.0
    config = _make_config()
    state = _make_state(last_event_time=base)

    gaps = [2, 4, 3, 5]  # minutes between prompts
    expected_total = sum(gaps)  # all under cap

    t = base
    for gap in gaps:
        t += gap * 60
        with patch("garlic.engine.time") as mt:
            mt.time.return_value = t
            handle_prompt(state, config)

    assert abs(state["accumulated_minutes"] - expected_total) < 0.01


def test_sequence_gap_exceeding_cap_then_normal():
    """A dropped gap followed by a normal gap accumulates correctly."""
    base = 1710567900.0
    config = _make_config(max_prompt_gap_minutes=10)
    state = _make_state(last_event_time=base)

    # 30-minute gap → dropped (exceeded cap, user was away)
    with patch("garlic.engine.time") as mt:
        mt.time.return_value = _at(base, 30)
        handle_prompt(state, config)
    assert state["accumulated_minutes"] == 0.0

    # 4-minute gap → added in full
    with patch("garlic.engine.time") as mt:
        mt.time.return_value = _at(base, 34)
        handle_prompt(state, config)
    assert abs(state["accumulated_minutes"] - 4.0) < 0.01


def test_all_thresholds_fire_in_order():
    """Each threshold fires exactly once as time accumulates past each one."""
    base = 1710567900.0
    # cap set above 65 so gaps are not truncated
    config = _make_config(nudge_thresholds_minutes=[60, 120, 180, 240], max_prompt_gap_minutes=70)
    state = _make_state(last_event_time=base)

    fired = []
    # Drive time in 65-minute chunks (each crosses exactly one new threshold)
    for i in range(1, 5):
        with patch("garlic.engine.time") as mt:
            mt.time.return_value = _at(base, i * 65)
            result = handle_prompt(state, config)
        if result is not None:
            fired.append(result)

    assert fired == [60, 120, 180, 240]
    assert state["nudges_given"] == [60, 120, 180, 240]


def test_threshold_not_refired_after_crossing():
    """Once a threshold is in nudges_given, it is never returned again."""
    base = 1710567900.0
    config = _make_config(nudge_thresholds_minutes=[60])
    state = _make_state(accumulated_minutes=59.0, last_event_time=base)

    # First prompt crosses 60
    with patch("garlic.engine.time") as mt:
        mt.time.return_value = _at(base, 2)
        r1 = handle_prompt(state, config)
    assert r1 == 60

    # Second prompt stays above 60 but threshold already given
    with patch("garlic.engine.time") as mt:
        mt.time.return_value = _at(base, 4)
        r2 = handle_prompt(state, config)
    assert r2 is None
    assert state["nudges_given"].count(60) == 1  # recorded only once


# ---------------------------------------------------------------------------
# Bedtime nudge tests
# ---------------------------------------------------------------------------


def test_check_bedtime_fires_in_window():
    """check_bedtime returns True when current hour is one before reset_hour."""
    from datetime import datetime

    state = _make_state()
    config = _make_config(reset_hour=2)

    with patch("garlic.engine.datetime") as mock_dt:
        mock_dt.now.return_value = datetime(2026, 3, 16, 1, 30)  # 1:30 AM
        result = check_bedtime(state, config)

    assert result is True
    assert state["bedtime_nudge_given"] is True


def test_check_bedtime_not_in_window():
    """check_bedtime returns False when outside the bedtime hour."""
    from datetime import datetime

    state = _make_state()
    config = _make_config(reset_hour=2)

    with patch("garlic.engine.datetime") as mock_dt:
        mock_dt.now.return_value = datetime(2026, 3, 16, 22, 0)  # 10 PM
        result = check_bedtime(state, config)

    assert result is False
    assert state.get("bedtime_nudge_given", False) is False


def test_check_bedtime_only_fires_once():
    """check_bedtime returns False on second call (already given)."""
    from datetime import datetime

    state = _make_state(bedtime_nudge_given=True)
    config = _make_config(reset_hour=2)

    with patch("garlic.engine.datetime") as mock_dt:
        mock_dt.now.return_value = datetime(2026, 3, 16, 1, 30)
        result = check_bedtime(state, config)

    assert result is False


def test_check_bedtime_wraps_midnight():
    """When reset_hour=0, bedtime hour is 23."""
    from datetime import datetime

    state = _make_state()
    config = _make_config(reset_hour=0)

    with patch("garlic.engine.datetime") as mock_dt:
        mock_dt.now.return_value = datetime(2026, 3, 16, 23, 15)
        result = check_bedtime(state, config)

    assert result is True
