"""Tests for garlic.config."""

import tomllib
from pathlib import Path

from garlic.config import DEFAULTS, load_config, _write_toml


def test_load_config_creates_default(garlic_env):
    """When no config exists, load_config creates one with defaults."""
    garlic_dir, config_path, _ = garlic_env
    config_path.unlink()  # remove default so it creates fresh

    config = load_config()

    assert config == DEFAULTS
    assert config_path.exists()

    # Verify the written file round-trips correctly
    with config_path.open("rb") as f:
        on_disk = tomllib.load(f)
    assert on_disk == DEFAULTS


def test_load_config_reads_existing(garlic_env):
    """When config exists, load_config reads it and merges with defaults."""
    _, config_path, _ = garlic_env
    config_path.write_text('nudge_style = "spicy"\n')

    config = load_config()

    assert config["nudge_style"] == "spicy"
    # Defaults still present for unspecified keys
    assert config["max_prompt_gap_minutes"] == 40
    assert config["nudge_thresholds_minutes"] == [30, 60, 90, 120, 150, 180, 210, 240]


def test_load_config_user_overrides_all(garlic_env):
    """User values override all defaults."""
    _, config_path, _ = garlic_env
    config_path.write_text(
        'max_prompt_gap_minutes = 5\n'
        'reset_hour = 4\n'
        'nudge_thresholds_minutes = [30, 60]\n'
        'nudge_style = "firm"\n'
    )

    config = load_config()

    assert config["max_prompt_gap_minutes"] == 5
    assert config["reset_hour"] == 4
    assert config["nudge_thresholds_minutes"] == [30, 60]
    assert config["nudge_style"] == "firm"


def test_write_toml_types(tmp_path):
    """_write_toml handles all supported value types."""
    path = tmp_path / "test.toml"
    data = {
        "a_string": "hello",
        "an_int": 42,
        "a_float": 3.14,
        "a_bool": True,
        "a_list": [1, 2, 3],
    }
    _write_toml(path, data)

    with path.open("rb") as f:
        result = tomllib.load(f)

    assert result["a_string"] == "hello"
    assert result["an_int"] == 42
    assert result["a_float"] == 3.14
    assert result["a_bool"] is True
    assert result["a_list"] == [1, 2, 3]
