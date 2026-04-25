"""Shared test fixtures for garlic tests."""

import pytest


@pytest.fixture
def garlic_env(tmp_path, monkeypatch):
    """Redirect all garlic file I/O to a temporary directory.

    Patches GARLIC_DIR, CONFIG_PATH, and STATE_PATH in both
    garlic.config and garlic.state so no test ever touches the
    user's real ~/.garlic/ directory.

    Returns (garlic_dir, config_path, state_path).
    """
    garlic_dir = tmp_path / ".garlic"
    garlic_dir.mkdir()
    config_path = garlic_dir / "config.toml"
    state_path = garlic_dir / "state.toml"

    # Write default config
    config_path.write_text(
        'max_prompt_gap_minutes = 20\n'
        'reset_hour = 2\n'
        'nudge_thresholds_minutes = [60, 120, 180, 240]\n'
        'nudge_style = "gentle"\n'
    )

    # Write default state
    state_path.write_text(
        'date = "2026-03-16"\n'
        'accumulated_minutes = 0.0\n'
        'last_event_time = 0.0\n'
        'nudges_given = []\n'
        'ignored = false\n'
        'bedtime_nudge_given = false\n'
        'history = []\n'
    )

    version_cache_path = garlic_dir / "version_cache.toml"

    monkeypatch.setattr("garlic.config.GARLIC_DIR", garlic_dir)
    monkeypatch.setattr("garlic.config.CONFIG_PATH", config_path)
    monkeypatch.setattr("garlic.config.VERSION_CACHE_PATH", version_cache_path)
    monkeypatch.setattr("garlic.state.GARLIC_DIR", garlic_dir)
    monkeypatch.setattr("garlic.state.STATE_PATH", state_path)
    monkeypatch.setattr("garlic.cli.VERSION_CACHE_PATH", version_cache_path)

    return garlic_dir, config_path, state_path
