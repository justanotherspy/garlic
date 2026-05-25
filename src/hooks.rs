//! Hook subcommand handlers for Claude Code integration.
//!
//! Each handler takes an injected `now` (Unix seconds) and writes any
//! user-facing output to `out`, so they're deterministic and testable.

use std::io::{self, Write};

use chrono::NaiveDateTime;

use crate::config::{load_config, Config};
use crate::engine::{
    check_bedtime, handle_prompt, handle_session_end, handle_session_start, handle_stop,
};
use crate::format::format_duration;
use crate::nudges::{get_bedtime_nudge, get_nudge};
use crate::paths::Paths;
use crate::remote::Remote;
use crate::state::{load_state, save_state};

/// Build the nudge message (if any) for a prompt event from the inputs the
/// local engine and the remote backend both produce: the highest threshold
/// just crossed, whether we're in the bedtime window, and the ignore flag.
fn nudge_for(
    config: &Config,
    accumulated_minutes: f64,
    ignored: bool,
    crossed: Option<i64>,
    bedtime: bool,
) -> Option<String> {
    if ignored {
        return None;
    }
    if let Some(t) = crossed {
        let is_final = !config.nudge_thresholds_minutes.is_empty()
            && t == *config.nudge_thresholds_minutes.iter().max().unwrap();
        Some(get_nudge(
            &config.nudge_style,
            accumulated_minutes,
            is_final,
        ))
    } else if bedtime {
        Some(get_bedtime_nudge(accumulated_minutes))
    } else {
        None
    }
}

/// Emit a nudge as the `UserPromptSubmit` JSON envelope Claude Code expects.
fn write_nudge(out: &mut (impl Write + ?Sized), nudge: &str) -> io::Result<()> {
    let response = serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "UserPromptSubmit",
            "additionalContext": nudge,
        }
    });
    writeln!(out, "{}", serde_json::to_string(&response)?)
}

/// Print the session-start banner when there's accumulated time to report.
fn write_session_banner(out: &mut (impl Write + ?Sized), accumulated: f64) -> io::Result<()> {
    if accumulated > 0.0 {
        writeln!(
            out,
            "\u{1f9c4} {} of active coding today",
            format_duration(accumulated)
        )?;
    }
    Ok(())
}

/// Handle SessionStart hook: record start timestamp and show status.
pub fn hook_session_start(
    paths: &Paths,
    session_id: &str,
    now: f64,
    debug: bool,
    out: &mut (impl Write + ?Sized),
) -> io::Result<()> {
    let config = load_config(paths);
    let mut state = load_state(paths, config.reset_hour);
    if debug {
        eprintln!("[garlic debug] session-start ({session_id}): recording timestamp");
    }
    handle_session_start(&mut state, session_id, now);
    save_state(paths, &state)?;
    write_session_banner(out, state.accumulated_minutes)
}

/// Handle UserPromptSubmit hook: accumulate time, maybe nudge.
pub fn hook_prompt(
    paths: &Paths,
    session_id: &str,
    now: f64,
    now_local: NaiveDateTime,
    debug: bool,
    out: &mut (impl Write + ?Sized),
) -> io::Result<()> {
    let config = load_config(paths);
    let mut state = load_state(paths, config.reset_hour);
    let crossed = handle_prompt(&mut state, &config, session_id, now, debug);

    // Only consider the bedtime window when no threshold fired and nudging
    // isn't paused — check_bedtime sets a once-per-night flag as a side effect.
    let bedtime =
        crossed.is_none() && !state.ignored && check_bedtime(&mut state, &config, now_local);
    let nudge = nudge_for(
        &config,
        state.accumulated_minutes,
        state.ignored,
        crossed,
        bedtime,
    );

    save_state(paths, &state)?;

    if let Some(nudge) = nudge {
        write_nudge(out, &nudge)?;
    }
    Ok(())
}

/// Handle Stop hook: close the agent interval and open a user interval.
pub fn hook_stop(paths: &Paths, session_id: &str, now: f64, debug: bool) -> io::Result<()> {
    let config = load_config(paths);
    let mut state = load_state(paths, config.reset_hour);
    handle_stop(&mut state, &config, session_id, now, debug);
    save_state(paths, &state)
}

