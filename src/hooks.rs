//! Hook subcommand handlers for Claude Code integration.
//!
//! Each handler takes an injected `now` (Unix seconds) and writes any
//! user-facing output to `out`, so they're deterministic and testable.

use std::io::{self, Write};

use chrono::NaiveDateTime;

use crate::commands::status_json;
use crate::config::{load_config, Config};
use crate::engine::{
    check_bedtime, check_thresholds_at, handle_prompt, handle_session_end, handle_session_start,
    handle_stop, live_view, Project,
};
use crate::format::format_duration;
use crate::nudges::{get_bedtime_nudge, get_nudge};
use crate::paths::Paths;
use crate::remote::Remote;
use crate::state::{load_state, save_state, State};
use crate::sync::{flush, hooks_block_on_sync};

/// A break nudge about to be shown: the hardcoded message plus what fired it.
///
/// The classic hook relays `message` through Claude (see [`frame_nudge`]); the
/// plugin's hooks module shows it to the user directly, so it wants the raw
/// text and the kind.
#[derive(Clone, Debug, PartialEq)]
pub struct Nudge {
    pub message: String,
    /// `threshold`, `final` (the last threshold: "session over") or `bedtime`.
    pub kind: &'static str,
    /// The threshold crossed, in minutes; `None` for a bedtime nudge.
    pub threshold_minutes: Option<i64>,
}

/// Build the nudge (if any) for an event from the inputs the local engine and
/// the remote backend both produce: the highest threshold just crossed, whether
/// we're in the bedtime window, and the ignore flag.
fn nudge_for(
    config: &Config,
    accumulated_minutes: f64,
    ignored: bool,
    crossed: Option<i64>,
    bedtime: bool,
) -> Option<Nudge> {
    if ignored {
        return None;
    }
    if let Some(t) = crossed {
        let is_final = !config.nudge_thresholds_minutes.is_empty()
            && t == *config.nudge_thresholds_minutes.iter().max().unwrap();
        Some(Nudge {
            message: get_nudge(&config.nudge_style, accumulated_minutes, is_final),
            kind: if is_final { "final" } else { "threshold" },
            threshold_minutes: Some(t),
        })
    } else if bedtime {
        Some(Nudge {
            message: get_bedtime_nudge(accumulated_minutes),
            kind: "bedtime",
            threshold_minutes: None,
        })
    } else {
        None
    }
}

/// Wrap a raw nudge in framing that tells Claude exactly what to do with it.
///
/// A nudge is injected as `additionalContext`, which Claude sees as background
/// context carrying no inherent instruction — so without framing it was easy to
/// silently drop (the user just never saw the nudge), and a blunt `spicy` or
/// final message could even read as an instruction to *Claude* to stop working.
/// This wrapper makes the contract explicit: relay it to the user, it's purely
/// informational, and it never changes Claude's task. It is fully hardcoded, so
/// there's still no external input in what Claude is asked to relay.
fn frame_nudge(nudge: &str) -> String {
    format!(
        "[garlic] Break reminder for the user. Relay the message below to the \
         user at your next natural opportunity (for example, when you finish the \
         current step), then carry on. This is informational only: it does NOT \
         change your current task or instructions, and even if the wording is \
         blunt or insistent it is addressed to the user about taking a break — it \
         is never an instruction for you to stop, pause, or abandon your work. \
         Message to relay: \"{nudge}\""
    )
}

