"""Tests for garlic.setup."""

import json

from garlic.setup import install_hooks


def test_install_hooks_creates_settings(tmp_path, monkeypatch):
    """Creates settings.json with hooks when it doesn't exist."""
    settings_path = tmp_path / ".claude" / "settings.json"
    monkeypatch.setattr("garlic.setup.CLAUDE_SETTINGS_PATH", settings_path)
    monkeypatch.setattr("garlic.setup.load_config", lambda: {})

    install_hooks()

    settings = json.loads(settings_path.read_text())
    hooks = settings["hooks"]

    assert len(hooks["SessionStart"]) == 1
    assert hooks["SessionStart"][0]["command"] == "garlic hook session-start"
    assert hooks["SessionStart"][0]["matcher"] == "startup"

    assert len(hooks["UserPromptSubmit"]) == 1
    assert hooks["UserPromptSubmit"][0]["command"] == "garlic hook prompt"

    assert len(hooks["Stop"]) == 1
    assert hooks["Stop"][0]["command"] == "garlic hook stop"


def test_install_hooks_idempotent(tmp_path, monkeypatch):
    """Running setup twice doesn't duplicate hooks."""
    settings_path = tmp_path / ".claude" / "settings.json"
    monkeypatch.setattr("garlic.setup.CLAUDE_SETTINGS_PATH", settings_path)
    monkeypatch.setattr("garlic.setup.load_config", lambda: {})

    install_hooks()
    install_hooks()

    settings = json.loads(settings_path.read_text())
    for event in ("SessionStart", "UserPromptSubmit", "Stop"):
        assert len(settings["hooks"][event]) == 1


def test_install_hooks_preserves_other_hooks(tmp_path, monkeypatch):
    """Existing non-garlic hooks are preserved."""
    settings_path = tmp_path / ".claude" / "settings.json"
    settings_path.parent.mkdir(parents=True)
    existing = {
        "hooks": {
            "UserPromptSubmit": [
                {"type": "command", "command": "other-tool do-stuff"}
            ]
        },
        "other_setting": True,
    }
    settings_path.write_text(json.dumps(existing))
    monkeypatch.setattr("garlic.setup.CLAUDE_SETTINGS_PATH", settings_path)
    monkeypatch.setattr("garlic.setup.load_config", lambda: {})

    install_hooks()

    settings = json.loads(settings_path.read_text())
    prompt_hooks = settings["hooks"]["UserPromptSubmit"]
    assert len(prompt_hooks) == 2
    assert prompt_hooks[0]["command"] == "other-tool do-stuff"
    assert prompt_hooks[1]["command"] == "garlic hook prompt"
    assert settings["other_setting"] is True
