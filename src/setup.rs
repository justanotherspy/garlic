//! Install/update garlic hooks in `~/.claude/settings.json`.

use std::fs;
use std::io::{self, Write};
use std::path::Path;

use serde_json::{json, Value};
use tempfile::NamedTempFile;

use crate::paths::ClaudePaths;

pub const GARLIC_COMMAND: &str = "---\ndescription: Show accumulated active coding time today\nargument-hint: \"[status|status --week|status --month|ignore]\"\nallowed-tools: Bash(garlic:*)\n---\nRun `garlic $ARGUMENTS` (or `garlic status` if no arg) and show the output to me.\n";

/// (event, matcher, base command) for each hook garlic installs.
const HOOK_DEFINITIONS: [(&str, &str, &str); 4] = [
    ("SessionStart", "startup", "garlic hook session-start"),
    ("UserPromptSubmit", "", "garlic hook prompt"),
    ("Stop", "", "garlic hook stop"),
    ("SessionEnd", "", "garlic hook session-end"),
];

fn hook_definitions(debug: bool) -> Vec<(&'static str, &'static str, String)> {
    let suffix = if debug { " --debug" } else { "" };
    HOOK_DEFINITIONS
        .iter()
        .map(|(ev, m, cmd)| (*ev, *m, format!("{cmd}{suffix}")))
        .collect()
}

/// Check whether a hook entry belongs to garlic (new envelope or legacy flat).
fn is_garlic_entry(entry: &Value) -> bool {
    if let Some(hooks) = entry.get("hooks").and_then(Value::as_array) {
        let matches = hooks.iter().any(|h| {
            h.get("type").and_then(Value::as_str) == Some("command")
                && h.get("command")
                    .and_then(Value::as_str)
                    .is_some_and(|c| c.starts_with("garlic hook"))
        });
        if matches {
            return true;
        }
    }
    entry.get("type").and_then(Value::as_str) == Some("command")
        && entry
            .get("command")
            .and_then(Value::as_str)
            .is_some_and(|c| c.starts_with("garlic hook"))
}

