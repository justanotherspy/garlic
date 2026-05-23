//! Black-box integration tests driving the `garlic` binary.
//!
//! Every test points `GARLIC_DIR` (and `CLAUDE_HOME` for setup) at a temp
//! directory so the real `~/.garlic/` and `~/.claude/` are never touched.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use assert_cmd::Command;
use garlic::paths::Paths;
use garlic::state::{current_date, save_state, State};
use predicates::prelude::*;
use tempfile::TempDir;

fn unix_now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}

fn garlic(dir: &Path) -> Command {
    let mut cmd = Command::cargo_bin("garlic").unwrap();
    cmd.env("GARLIC_DIR", dir);
    cmd
}

/// A temp dir containing a `.garlic` directory with state seeded for today.
fn seeded(state: State) -> TempDir {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().join(".garlic");
    std::fs::create_dir_all(&dir).unwrap();
    save_state(&Paths::with_dir(&dir), &state).unwrap();
    tmp
}

fn gdir(tmp: &TempDir) -> std::path::PathBuf {
    tmp.path().join(".garlic")
}

#[test]
fn version_flag_prints_name_and_version() {
    let tmp = TempDir::new().unwrap();
    garlic(tmp.path())
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::starts_with("garlic "));
}

#[test]
fn version_subcommand_no_update_with_fresh_cache() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().join(".garlic");
    std::fs::create_dir_all(&dir).unwrap();
    // Far-future checked_at keeps the cache fresh; empty latest => no update.
    std::fs::write(
        dir.join("version_cache.toml"),
        "checked_at = 9999999999.0\nlatest_version = \"\"\n",
    )
    .unwrap();

    garlic(&dir)
        .arg("version")
        .assert()
        .success()
        .stdout(predicate::str::contains("garlic "))
        .stdout(predicate::str::contains("update available").not());
}

#[test]
fn version_subcommand_shows_update_from_cache() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().join(".garlic");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("version_cache.toml"),
        "checked_at = 9999999999.0\nlatest_version = \"99.0.0\"\n",
    )
    .unwrap();

    garlic(&dir)
        .arg("version")
        .assert()
        .success()
        .stdout(predicate::str::contains("update available: 99.0.0"))
        .stdout(predicate::str::contains("cargo install garlic-cli"));
}

#[test]
fn no_args_prints_help_and_exits_1() {
    let tmp = TempDir::new().unwrap();
    garlic(tmp.path())
        .assert()
        .failure()
        .code(1)
        .stdout(predicate::str::contains("Usage"));
}

#[test]
fn status_json_emits_valid_object() {
    let tmp = seeded(State {
        date: current_date(2),
        accumulated_minutes: 45.0,
        nudges_given: vec![30],
        ..State::default()
    });
    garlic(&gdir(&tmp))
        .args(["status", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"accumulated_minutes\":45.0"))
        .stdout(predicate::str::contains("\"next_threshold\":60"));
}

#[test]
fn statusline_single_line() {
    let tmp = seeded(State {
        date: current_date(2),
        accumulated_minutes: 45.0,
        ..State::default()
    });
    let assert = garlic(&gdir(&tmp)).arg("statusline").assert().success();
    let out = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert_eq!(out.lines().count(), 1);
    assert!(out.contains("\u{1f9c4}") || out.contains("\u{1f9db}"));
}

#[test]
fn set_updates_config() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().join(".garlic");
    std::fs::create_dir_all(&dir).unwrap();

    garlic(&dir)
        .args(["set", "nudge_style=spicy"])
        .assert()
        .success()
        .stdout(predicate::str::contains("nudge_style = spicy"));

    let config = std::fs::read_to_string(dir.join("config.toml")).unwrap();
    assert!(config.contains("nudge_style = \"spicy\""));
}

#[test]
fn set_unknown_key_fails() {
    let tmp = TempDir::new().unwrap();
    garlic(tmp.path())
        .args(["set", "bogus=1"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("unknown config key"));
}

#[test]
fn reset_yes_zeroes_timer() {
    let tmp = seeded(State {
        date: current_date(2),
        accumulated_minutes: 95.0,
        nudges_given: vec![60],
        ..State::default()
    });
    let dir = gdir(&tmp);
    garlic(&dir)
        .args(["reset", "-y"])
        .assert()
        .success()
        .stdout(predicate::str::contains("timer reset"));

    garlic(&dir)
        .args(["status", "--json"])
        .assert()
        .stdout(predicate::str::contains("\"accumulated_minutes\":0.0"));
}

#[test]
fn ignore_toggles_pause() {
    let tmp = seeded(State {
        date: current_date(2),
        accumulated_minutes: 10.0,
        ..State::default()
    });
    let dir = gdir(&tmp);
    garlic(&dir)
        .arg("ignore")
        .assert()
        .success()
        .stdout(predicate::str::contains("nudging disabled"));
    garlic(&dir)
        .arg("ignore")
        .assert()
        .success()
        .stdout(predicate::str::contains("nudging resumed"));
}

#[test]
fn hook_prompt_accumulates_and_nudges() {
    let tmp = seeded(State {
        date: current_date(2),
        accumulated_minutes: 58.0,
        last_event_time: unix_now() - 180.0, // ~3 min ago, within the gap cap
        ..State::default()
    });
    garlic(&gdir(&tmp))
        .args(["hook", "prompt"])
        .write_stdin("{\"session_id\":\"x\"}")
        .assert()
        .success()
        .stdout(predicate::str::contains("hookSpecificOutput"))
        .stdout(predicate::str::contains("UserPromptSubmit"));
}

#[test]
fn hook_prompt_below_threshold_is_silent() {
    let tmp = seeded(State {
        date: current_date(2),
        accumulated_minutes: 5.0,
        last_event_time: unix_now() - 60.0,
        ..State::default()
    });
    garlic(&gdir(&tmp))
        .args(["hook", "prompt"])
        .write_stdin("{}")
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
}

#[test]
fn hook_session_start_records_and_shows_time() {
    let tmp = seeded(State {
        date: current_date(2),
        accumulated_minutes: 95.0,
        ..State::default()
    });
    garlic(&gdir(&tmp))
        .args(["hook", "session-start"])
        .write_stdin("{}")
        .assert()
        .success()
        .stdout(predicate::str::contains("1h 35m"));
}

#[test]
fn setup_installs_hooks_and_command() {
    let tmp = TempDir::new().unwrap();
    let garlic_dir = tmp.path().join(".garlic");
    let claude_dir = tmp.path().join(".claude");

    Command::cargo_bin("garlic")
        .unwrap()
        .env("GARLIC_DIR", &garlic_dir)
        .env("CLAUDE_HOME", &claude_dir)
        .args(["setup", "-y"])
        .assert()
        .success()
        .stdout(predicate::str::contains("hooks installed"));

    let settings = std::fs::read_to_string(claude_dir.join("settings.json")).unwrap();
    assert!(settings.contains("garlic hook prompt"));
    assert!(settings.contains("garlic hook session-end"));
    assert!(claude_dir.join("commands").join("garlic.md").exists());
}

#[test]
fn week_and_stats_run() {
    let tmp = seeded(State {
        date: current_date(2),
        accumulated_minutes: 60.0,
        ..State::default()
    });
    let dir = gdir(&tmp);
    garlic(&dir)
        .arg("week")
        .assert()
        .success()
        .stdout(predicate::str::contains("Weekly usage"));
    garlic(&dir)
        .arg("stats")
        .assert()
        .success()
        .stdout(predicate::str::contains("Stats"));
}
