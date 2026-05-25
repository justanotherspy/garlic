//! Black-box integration tests driving the `garlic` binary.
//!
//! Every test points `GARLIC_DIR` (and `CLAUDE_HOME` for setup) at a temp
//! directory so the real `~/.garlic/` and `~/.claude/` are never touched.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use assert_cmd::Command;
use garlic::intervals::{Kind, OpenCursor};
use garlic::paths::Paths;
use garlic::state::{current_date, save_state, State};
use httpmock::prelude::*;
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
        .stdout(predicate::str::contains("cargo install garlic-ward"));
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
        // An open thinking interval ~3 min old (within the gap cap) for the
        // session the prompt below belongs to.
        open: vec![OpenCursor {
            session_id: "x".to_string(),
            kind: Kind::User,
            start: unix_now() - 180.0,
        }],
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
        // Empty stdin → session "default"; open a matching thinking cursor.
        open: vec![OpenCursor {
            session_id: "default".to_string(),
            kind: Kind::User,
            start: unix_now() - 60.0,
        }],
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

// --- Remote (shared-state backend) integration ---------------------------
//
// garlic is local-first: hooks always write `state.toml`; a configured backend
// is a sync peer that merges intervals (`POST /v1/intervals`) and returns the
// shared total. Hooks only flush inline when `GARLIC_SYNC=blocking` is set.

fn intervals_response(minutes: f64, nudges: &str, crossed: &str) -> String {
    format!(
        r#"{{"state":{{"date":"{date}","accumulated_minutes":{minutes},"last_event_time":0.0,"nudges_given":{nudges},"ignored":false,"bedtime_nudge_given":false,"history":[],"intervals":[],"open":[]}},"crossed_threshold":{crossed},"bedtime":false}}"#,
        date = current_date(2),
    )
}

#[test]
fn status_flushes_and_shows_shared_total() {
    let server = MockServer::start();
    // status pushes local intervals to the merge endpoint and renders the
    // merged total it gets back.
    let m = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/intervals")
            .header("authorization", "Bearer tok");
        then.status(200)
            .body(intervals_response(123.0, "[60]", "null"));
    });

    // Seed a *different* local total to prove the merged value wins.
    let tmp = seeded(State {
        date: current_date(2),
        accumulated_minutes: 45.0,
        ..State::default()
    });

    garlic(&gdir(&tmp))
        .env("GARLIC_URL", server.base_url())
        .env("GARLIC_TOKEN", "tok")
        .args(["status", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"accumulated_minutes\":123.0"));
    m.assert();
}

#[test]
fn sync_pushes_local_intervals_to_backend() {
    let server = MockServer::start();
    let m = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/intervals")
            .header("authorization", "Bearer tok");
        then.status(200)
            .body(intervals_response(90.0, "[]", "null"));
    });

    let tmp = seeded(State {
        date: current_date(2),
        accumulated_minutes: 45.0,
        ..State::default()
    });

    garlic(&gdir(&tmp))
        .env("GARLIC_URL", server.base_url())
        .env("GARLIC_TOKEN", "tok")
        .arg("sync")
        .assert()
        .success()
        .stdout(predicate::str::contains("synced"))
        .stdout(predicate::str::contains("1h 30m"));
    m.assert();
}

#[test]
fn sync_without_backend_reports_error() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().join(".garlic");
    std::fs::create_dir_all(&dir).unwrap();

    garlic(&dir)
        .arg("sync")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("no backend configured"));
}

#[test]
fn blocking_hook_prompt_nudges_from_backend() {
    let server = MockServer::start();
    let m = server.mock(|when, then| {
        when.method(POST).path("/v1/intervals");
        then.status(200)
            .body(intervals_response(60.0, "[60]", "60"));
    });

    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().join(".garlic");
    std::fs::create_dir_all(&dir).unwrap();

    // GARLIC_SYNC=blocking → the prompt hook flushes synchronously and nudges
    // from the merged (shared) total the backend reports.
    garlic(&dir)
        .env("GARLIC_URL", server.base_url())
        .env("GARLIC_TOKEN", "tok")
        .env("GARLIC_SYNC", "blocking")
        .args(["hook", "prompt"])
        .write_stdin("{}")
        .assert()
        .success()
        .stdout(predicate::str::contains("hookSpecificOutput"))
        .stdout(predicate::str::contains("UserPromptSubmit"));
    m.assert();
}