/// Write JSON to `path` atomically via temp-file + rename.
///
/// The original file is never opened for writing, so a failed rename leaves it
/// untouched.
fn atomic_write_json(path: &Path, data: &Value) -> io::Result<()> {
    let content = serde_json::to_string_pretty(data)? + "\n";
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut tmp = NamedTempFile::new_in(parent)?;
    tmp.write_all(content.as_bytes())?;
    tmp.flush()?;
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

/// Install garlic hooks into `~/.claude/settings.json` (idempotent).
pub fn install_hooks(claude: &ClaudePaths, debug: bool) -> io::Result<()> {
    let settings_path = claude.settings();
    if let Some(parent) = settings_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut settings: Value = if settings_path.exists() {
        let text = fs::read_to_string(&settings_path)?;
        serde_json::from_str(&text).unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };
    if !settings.is_object() {
        settings = json!({});
    }

    {
        let obj = settings.as_object_mut().unwrap();
        let hooks = obj.entry("hooks").or_insert_with(|| json!({}));
        if !hooks.is_object() {
            *hooks = json!({});
        }
        let hooks_obj = hooks.as_object_mut().unwrap();

        for (event, matcher, command) in hook_definitions(debug) {
            let list = hooks_obj.entry(event).or_insert_with(|| json!([]));
            if !list.is_array() {
                *list = json!([]);
            }
            let arr = list.as_array_mut().unwrap();
            arr.retain(|e| !is_garlic_entry(e));
            arr.push(json!({
                "matcher": matcher,
                "hooks": [{"type": "command", "command": command}],
            }));
        }
    }

    atomic_write_json(&settings_path, &settings)?;

    install_slash_command(claude)?;

    if cleanup_claude_md(&claude.claude_md())? {
        println!("removed legacy CLAUDE.md injection");
    }
    Ok(())
}

/// Install the `/garlic` slash command into `~/.claude/commands/`.
pub fn install_slash_command(claude: &ClaudePaths) -> io::Result<()> {
    let dir = claude.commands_dir();
    fs::create_dir_all(&dir)?;
    fs::write(dir.join("garlic.md"), GARLIC_COMMAND)
}

/// Remove the legacy garlic nudge-relay block from `~/.claude/CLAUDE.md`.
///
/// Returns `true` if the block was found and removed.
pub fn cleanup_claude_md(path: &Path) -> io::Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    let content = fs::read_to_string(path)?;
    if !content.contains("## garlic") {
        return Ok(false);
    }

    let mut cleaned: Vec<&str> = Vec::new();
    let mut in_section = false;
    for line in content.split('\n') {
        if line.trim().starts_with("## garlic") {
            in_section = true;
            continue;
        }
        if in_section && line.trim().starts_with('#') {
            in_section = false;
        }
        if in_section {
            continue;
        }
        cleaned.push(line);
    }
    while cleaned.last().is_some_and(|l| l.trim().is_empty()) {
        cleaned.pop();
    }
    let mut cleaned_content = cleaned.join("\n");
    if !cleaned_content.is_empty() && !cleaned_content.ends_with('\n') {
        cleaned_content.push('\n');
    }
    fs::write(path, cleaned_content)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn claude_paths() -> (TempDir, ClaudePaths) {
        let tmp = TempDir::new().unwrap();
        let claude = ClaudePaths::with_dir(tmp.path().join(".claude"));
        (tmp, claude)
    }

    fn read_settings(claude: &ClaudePaths) -> Value {
        serde_json::from_str(&fs::read_to_string(claude.settings()).unwrap()).unwrap()
    }

    #[test]
    fn install_creates_settings() {
        let (_tmp, claude) = claude_paths();
        install_hooks(&claude, false).unwrap();
        let settings = read_settings(&claude);
        let hooks = &settings["hooks"];

        assert_eq!(hooks["SessionStart"].as_array().unwrap().len(), 1);
        assert_eq!(hooks["SessionStart"][0]["matcher"], "startup");
        assert_eq!(
            hooks["SessionStart"][0]["hooks"][0]["command"],
            "garlic hook session-start"
        );
        assert_eq!(hooks["UserPromptSubmit"][0]["matcher"], "");
        assert_eq!(
            hooks["UserPromptSubmit"][0]["hooks"][0]["command"],
            "garlic hook prompt"
        );
        assert_eq!(hooks["Stop"][0]["hooks"][0]["command"], "garlic hook stop");
        assert_eq!(
            hooks["SessionEnd"][0]["hooks"][0]["command"],
            "garlic hook session-end"
        );

        // Slash command installed.
        assert!(claude.commands_dir().join("garlic.md").exists());
    }

    #[test]
    fn install_is_idempotent() {
        let (_tmp, claude) = claude_paths();
        install_hooks(&claude, false).unwrap();
        install_hooks(&claude, false).unwrap();
        let settings = read_settings(&claude);
        for event in ["SessionStart", "UserPromptSubmit", "Stop", "SessionEnd"] {
            assert_eq!(settings["hooks"][event].as_array().unwrap().len(), 1);
        }
    }

    #[test]
    fn install_preserves_other_hooks() {
        let (_tmp, claude) = claude_paths();
        fs::create_dir_all(claude.settings().parent().unwrap()).unwrap();
        let existing = json!({
            "hooks": {
                "UserPromptSubmit": [
                    {"type": "command", "command": "other-tool do-stuff"}
                ]
            },
            "other_setting": true,
        });
        fs::write(claude.settings(), serde_json::to_string(&existing).unwrap()).unwrap();

        install_hooks(&claude, false).unwrap();

        let settings = read_settings(&claude);
        let prompt_hooks = settings["hooks"]["UserPromptSubmit"].as_array().unwrap();
        assert_eq!(prompt_hooks.len(), 2);
        assert_eq!(prompt_hooks[0]["command"], "other-tool do-stuff");
        assert_eq!(prompt_hooks[1]["hooks"][0]["command"], "garlic hook prompt");
        assert_eq!(settings["other_setting"], true);
    }

    #[test]
    fn install_debug_appends_flag() {
        let (_tmp, claude) = claude_paths();
        install_hooks(&claude, true).unwrap();
        let settings = read_settings(&claude);
        assert_eq!(
            settings["hooks"]["Stop"][0]["hooks"][0]["command"],
            "garlic hook stop --debug"
        );
    }

    #[test]
    fn cleanup_no_file() {
        let (_tmp, claude) = claude_paths();
        assert!(!cleanup_claude_md(&claude.claude_md()).unwrap());
    }

    #[test]
    fn cleanup_no_garlic_block() {
        let (_tmp, claude) = claude_paths();
        let path = claude.claude_md();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "# My rules\n\nDo stuff.\n").unwrap();
        assert!(!cleanup_claude_md(&path).unwrap());
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "# My rules\n\nDo stuff.\n"
        );
    }

    #[test]
    fn cleanup_removes_legacy_block() {
        let (_tmp, claude) = claude_paths();
        let path = claude.claude_md();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let content = "# My rules\n\nDo stuff.\n\n## garlic\nIf a garlic nudge appears in a system-reminder, relay it verbatim.\n\n## Other section\nMore rules.\n";
        fs::write(&path, content).unwrap();

        assert!(cleanup_claude_md(&path).unwrap());
        let cleaned = fs::read_to_string(&path).unwrap();
        assert!(!cleaned.contains("## garlic"));
        assert!(!cleaned.contains("relay it verbatim"));
        assert!(cleaned.contains("# My rules"));
        assert!(cleaned.contains("Do stuff."));
        assert!(cleaned.contains("## Other section"));
        assert!(cleaned.contains("More rules."));
    }

    #[test]
    fn cleanup_removes_block_at_eof() {
        let (_tmp, claude) = claude_paths();
        let path = claude.claude_md();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let content = "# My rules\n\nDo stuff.\n\n## garlic\nIf a garlic nudge appears in a system-reminder, relay it verbatim.\n";
        fs::write(&path, content).unwrap();

        assert!(cleanup_claude_md(&path).unwrap());
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "# My rules\n\nDo stuff.\n"
        );
    }
}
