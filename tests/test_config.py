"""Tests for garlic.config."""

import tomllib
from pathlib import Path

from garlic.config import DEFAULTS, load_config, _write_toml


def test_load_config_creates_default(tmp_path, monkeypatch):
    """When no config exists, load_config creates one with defaults."""
    garlic_dir = tmp_path / ".garlic"
    monkeypatch.setattr("garlic.config.GARLIC_DIR", garlic_dir)
    monkeypatch.setattr("garlic.config.CONFIG_PATH", garlic_dir / "config.toml")

    config = load_config()

    assert config == DEFAULTS
    assert (garlic_dir / "config.toml").exists()

    # Verify the written file round-trips correctly
    with (garlic_dir / "config.toml").open("rb") as f:
        on_disk = tomllib.load(f)
    assert on_disk == DEFAULTS


def test_load_config_reads_existing(tmp_path, monkeypatch):
    """When config exists, load_config reads it and merges with defaults."""
    garlic_dir = tmp_path / ".garlic"
    garlic_dir.mkdir()
    config_path = garlic_dir / "config.toml"
    config_path.write_text('nudge_style = "spicy"\n')

    monkeypatch.setattr("garlic.config.GARLIC_DIR", garlic_dir)
    monkeypatch.setattr("garlic.config.CONFIG_PATH", config_path)

    config = load_config()

    assert config["nudge_style"] == "spicy"
    # Defaults still present for unspecified keys
    assert config["max_prompt_gap_minutes"] == 20
    assert config["nudge_thresholds_minutes"] == [60, 120, 180, 240]


def test_load_config_user_overrides_all(tmp_path, monkeypatch):
    """User values override all defaults."""
    garlic_dir = tmp_path / ".garlic"
    garlic_dir.mkdir()
    config_path = garlic_dir / "config.toml"
    config_path.write_text(
        'max_prompt_gap_minutes = 5\n'
        'reset_hour = 4\n'
        'nudge_thresholds_minutes = [30, 60]\n'
        'nudge_style = "firm"\n'
    )

    monkeypatch.setattr("garlic.config.GARLIC_DIR", garlic_dir)
    monkeypatch.setattr("garlic.config.CONFIG_PATH", config_path)

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
