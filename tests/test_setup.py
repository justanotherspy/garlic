"""Tests for garlic.setup."""

import json
from unittest.mock import patch

from garlic.setup import install_claude_md, install_hooks


def test_install_hooks_creates_settings(tmp_path, monkeypatch):
    """Creates settings.json with hooks when it doesn't exist."""
    settings_path = tmp_path / ".claude" / "settings.json"
    monkeypatch.setattr("garlic.setup.CLAUDE_SETTINGS_PATH", settings_path)
    monkeypatch.setattr("garlic.setup.load_config", lambda: {})
    monkeypatch.setattr("garlic.setup.save_config", lambda c: None)

    install_hooks()

    settings = json.loads(settings_path.read_text())
    hooks = settings["hooks"]

    assert len(hooks["SessionStart"]) == 1
    ss = hooks["SessionStart"][0]
    assert ss["matcher"] == "startup"
    assert ss["hooks"][0]["command"] == "garlic hook session-start"

    assert len(hooks["UserPromptSubmit"]) == 1
    up = hooks["UserPromptSubmit"][0]
    assert up["matcher"] == ""
    assert up["hooks"][0]["command"] == "garlic hook prompt"

    assert len(hooks["Stop"]) == 1
    st = hooks["Stop"][0]
    assert st["matcher"] == ""
    assert st["hooks"][0]["command"] == "garlic hook stop"

    assert len(hooks["SessionEnd"]) == 1
    se = hooks["SessionEnd"][0]
    assert se["matcher"] == ""
    assert se["hooks"][0]["command"] == "garlic hook session-end"


def test_install_hooks_idempotent(tmp_path, monkeypatch):
    """Running setup twice doesn't duplicate hooks."""
    settings_path = tmp_path / ".claude" / "settings.json"
    monkeypatch.setattr("garlic.setup.CLAUDE_SETTINGS_PATH", settings_path)
    monkeypatch.setattr("garlic.setup.load_config", lambda: {})
    monkeypatch.setattr("garlic.setup.save_config", lambda c: None)

    install_hooks()
    install_hooks()

    settings = json.loads(settings_path.read_text())
    for event in ("SessionStart", "UserPromptSubmit", "Stop", "SessionEnd"):
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
    monkeypatch.setattr("garlic.setup.save_config", lambda c: None)

    install_hooks()

    settings = json.loads(settings_path.read_text())
    prompt_hooks = settings["hooks"]["UserPromptSubmit"]
    assert len(prompt_hooks) == 2
    assert prompt_hooks[0]["command"] == "other-tool do-stuff"
    assert prompt_hooks[1]["hooks"][0]["command"] == "garlic hook prompt"
    assert settings["other_setting"] is True


def test_install_hooks_atomic_preserves_file_on_write_failure(tmp_path, monkeypatch):
    """If the write fails, the original settings.json is untouched."""
    settings_path = tmp_path / ".claude" / "settings.json"
    settings_path.parent.mkdir(parents=True)
    original = {"permissions": {"allow": ["Bash"]}, "hooks": {}}
    original_text = json.dumps(original, indent=2) + "\n"
    settings_path.write_text(original_text)
    monkeypatch.setattr("garlic.setup.CLAUDE_SETTINGS_PATH", settings_path)
    monkeypatch.setattr("garlic.setup.load_config", lambda: {})
    monkeypatch.setattr("garlic.setup.save_config", lambda c: None)

    with patch("garlic.setup.os.replace", side_effect=OSError("disk full")):
        try:
            install_hooks()
        except OSError:
            pass

    # Original file must be intact
    assert settings_path.read_text() == original_text


def test_install_claude_md_creates_file(tmp_path, monkeypatch):
    """Creates CLAUDE.md with garlic block when it doesn't exist."""
    claude_md = tmp_path / ".claude" / "CLAUDE.md"
    monkeypatch.setattr("garlic.setup.CLAUDE_MD_PATH", claude_md)

    install_claude_md()

    content = claude_md.read_text()
    assert "## garlic" in content
    assert "relay it verbatim" in content


def test_install_claude_md_appends_to_existing(tmp_path, monkeypatch):
    """Appends garlic block to an existing CLAUDE.md."""
    claude_md = tmp_path / ".claude" / "CLAUDE.md"
    claude_md.parent.mkdir(parents=True)
    claude_md.write_text("# My rules\n\nDo stuff.\n")
    monkeypatch.setattr("garlic.setup.CLAUDE_MD_PATH", claude_md)

    install_claude_md()

    content = claude_md.read_text()
    assert content.startswith("# My rules")
    assert "## garlic" in content


def test_install_claude_md_idempotent(tmp_path, monkeypatch):
    """Running install_claude_md twice doesn't duplicate the block."""
    claude_md = tmp_path / ".claude" / "CLAUDE.md"
    monkeypatch.setattr("garlic.setup.CLAUDE_MD_PATH", claude_md)

    install_claude_md()
    install_claude_md()

    content = claude_md.read_text()
    assert content.count("## garlic") == 1