#[test]
fn status_falls_back_to_local_when_backend_unreachable() {
    // Local-first: an unreachable backend must not fail a status check — it
    // shows local time and notes the backend is unavailable.
    let tmp = seeded(State {
        date: current_date(2),
        accumulated_minutes: 45.0,
        ..State::default()
    });

    // Port 1 refuses connections immediately (no slow timeout).
    garlic(&gdir(&tmp))
        .env("GARLIC_URL", "http://127.0.0.1:1")
        .env("GARLIC_TOKEN", "tok")
        .args(["status", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"accumulated_minutes\":45.0"))
        .stderr(predicate::str::contains("shared backend unavailable"));
}

#[test]
fn offline_hook_prompt_still_tracks_locally() {
    // The motivating case: the backend is unreachable when the hook fires, but
    // time must still be recorded locally (to sync later). Default mode never
    // touches the network, so the hook succeeds and accumulates regardless.
    let tmp = seeded(State {
        date: current_date(2),
        accumulated_minutes: 0.0,
        open: vec![OpenCursor {
            session_id: "x".to_string(),
            kind: Kind::User,
            start: unix_now() - 120.0, // 2m of thinking within the gap cap
        }],
        ..State::default()
    });
    let dir = gdir(&tmp);

    garlic(&dir)
        .env("GARLIC_URL", "http://127.0.0.1:1")
        .env("GARLIC_TOKEN", "tok")
        .args(["hook", "prompt"])
        .write_stdin("{\"session_id\":\"x\"}")
        .assert()
        .success();

    // The 2-minute thinking gap was recorded into local state.
    let state = garlic::state::load_state(&Paths::with_dir(&dir), 2);
    assert!(
        state.accumulated_minutes > 1.0,
        "expected local tracking despite unreachable backend, got {}",
        state.accumulated_minutes
    );
}

#[test]
fn reset_offline_then_sync_reconciles_backend() {
    // Reset while the backend is unreachable: local zeroes, reset is queued.
    let tmp = seeded(State {
        date: current_date(2),
        accumulated_minutes: 95.0,
        ..State::default()
    });
    let dir = gdir(&tmp);

    garlic(&dir)
        .env("GARLIC_URL", "http://127.0.0.1:1")
        .env("GARLIC_TOKEN", "tok")
        .args(["reset", "-y"])
        .assert()
        .success()
        .stdout(predicate::str::contains("timer reset"));

    let state = garlic::state::load_state(&Paths::with_dir(&dir), 2);
    assert_eq!(state.accumulated_minutes, 0.0);
    assert!(
        state.reset_pending,
        "an offline reset should queue a backend reset"
    );

    // Backend reachable again: sync must zero it (/v1/reset) BEFORE pushing
    // (/v1/intervals), so the stale pre-reset intervals can't survive the merge.
    let server = MockServer::start();
    let reset_mock = server.mock(|when, then| {
        when.method(POST).path("/v1/reset");
        then.status(200).body(intervals_response(0.0, "[]", "null"));
    });
    let push_mock = server.mock(|when, then| {
        when.method(POST).path("/v1/intervals");
        then.status(200).body(intervals_response(0.0, "[]", "null"));
    });

    garlic(&dir)
        .env("GARLIC_URL", server.base_url())
        .env("GARLIC_TOKEN", "tok")
        .arg("sync")
        .assert()
        .success();
    reset_mock.assert();
    push_mock.assert();

    // The pending flag is cleared so the reset reconciles exactly once.
    let state = garlic::state::load_state(&Paths::with_dir(&dir), 2);
    assert!(
        !state.reset_pending,
        "reset_pending should clear after a successful sync"
    );
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
        .args(["status", "--week"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Weekly usage"));
    garlic(&dir)
        .args(["status", "--month"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Stats"));
}