/// Handle SessionEnd hook: finalize the session's in-flight interval and drop
/// its cursor.
pub fn hook_session_end(paths: &Paths, session_id: &str, now: f64, debug: bool) -> io::Result<()> {
    let config = load_config(paths);
    let mut state = load_state(paths, config.reset_hour);
    handle_session_end(&mut state, &config, session_id, now, debug);
    save_state(paths, &state)
}

// --- Remote (shared-state backend) hook handlers ---------------------------
//
// When a backend is configured the server clock is authoritative, so these
// don't pass `now`. A backend error must never break a session: each handler
// degrades to a silent no-op (logging only under `--debug`).

/// Log a backend failure to stderr in debug mode; never propagates.
fn degrade(debug: bool, event: &str, err: &str) {
    if debug {
        eprintln!("[garlic debug] {event}: backend unavailable: {err}");
    }
}

pub fn hook_session_start_remote(
    remote: &Remote,
    paths: &Paths,
    session_id: &str,
    debug: bool,
    out: &mut (impl Write + ?Sized),
) -> io::Result<()> {
    let config = load_config(paths);
    match remote.session_start(&config, session_id) {
        Ok(resp) => write_session_banner(out, resp.state.accumulated_minutes),
        Err(e) => {
            degrade(debug, "session-start", &e);
            Ok(())
        }
    }
}

pub fn hook_prompt_remote(
    remote: &Remote,
    paths: &Paths,
    session_id: &str,
    debug: bool,
    out: &mut (impl Write + ?Sized),
) -> io::Result<()> {
    let config = load_config(paths);
    match remote.prompt(&config, session_id) {
        Ok(resp) => {
            if let Some(nudge) = nudge_for(
                &config,
                resp.state.accumulated_minutes,
                resp.state.ignored,
                resp.crossed_threshold,
                resp.bedtime,
            ) {
                write_nudge(out, &nudge)?;
            }
            Ok(())
        }
        Err(e) => {
            degrade(debug, "prompt", &e);
            Ok(())
        }
    }
}

pub fn hook_stop_remote(
    remote: &Remote,
    paths: &Paths,
    session_id: &str,
    debug: bool,
) -> io::Result<()> {
    let config = load_config(paths);
    if let Err(e) = remote.stop(&config, session_id) {
        degrade(debug, "stop", &e);
    }
    Ok(())
}