/// Emit a nudge as the `UserPromptSubmit` JSON envelope Claude Code expects.
fn write_nudge(out: &mut (impl Write + ?Sized), nudge: &str) -> io::Result<()> {
    let response = serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "UserPromptSubmit",
            "additionalContext": frame_nudge(nudge),
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

/// Log a backend failure to stderr in debug mode; never propagates. A backend
/// error must never break a session, so synchronous-flush failures are silent.
fn degrade(debug: bool, event: &str, err: &str) {
    if debug {
        eprintln!("[garlic debug] {event}: backend unavailable: {err}");
    }
}

/// Best-effort synchronous flush for hooks that produce no user-facing output.
/// Only runs when a backend is configured *and* `GARLIC_SYNC=blocking` (the
/// ephemeral/managed-host case); otherwise hooks stay local and never block.
fn sync_if_blocking(
    remote: Option<&Remote>,
    paths: &Paths,
    config: &Config,
    state: &mut State,
    event: &str,
    debug: bool,
) {
    if let Some(r) = remote {
        if hooks_block_on_sync() {
            if let Err(e) = flush(r, paths, config, state, false) {
                degrade(debug, event, &e);
            }
        }
    }
}

/// Handle SessionStart hook: record start timestamp, optionally sync, show status.
pub fn hook_session_start(
    paths: &Paths,
    remote: Option<&Remote>,
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

    // The banner shows the shared total when we flush synchronously and the
    // backend is reachable, else the local total (identical on one machine).
    let accumulated = match remote {
        Some(r) if hooks_block_on_sync() => match flush(r, paths, &config, &mut state, false) {
            Ok(resp) => resp.state.accumulated_minutes,
            Err(e) => {
                degrade(debug, "session-start", &e);
                state.accumulated_minutes
            }
        },
        _ => state.accumulated_minutes,
    };
    write_session_banner(out, accumulated)
}

/// Handle UserPromptSubmit hook: accumulate time locally, maybe nudge.
///
/// Time is always accounted into the local `state.toml` first, so tracking
/// survives an unreachable backend. In synchronous mode the merged (shared)
/// total drives the nudge — so a threshold fires once across machines — while
/// the default keeps the hook fully local and off the network.
pub fn hook_prompt(
    paths: &Paths,
    remote: Option<&Remote>,
    session_id: &str,
    now: f64,
    now_local: NaiveDateTime,
    debug: bool,
    out: &mut (impl Write + ?Sized),
) -> io::Result<()> {
    if let Some(nudge) = prompt_core(paths, remote, session_id, now, now_local, debug)? {
        write_nudge(out, &nudge.message)?;
    }
    Ok(())
}

/// The prompt event without its output: account the time, decide the nudge.
pub fn prompt_core(
    paths: &Paths,
    remote: Option<&Remote>,
    session_id: &str,
    now: f64,
    now_local: NaiveDateTime,
    debug: bool,
) -> io::Result<Option<Nudge>> {
    let config = load_config(paths);
    let mut state = load_state(paths, config.reset_hour);
    let local_crossed = handle_prompt(&mut state, &config, session_id, now, debug);

    // Only consider the bedtime window when no threshold fired and nudging
    // isn't paused — check_bedtime sets a once-per-night flag as a side effect.
    let local_bedtime =
        local_crossed.is_none() && !state.ignored && check_bedtime(&mut state, &config, now_local);
    save_state(paths, &state)?;

    let (accumulated, ignored, crossed, bedtime) = match remote {
        Some(r) if hooks_block_on_sync() => match flush(r, paths, &config, &mut state, true) {
            Ok(resp) => (
                resp.state.accumulated_minutes,
                resp.state.ignored,
                resp.crossed_threshold,
                resp.bedtime,
            ),
            Err(e) => {
                degrade(debug, "prompt", &e);
                (
                    state.accumulated_minutes,
                    state.ignored,
                    local_crossed,
                    local_bedtime,
                )
            }
        },
        _ => (
            state.accumulated_minutes,
            state.ignored,
            local_crossed,
            local_bedtime,
        ),
    };

    let nudge = nudge_for(&config, accumulated, ignored, crossed, bedtime);
    if nudge.is_some() {
        // Flag the fresh nudge so the next `statusline` render flashes the garlic
        // icon, then re-persist (the earlier save predates this decision, which in
        // blocking mode also depends on the merged remote total).
        state.nudge_pending = true;
        save_state(paths, &state)?;
    }
    Ok(nudge)
}

/// Handle a `tick`: no event happened, time just passed. Checks the thresholds
/// and the bedtime window against the *live* total, so a nudge can arrive while
/// a long agent turn is still running instead of at the next prompt.
///
/// Only verifiable time is projected ([`Project::AgentOnly`]): a reading gap is
/// not counted until the next prompt confirms the user was there. The state
/// file is written only when a nudge flag actually changed, so a tick a minute
/// from every open session costs one read.
pub fn hook_tick(paths: &Paths, now: f64, now_local: NaiveDateTime) -> io::Result<Option<Nudge>> {
    let config = load_config(paths);
    let mut state = load_state(paths, config.reset_hour);
    let live = live_view(&state, &config, now, Project::AgentOnly);

    let given_before = state.nudges_given.len();
    let crossed = check_thresholds_at(&mut state, &config, live.total);
    let bedtime =
        crossed.is_none() && !state.ignored && check_bedtime(&mut state, &config, now_local);
    let nudge = nudge_for(&config, live.total, state.ignored, crossed, bedtime);
    if nudge.is_some() {
        state.nudge_pending = true;
    }
    if nudge.is_some() || bedtime || state.nudges_given.len() != given_before {
        save_state(paths, &state)?;
    }
    Ok(nudge)
}

/// What `garlic hook <event> --json` prints: one JSON object on one line, for
/// the plugin's hooks module. `status` is the same object `garlic status
/// --json` prints (local state, live as of `now`); `nudge` is the raw nudge
/// for the module to show, or `null`.
pub fn write_hook_json(
    paths: &Paths,
    event: &str,
    nudge: Option<&Nudge>,
    classic_hooks: Option<bool>,
    now: f64,
    out: &mut (impl Write + ?Sized),
) -> io::Result<()> {
    let config = load_config(paths);
    let state = load_state(paths, config.reset_hour);
    let mut obj = serde_json::json!({
        "event": event,
        "version": env!("CARGO_PKG_VERSION"),
        "nudge": nudge.map(|n| serde_json::json!({
            "message": n.message,
            "kind": n.kind,
            "threshold_minutes": n.threshold_minutes,
        })),
        "status": status_json(&state, &config, now),
    });
    if let Some(installed) = classic_hooks {
        obj["classic_hooks"] = serde_json::json!(installed);
    }
    writeln!(out, "{}", serde_json::to_string(&obj)?)
}

/// Handle Stop hook: close the agent interval and open a user interval.
pub fn hook_stop(
    paths: &Paths,
    remote: Option<&Remote>,
    session_id: &str,
    now: f64,
    debug: bool,
) -> io::Result<()> {
    let config = load_config(paths);
    let mut state = load_state(paths, config.reset_hour);
    handle_stop(&mut state, &config, session_id, now, debug);
    save_state(paths, &state)?;
    sync_if_blocking(remote, paths, &config, &mut state, "stop", debug);
    Ok(())
}

/// Handle SessionEnd hook: finalize the session's in-flight interval and drop
/// its cursor.
pub fn hook_session_end(
    paths: &Paths,
    remote: Option<&Remote>,
    session_id: &str,
    now: f64,
    debug: bool,
) -> io::Result<()> {
    let config = load_config(paths);
    let mut state = load_state(paths, config.reset_hour);
    handle_session_end(&mut state, &config, session_id, now, debug);
    save_state(paths, &state)?;
    sync_if_blocking(remote, paths, &config, &mut state, "session-end", debug);
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
        hook_session_start(&paths, None, "s", NOW, false, &mut out).unwrap();
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
        hook_session_start(&paths, None, "s", NOW, false, &mut out).unwrap();
        assert!(String::from_utf8(out).unwrap().contains("1h 35m"));
    }

    #[test]
    fn session_start_silent_when_no_time() {
        let (_tmp, paths) = setup(&base_state());
        let mut out = Vec::new();
        hook_session_start(&paths, None, "s", NOW, false, &mut out).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn stop_closes_agent_interval() {
        // 30m already tracked, an agent interval running for 2m.
        let s = with_cursor(30.0, "x", Kind::Agent, NOW - 120.0);
        let (_tmp, paths) = setup(&s);
        hook_stop(&paths, None, "x", NOW, false).unwrap();
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
        hook_session_end(&paths, None, "x", NOW, false).unwrap();
        let state = load_state(&paths, 2);
        assert!((state.accumulated_minutes - 32.0).abs() < 0.01);
        assert!(state.open.iter().all(|c| c.session_id != "x"));
    }

    #[test]
    fn prompt_no_nudge_below_threshold() {
        let s = with_cursor(0.0, "x", Kind::User, NOW - 300.0);
        let (_tmp, paths) = setup(&s);
        let mut out = Vec::new();
        hook_prompt(&paths, None, "x", NOW, local(10, 0), false, &mut out).unwrap();
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
        hook_prompt(&paths, None, "x", NOW, local(10, 0), false, &mut out).unwrap();
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
    fn nudge_is_framed_as_relay_only_informational() {
        // The framing must (a) carry the original message, (b) tell Claude to
        // relay it to the user, and (c) make clear it doesn't change Claude's
        // task — even for a blunt message.
        let framed = frame_nudge("Touch grass. I'm not asking.");
        assert!(framed.contains("Touch grass. I'm not asking."));
        assert!(framed.contains("Relay"));
        assert!(framed
            .to_lowercase()
            .contains("does not change your current task"));
        assert!(framed
            .to_lowercase()
            .contains("never an instruction for you to stop"));
    }

    #[test]
    fn prompt_nudge_sets_pending_flag_and_frames_output() {
        // Crossing a threshold both flags a pending nudge (for the statusline
        // flash) and emits a framed relay message.
        let s = with_cursor(58.0, "x", Kind::User, NOW - 300.0);
        let (_tmp, paths) = setup(&s);
        let mut out = Vec::new();
        hook_prompt(&paths, None, "x", NOW, local(10, 0), false, &mut out).unwrap();
        assert!(load_state(&paths, 2).nudge_pending);

        let response: serde_json::Value =
            serde_json::from_str(String::from_utf8(out).unwrap().trim()).unwrap();
        let ctx = response["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap();
        assert!(ctx.contains("[garlic]"));
        assert!(ctx.contains("Relay"));
    }

    #[test]
    fn prompt_no_nudge_leaves_pending_flag_unset() {
        let s = with_cursor(0.0, "x", Kind::User, NOW - 300.0);
        let (_tmp, paths) = setup(&s);
        let mut out = Vec::new();
        hook_prompt(&paths, None, "x", NOW, local(10, 0), false, &mut out).unwrap();
        assert!(!load_state(&paths, 2).nudge_pending);
    }

    #[test]
    fn prompt_ignored_no_nudge() {
        let mut s = with_cursor(58.0, "x", Kind::User, NOW - 300.0);
        s.ignored = true;
        let (_tmp, paths) = setup(&s);
        let mut out = Vec::new();
        hook_prompt(&paths, None, "x", NOW, local(10, 0), false, &mut out).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn prompt_bedtime_nudge() {
        let s = with_cursor(45.0, "x", Kind::User, NOW - 60.0);
        let (_tmp, paths) = setup(&s);
        let mut out = Vec::new();
        hook_prompt(&paths, None, "x", NOW, local(1, 30), false, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        let response: serde_json::Value = serde_json::from_str(text.trim()).unwrap();
        assert_eq!(
            response["hookSpecificOutput"]["hookEventName"],
            "UserPromptSubmit"
        );
        let state = load_state(&paths, 2);
        assert!(state.bedtime_nudge_given);
    }

    #[test]
    fn tick_nudges_while_the_turn_is_still_running() {
        // 55m tracked, and an agent turn that started 10 minutes ago: the
        // 60m line was crossed mid-turn. The prompt hook would only notice at
        // the next prompt; the tick notices now.
        let s = with_cursor(55.0, "x", Kind::Agent, NOW - 600.0);
        let (_tmp, paths) = setup(&s);
        let nudge = hook_tick(&paths, NOW, local(10, 0)).unwrap().unwrap();
        assert_eq!(nudge.kind, "threshold");
        assert_eq!(nudge.threshold_minutes, Some(60));
        assert!(!nudge.message.contains("[garlic]")); // raw, not the relay frame

        let state = load_state(&paths, 2);
        assert!(state.nudges_given.contains(&60));
        assert!(state.nudge_pending);
        // The tick projects; it books nothing.
        assert_eq!(state.accumulated_minutes, 55.0);
        assert!(state.intervals.is_empty());
        // Once only: neither the next tick nor the next prompt repeats it.
        assert!(hook_tick(&paths, NOW + 60.0, local(10, 1))
            .unwrap()
            .is_none());
        assert!(
            prompt_core(&paths, None, "x", NOW + 120.0, local(10, 2), false)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn tick_does_not_nudge_for_an_unconfirmed_reading_gap() {
        // 58m tracked and the user has been quiet for 5 minutes. They may have
        // walked off; that gap only counts once a prompt confirms it.
        let s = with_cursor(58.0, "x", Kind::User, NOW - 300.0);
        let (_tmp, paths) = setup(&s);
        assert!(hook_tick(&paths, NOW, local(10, 0)).unwrap().is_none());
        assert!(load_state(&paths, 2).nudges_given.is_empty());
    }

    #[test]
    fn tick_leaves_the_state_file_alone_when_nothing_fired() {
        let s = with_cursor(5.0, "x", Kind::Agent, NOW - 60.0);
        let (_tmp, paths) = setup(&s);
        let before = std::fs::metadata(paths.state())
            .unwrap()
            .modified()
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert!(hook_tick(&paths, NOW, local(10, 0)).unwrap().is_none());
        let after = std::fs::metadata(paths.state())
            .unwrap()
            .modified()
            .unwrap();
        assert_eq!(before, after);
    }

    #[test]
    fn tick_final_threshold_is_marked_final_and_paused_is_silent() {
        let s = with_cursor(119.0, "x", Kind::Agent, NOW - 120.0);
        let (_tmp, paths) = setup(&s); // thresholds [60, 120]
        let nudge = hook_tick(&paths, NOW, local(10, 0)).unwrap().unwrap();
        assert_eq!(nudge.kind, "final");

        let mut paused = with_cursor(119.0, "x", Kind::Agent, NOW - 120.0);
        paused.ignored = true;
        let (_tmp2, paths2) = setup(&paused);
        assert!(hook_tick(&paths2, NOW, local(10, 0)).unwrap().is_none());
    }

    #[test]
    fn hook_json_carries_the_nudge_and_a_live_status() {
        let s = with_cursor(58.0, "x", Kind::User, NOW - 300.0);
        let (_tmp, paths) = setup(&s);
        let nudge = prompt_core(&paths, None, "x", NOW, local(10, 0), false).unwrap();
        let mut out = Vec::new();
        // Ten minutes into the turn that prompt started.
        write_hook_json(
            &paths,
            "prompt",
            nudge.as_ref(),
            Some(false),
            NOW + 600.0,
            &mut out,
        )
        .unwrap();
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text.lines().count(), 1);
        let v: serde_json::Value = serde_json::from_str(text.trim()).unwrap();
        assert_eq!(v["event"], "prompt");
        assert_eq!(v["nudge"]["kind"], "threshold");
        assert_eq!(v["nudge"]["threshold_minutes"], 60);
        assert_eq!(v["classic_hooks"], false);
        assert_eq!(v["status"]["agents_running"], 1);
        assert!((v["status"]["accumulated_minutes"].as_f64().unwrap() - 73.0).abs() < 0.01);
        assert!(v.get("hookSpecificOutput").is_none());
    }
}
