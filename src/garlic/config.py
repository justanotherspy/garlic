"""Load and create ~/.garlic/config.toml with defaults."""

import tomllib
from pathlib import Path
from typing import Any

GARLIC_DIR = Path.home() / ".garlic"
CONFIG_PATH = GARLIC_DIR / "config.toml"

DEFAULTS: dict[str, Any] = {
    "max_prompt_gap_minutes": 40,
    "reset_hour": 2,
    "nudge_thresholds_minutes": [60, 120, 180, 240],
    "nudge_style": "gentle",
}


def _write_toml(path: Path, data: dict[str, Any]) -> None:
    """Write a flat dict as TOML (simple key-value format)."""
    lines: list[str] = []
    for key, value in data.items():
        if isinstance(value, str):
            lines.append(f'{key} = "{value}"')
        elif isinstance(value, bool):
            lines.append(f"{key} = {'true' if value else 'false'}")
        elif isinstance(value, (int, float)):
            lines.append(f"{key} = {value}")
        elif isinstance(value, list):
            items = ", ".join(
                f'"{v}"' if isinstance(v, str) else str(v) for v in value
            )
            lines.append(f"{key} = [{items}]")
        else:
            raise TypeError(f"Unsupported TOML value type: {type(value)}")
    path.write_text("\n".join(lines) + "\n")


def save_config(config: dict[str, Any]) -> None:
    """Write config to disk."""
    GARLIC_DIR.mkdir(parents=True, exist_ok=True)
    _write_toml(CONFIG_PATH, config)


def load_config() -> dict[str, Any]:
    """Load config from disk, creating it with defaults if missing."""
    if not CONFIG_PATH.exists():
        GARLIC_DIR.mkdir(parents=True, exist_ok=True)
        _write_toml(CONFIG_PATH, DEFAULTS)
        return dict(DEFAULTS)

    with CONFIG_PATH.open("rb") as f:
        user_config = tomllib.load(f)

    # Merge defaults for any missing keys
    merged = dict(DEFAULTS)
    merged.update(user_config)
    return merged