pub fn hook_session_end_remote(
    remote: &Remote,
    paths: &Paths,
    session_id: &str,
    debug: bool,
) -> io::Result<()> {
    let config = load_config(paths);
    if let Err(e) = remote.session_end(&config, session_id) {
        degrade(debug, "session-end", &e);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intervals::{Kind, OpenCursor};
    use crate::state::{current_date, load_state, State};
    use chrono::NaiveDate;
    use tempfile::TempDir;

    const NOW: f64 = 1710567900.0;

    fn today() -> String {
        current_date(2)
    }

    fn setup(state: &State) -> (TempDir, Paths) {
        let tmp = TempDir::new().unwrap();
        let paths = Paths::with_dir(tmp.path().join(".garlic"));
        std::fs::create_dir_all(paths.dir()).unwrap();
        std::fs::write(
            paths.config(),
            "max_prompt_gap_minutes = 10\n\
             max_generation_minutes = 120\n\
             reset_hour = 2\n\
             nudge_thresholds_minutes = [60, 120]\n\
             nudge_style = \"gentle\"\n",
        )
        .unwrap();
        save_state(&paths, state).unwrap();
        (tmp, paths)
    }

    fn base_state() -> State {
        State {
            date: today(),
            ..State::default()
        }
    }

    fn local(h: u32, min: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 3, 16)
            .unwrap()
            .and_hms_opt(h, min, 0)
            .unwrap()
    }

    /// A state with one open cursor for session `sid`.
    fn with_cursor(accumulated: f64, sid: &str, kind: Kind, start: f64) -> State {
        State {
            accumulated_minutes: accumulated,
            open: vec![OpenCursor {
                session_id: sid.to_string(),
                kind,
                start,
            }],
            ..base_state()
        }
    }

    #[test]
    fn session_start_opens_user_cursor() {
        let (_tmp, paths) = setup(&base_state());
        let mut out = Vec::new();
        hook_session_start(&paths, "s", NOW, false, &mut out).unwrap();
        let state = load_state(&paths, 2);
        assert_eq!(state.open.len(), 1);
        assert_eq!(state.open[0].session_id, "s");
        assert_eq!(state.open[0].kind, Kind::User);
    }

    #[test]
    fn session_start_shows_status_with_accumulated_time() {
        let mut s = base_state();
        s.accumulated_minutes = 95.0;
        let (_tmp, paths) = setup(&s);
        let mut out = Vec::new();
        hook_session_start(&paths, "s", NOW, false, &mut out).unwrap();
        assert!(String::from_utf8(out).unwrap().contains("1h 35m"));
    }

    #[test]
    fn session_start_silent_when_no_time() {
        let (_tmp, paths) = setup(&base_state());
        let mut out = Vec::new();
        hook_session_start(&paths, "s", NOW, false, &mut out).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn stop_closes_agent_interval() {
        // 30m already tracked, an agent interval running for 2m.
        let s = with_cursor(30.0, "x", Kind::Agent, NOW - 120.0);
        let (_tmp, paths) = setup(&s);
        hook_stop(&paths, "x", NOW, false).unwrap();
        let state = load_state(&paths, 2);
        assert!((state.accumulated_minutes - 32.0).abs() < 0.01);
        // A user-thinking cursor is now open for the same session.
        assert!(state
            .open
            .iter()
            .any(|c| c.session_id == "x" && c.kind == Kind::User));
    }

    #[test]
    fn session_end_finalizes_and_drops_cursor() {
        let s = with_cursor(30.0, "x", Kind::Agent, NOW - 120.0);
        let (_tmp, paths) = setup(&s);
        hook_session_end(&paths, "x", NOW, false).unwrap();
        let state = load_state(&paths, 2);
        assert!((state.accumulated_minutes - 32.0).abs() < 0.01);
        assert!(state.open.iter().all(|c| c.session_id != "x"));
    }

    #[test]
    fn prompt_no_nudge_below_threshold() {
        let s = with_cursor(0.0, "x", Kind::User, NOW - 300.0);
        let (_tmp, paths) = setup(&s);
        let mut out = Vec::new();
        hook_prompt(&paths, "x", NOW, local(10, 0), false, &mut out).unwrap();
        assert!(out.is_empty());
        let state = load_state(&paths, 2);
        assert!(state.accumulated_minutes > 0.0);
    }

    #[test]
    fn prompt_with_nudge_crosses_threshold() {
        // 58m tracked + a 5m thinking gap → crosses the 60m threshold.
        let s = with_cursor(58.0, "x", Kind::User, NOW - 300.0);
        let (_tmp, paths) = setup(&s);
        let mut out = Vec::new();
        hook_prompt(&paths, "x", NOW, local(10, 0), false, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        let response: serde_json::Value = serde_json::from_str(text.trim()).unwrap();
        assert_eq!(
            response["hookSpecificOutput"]["hookEventName"],
            "UserPromptSubmit"
        );
        assert!(!response["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap()
            .is_empty());
        let state = load_state(&paths, 2);
        assert!(state.nudges_given.contains(&60));
    }

    #[test]
    fn prompt_ignored_no_nudge() {
        let mut s = with_cursor(58.0, "x", Kind::User, NOW - 300.0);
        s.ignored = true;
        let (_tmp, paths) = setup(&s);
        let mut out = Vec::new();
        hook_prompt(&paths, "x", NOW, local(10, 0), false, &mut out).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn prompt_bedtime_nudge() {
        let s = with_cursor(45.0, "x", Kind::User, NOW - 60.0);
        let (_tmp, paths) = setup(&s);
        let mut out = Vec::new();
        hook_prompt(&paths, "x", NOW, local(1, 30), false, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        let response: serde_json::Value = serde_json::from_str(text.trim()).unwrap();
        assert_eq!(
            response["hookSpecificOutput"]["hookEventName"],
            "UserPromptSubmit"
        );
        let state = load_state(&paths, 2);
        assert!(state.bedtime_nudge_given);
    }
}
