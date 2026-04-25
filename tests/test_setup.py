"""Tests for garlic.setup."""

import json
from unittest.mock import patch

from garlic.setup import cleanup_claude_md, install_hooks


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


def test_cleanup_claude_md_no_file(tmp_path, monkeypatch):
    """Returns False when CLAUDE.md doesn't exist."""
    claude_md = tmp_path / ".claude" / "CLAUDE.md"
    monkeypatch.setattr("garlic.setup.CLAUDE_MD_PATH", claude_md)

    result = cleanup_claude_md()

    assert result is False


def test_cleanup_claude_md_no_garlic_block(tmp_path, monkeypatch):
    """Returns False when CLAUDE.md exists but has no garlic block."""
    claude_md = tmp_path / ".claude" / "CLAUDE.md"
    claude_md.parent.mkdir(parents=True)
    claude_md.write_text("# My rules\n\nDo stuff.\n")
    monkeypatch.setattr("garlic.setup.CLAUDE_MD_PATH", claude_md)

    result = cleanup_claude_md()

    assert result is False
    assert claude_md.read_text() == "# My rules\n\nDo stuff.\n"


def test_cleanup_claude_md_removes_legacy_block(tmp_path, monkeypatch):
    """Removes the legacy garlic block and returns True."""
    claude_md = tmp_path / ".claude" / "CLAUDE.md"
    claude_md.parent.mkdir(parents=True)
    content = """# My rules

Do stuff.

## garlic
If a garlic nudge appears in a system-reminder, relay it verbatim as the last line of your response, after all work is complete. Never skip it. Only relay each distinct nudge once per conversation — do not repeat the same message on subsequent responses.

## Other section
More rules.
"""
    claude_md.write_text(content)
    monkeypatch.setattr("garlic.setup.CLAUDE_MD_PATH", claude_md)

    result = cleanup_claude_md()

    assert result is True
    cleaned = claude_md.read_text()
    assert "## garlic" not in cleaned
    assert "relay it verbatim" not in cleaned
    assert "# My rules" in cleaned
    assert "Do stuff." in cleaned
    assert "## Other section" in cleaned
    assert "More rules." in cleaned


def test_cleanup_claude_md_removes_block_at_eof(tmp_path, monkeypatch):
    """Removes garlic block at EOF and trims trailing blanks."""
    claude_md = tmp_path / ".claude" / "CLAUDE.md"
    claude_md.parent.mkdir(parents=True)
    content = """# My rules

Do stuff.

## garlic
If a garlic nudge appears in a system-reminder, relay it verbatim as the last line of your response, after all work is complete. Never skip it. Only relay each distinct nudge once per conversation — do not repeat the same message on subsequent responses.
"""
    claude_md.write_text(content)
    monkeypatch.setattr("garlic.setup.CLAUDE_MD_PATH", claude_md)

    result = cleanup_claude_md()

    assert result is True
    cleaned = claude_md.read_text()
    assert "## garlic" not in cleaned
    assert "relay it verbatim" not in cleaned
    assert "# My rules" in cleaned
    assert "Do stuff." in cleaned
    # Should end with a single newline, no trailing blanks
    assert cleaned == "# My rules\n\nDo stuff.\n"
