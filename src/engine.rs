//! Time gap calculation, accumulation, and threshold checking.
//!
//! All functions take an injected `now` so they stay pure and deterministic
//! under test. `now` is a Unix timestamp in seconds (matching `time.time()`);
//! `now_local` is the wall-clock time used only for the bedtime window.

use chrono::{NaiveDateTime, Timelike};

use crate::config::Config;
use crate::intervals::{breakdown, push_interval, Interval, Kind, OpenCursor};
use crate::state::State;

/// Remove and return the open cursor for `session_id`, if one exists.
fn take_cursor(state: &mut State, session_id: &str) -> Option<OpenCursor> {
    state
        .open
        .iter()
        .position(|c| c.session_id == session_id)
        .map(|pos| state.open.remove(pos))
}

/// Open a fresh cursor for `session_id`, replacing any existing one.
fn open_cursor(state: &mut State, session_id: &str, kind: Kind, now: f64) {
    state.open.retain(|c| c.session_id != session_id);
    state.open.push(OpenCursor {
        session_id: session_id.to_string(),
        kind,
        start: now,
    });
}

/// Close `session_id`'s open cursor into a finished interval, applying the rule
/// for the cursor's own kind: user gaps over `max_prompt_gap_minutes` are
/// dropped entirely; agent generation is clamped to `max_generation_minutes`.
/// Closing by stored kind (not by which event fired) keeps interleaved and
/// missing-event sequences correct.
fn close_cursor(state: &mut State, config: &Config, session_id: &str, now: f64) {
    let Some(cursor) = take_cursor(state, session_id) else {
        return;
    };
    let end = match cursor.kind {
        Kind::User => {
            let raw_gap = (now - cursor.start) / 60.0;
            if raw_gap > config.max_prompt_gap_minutes as f64 {
                // Stepped away — count nothing for this gap, but still seed the
                // migrated baseline so a pre-upgrade total isn't wiped by the
                // following recompute when this is the first interval.
                seed_baseline(state, cursor.start);
                return;
            }
            now
        }
        Kind::Agent => {
            let cap_secs = config.max_generation_minutes as f64 * 60.0;
            cursor.start + (now - cursor.start).min(cap_secs)
        }
    };
    seed_baseline(state, cursor.start);
    push_interval(
        &mut state.intervals,
        Interval {
            session_id: cursor.session_id,
            kind: cursor.kind,
            start: cursor.start,
            end,
        },
    );
}

/// On the first interval recorded after an upgrade, preserve the day's existing
/// `accumulated_minutes` (tracked by the old scalar model) as one synthetic
/// agent interval ending at `anchor`, so the migrated total isn't lost.
fn seed_baseline(state: &mut State, anchor: f64) {
    if state.intervals.is_empty() && state.accumulated_minutes > 0.0 {
        let start = anchor - state.accumulated_minutes * 60.0;
        state.intervals.push(Interval {
            session_id: "migrated".to_string(),
            kind: Kind::Agent,
            start,
            end: anchor,
        });
    }
}

/// Which in-flight cursors a live view projects up to `now`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Project {
    /// Agent generation only: an agent is verifiably running. Used for nudges,
    /// so nobody is nudged for a reading gap that may yet turn out to be a
    /// step away (and count for nothing).
    AgentOnly,
    /// Agent generation and user gaps still inside `max_prompt_gap_minutes`:
    /// what the day will read as if the next prompt arrived right now.
    All,
}

/// The day's figures as of `now`, in-flight intervals included.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LiveView {
    /// Union total (`accumulated_minutes` plus whatever is in flight).
    pub total: f64,
    /// Union of agent generation time.
    pub agent: f64,
    /// Union of user reading/thinking/typing time.
    pub user: f64,
    /// Wall-clock time with two or more sessions active.
    pub overlap: f64,
    /// Sessions with an agent generating right now.
    pub agents_running: usize,
}

/// Project the open cursors to `now` without mutating anything. A user gap
/// past `max_prompt_gap_minutes` contributes nothing (the user stepped away),
/// as in [`close_cursor`]. An agent cursor past `max_generation_minutes`
/// contributes nothing either: no stop ever arrived, so it is most likely a
/// session that crashed mid-turn, and projecting it would pin up to the whole
/// cap onto the day's readout for a turn that is not running. (If it really is
/// a turn that long, its stop still books the clamped interval.)
fn projected(state: &State, config: &Config, now: f64, which: Project) -> Vec<Interval> {
    let mut all = state.intervals.clone();
    for cursor in &state.open {
        let end = match cursor.kind {
            Kind::Agent => {
                if !is_running(cursor, config, now) {
                    continue;
                }
                now
            }
            Kind::User => {
                let gap = (now - cursor.start) / 60.0;
                if which == Project::AgentOnly || gap > config.max_prompt_gap_minutes as f64 {
                    continue;
                }
                now
            }
        };
        if end > cursor.start {
            all.push(Interval {
                session_id: cursor.session_id.clone(),
                kind: cursor.kind,
                start: cursor.start,
                end,
            });
        }
    }
    all
}

/// Whether `cursor` is an agent generation still inside its cap at `now`.
fn is_running(cursor: &OpenCursor, config: &Config, now: f64) -> bool {
    cursor.kind == Kind::Agent && now - cursor.start <= config.max_generation_minutes as f64 * 60.0
}

/// The day's totals as of `now`, counting in-flight intervals.
///
/// The persisted figures only move when an event closes an interval, so during
/// a long agent turn they lag by the whole turn. This derives the current
/// figures from the closed intervals plus the open cursors, deduplicated by the
/// same sweep-line. The total is expressed as a delta over the persisted
/// `accumulated_minutes` so a migrated (pre-interval) or backend-merged total
/// is carried through unchanged.
pub fn live_view(state: &State, config: &Config, now: f64, which: Project) -> LiveView {
    let closed = breakdown(&state.intervals);
    let live = breakdown(&projected(state, config, now, which));
    LiveView {
        total: state.accumulated_minutes + (live.total - closed.total).max(0.0),
        agent: live.agent,
        user: live.user,
        overlap: live.overlap,
        agents_running: state
            .open
            .iter()
            .filter(|c| is_running(c, config, now))
            .count(),
    }
}

/// Recompute the persisted daily total as the union of all intervals.
///
/// When there are no intervals yet, leave `accumulated_minutes` untouched: an
/// event that closed no cursor records no interval, and a migrated pre-interval
/// baseline (`accumulated_minutes > 0` with an empty `intervals`) must not be
/// zeroed here — it's seeded into a synthetic interval the first time a real
/// interval is recorded. Once any interval exists the union is the truth.
fn recompute_total(state: &mut State) {
    if state.intervals.is_empty() {
        return;
    }
    state.accumulated_minutes = breakdown(&state.intervals).total;
}

/// Process a prompt event for `session_id`: close the user-thinking interval,
/// start an agent interval, and check thresholds against the union total.
///
/// Returns the threshold (in minutes) that was just crossed, or `None`.
pub fn handle_prompt(
    state: &mut State,
    config: &Config,
    session_id: &str,
    now: f64,
    debug: bool,
) -> Option<i64> {
    close_cursor(state, config, session_id, now);
    open_cursor(state, session_id, Kind::Agent, now);
    recompute_total(state);
    if debug {
        eprintln!(
            "[garlic debug] prompt ({session_id}) | total: {:.2}m",
            state.accumulated_minutes
        );
    }
    check_thresholds(state, config)
}

/// Process a stop event for `session_id`: close the agent interval (clamped to
/// `max_generation_minutes`) and start a user-thinking interval.
pub fn handle_stop(state: &mut State, config: &Config, session_id: &str, now: f64, debug: bool) {
    close_cursor(state, config, session_id, now);
    open_cursor(state, session_id, Kind::User, now);
    recompute_total(state);
    if debug {
        eprintln!(
            "[garlic debug] stop ({session_id}) | total: {:.2}m",
            state.accumulated_minutes
        );
    }
}

/// Process a session-start event for `session_id`: open a user-thinking cursor
/// so the time before the first prompt counts as engagement (as before).
pub fn handle_session_start(state: &mut State, session_id: &str, now: f64) {
    open_cursor(state, session_id, Kind::User, now);
}

/// Process a session-end event for `session_id`: finalize the in-flight
/// interval (clamped) and drop the session's cursor, so a killed session can't
/// leak time into the next one. Per-session, so other live sessions are
/// untouched.
pub fn handle_session_end(
    state: &mut State,
    config: &Config,
    session_id: &str,
    now: f64,
    debug: bool,
) {
    close_cursor(state, config, session_id, now);
    state.open.retain(|c| c.session_id != session_id);
    recompute_total(state);
    if debug {
        eprintln!(
            "[garlic debug] session-end ({session_id}) | total: {:.2}m",
            state.accumulated_minutes
        );
    }
}

/// Return the highest newly-crossed threshold, or `None`.
///
/// When a single event jumps the total past several thresholds at once (an
/// agent generation can add up to `max_generation_minutes`, only counted at the
/// next prompt), *all* of those thresholds are marked given — not just the
/// highest. Marking only the highest would leave the lower ones to re-fire one
/// at a time on later prompts, nagging on every prompt and emitting a less
/// severe message after the final "session over" one. We still return the
/// highest so the nudge reflects how far past you are.
fn check_thresholds(state: &mut State, config: &Config) -> Option<i64> {
    let accumulated = state.accumulated_minutes;
    check_thresholds_at(state, config, accumulated)
}

/// [`check_thresholds`] against an explicit total, so a caller can test a
/// *live* figure (see [`live_view`]) without persisting it as the day's total.
pub fn check_thresholds_at(state: &mut State, config: &Config, accumulated: f64) -> Option<i64> {
    let mut thresholds = config.nudge_thresholds_minutes.clone();
    thresholds.sort_unstable();

    let mut highest = None;
    for &threshold in thresholds.iter() {
        if accumulated >= threshold as f64 && !state.nudges_given.contains(&threshold) {
            state.nudges_given.push(threshold);
            highest = Some(threshold);
        }
    }

    highest
}

/// Return `true` if we're in the bedtime window and haven't nudged yet.
///
/// The bedtime window is the hour before `reset_hour` (e.g. 1 AM if reset is
/// 2 AM).
pub fn check_bedtime(state: &mut State, config: &Config, now_local: NaiveDateTime) -> bool {
    if state.bedtime_nudge_given {
        return false;
    }

    let reset_hour = config.reset_hour;
    let bedtime_hour = (reset_hour - 1).rem_euclid(24);

    if now_local.hour() as i64 == bedtime_hour {
        state.bedtime_nudge_given = true;
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    const NOW: f64 = 1710567900.0;

    fn config() -> Config {
        Config {
            max_prompt_gap_minutes: 10,
            max_generation_minutes: 120,
            reset_hour: 2,
            nudge_thresholds_minutes: vec![60, 120, 180, 240],
            nudge_style: "gentle".to_string(),
        }
    }

    fn state() -> State {
        State {
            date: "2026-03-16".to_string(),
            ..State::default()
        }
    }

    fn at(base: f64, minutes: f64) -> f64 {
        base + minutes * 60.0
    }

    #[test]
    fn prompt_then_stop_splits_user_and_agent() {
        let base = NOW;
        let cfg = config();
        let mut s = state();

        handle_session_start(&mut s, "a", at(base, 0.0));
        handle_prompt(&mut s, &cfg, "a", at(base, 3.0), false); // 3m user thinking
        handle_stop(&mut s, &cfg, "a", at(base, 5.0), false); // 2m agent generation

        let b = breakdown(&s.intervals);
        assert!((b.user - 3.0).abs() < 0.01);
        assert!((b.agent - 2.0).abs() < 0.01);
        assert!((s.accumulated_minutes - 5.0).abs() < 0.01);
        assert_eq!(b.overlap, 0.0);
    }

    #[test]
    fn prompt_drops_large_user_gap() {
        let base = NOW;
        let mut s = state();
        handle_session_start(&mut s, "a", base);
        handle_prompt(&mut s, &config(), "a", at(base, 30.0), false); // gap 30 > cap 10
        assert_eq!(s.accumulated_minutes, 0.0);
    }

    #[test]
    fn first_prompt_without_start_only_opens_agent() {
        let mut s = state();
        handle_prompt(&mut s, &config(), "a", NOW, false);
        assert_eq!(s.accumulated_minutes, 0.0);
        assert!(s.intervals.is_empty());
        assert_eq!(s.open.len(), 1);
        assert_eq!(s.open[0].kind, Kind::Agent);
    }

    #[test]
    fn stop_clamps_generation_at_cap() {
        let base = NOW;
        let cfg = config();
        let mut s = state();
        handle_session_start(&mut s, "a", base);
        handle_prompt(&mut s, &cfg, "a", at(base, 1.0), false); // open agent
        handle_stop(&mut s, &cfg, "a", at(base, 1.0 + 6.0 * 60.0), false); // 6h → clamp 120
        let b = breakdown(&s.intervals);
        assert!((b.agent - 120.0).abs() < 0.01);
        assert!((b.user - 1.0).abs() < 0.01);
    }

    #[test]
    fn threshold_crosses_on_union_total() {
        let mut s = state();
        s.accumulated_minutes = 58.0;
        assert_eq!(check_thresholds(&mut s, &config()), None); // not yet
        s.accumulated_minutes = 61.0;
        assert_eq!(check_thresholds(&mut s, &config()), Some(60));
        assert!(s.nudges_given.contains(&60));
    }

    #[test]
    fn threshold_not_refired_after_crossing() {
        let mut s = state();
        s.accumulated_minutes = 61.0;
        assert_eq!(check_thresholds(&mut s, &config()), Some(60));
        assert_eq!(check_thresholds(&mut s, &config()), None);
        assert_eq!(s.nudges_given.iter().filter(|&&t| t == 60).count(), 1);
    }

    #[test]
    fn threshold_crosses_multiple_returns_highest() {
        let mut s = state();
        s.accumulated_minutes = 121.0;
        assert_eq!(check_thresholds(&mut s, &config()), Some(120));
        assert!(s.nudges_given.contains(&120));
    }

    #[test]
    fn multi_cross_marks_all_lower_thresholds_no_renag() {
        // A single event jumps past two thresholds at once (e.g. a long agent
        // generation counted at the next prompt). Both must be marked so the
        // lower one doesn't re-fire on the following prompt.
        let mut s = state();
        s.accumulated_minutes = 130.0; // past 60 and 120 (config [60,120,180,240])
        assert_eq!(check_thresholds(&mut s, &config()), Some(120));
        assert!(s.nudges_given.contains(&60));
        assert!(s.nudges_given.contains(&120));
        // No further nudge on the next prompt at the same total.
        assert_eq!(check_thresholds(&mut s, &config()), None);
    }

    #[test]
    fn session_end_finalizes_agent_and_clears_cursor() {
        let base = NOW;
        let cfg = config();
        let mut s = state();
        handle_session_start(&mut s, "a", base);
        handle_prompt(&mut s, &cfg, "a", at(base, 1.0), false); // open agent
        handle_session_end(&mut s, &cfg, "a", at(base, 4.0), false); // 3m agent
        assert!(s.open.iter().all(|c| c.session_id != "a"));
        let b = breakdown(&s.intervals);
        assert!((b.agent - 3.0).abs() < 0.01);
    }

    #[test]
    fn session_end_per_session_leaves_others_running() {
        let base = NOW;
        let cfg = config();
        let mut s = state();
        handle_session_start(&mut s, "a", base);
        handle_session_start(&mut s, "b", base);
        handle_prompt(&mut s, &cfg, "a", at(base, 1.0), false);
        handle_prompt(&mut s, &cfg, "b", at(base, 1.0), false);
        handle_session_end(&mut s, &cfg, "a", at(base, 2.0), false);
        // b's agent cursor survives.
        assert!(s.open.iter().any(|c| c.session_id == "b"));
        assert!(s.open.iter().all(|c| c.session_id != "a"));
    }

    #[test]
    fn parallel_sessions_union_and_overlap() {
        let base = NOW;
        let cfg = config();
        let mut s = state();
        handle_session_start(&mut s, "a", base);
        handle_session_start(&mut s, "b", base);
        handle_prompt(&mut s, &cfg, "a", at(base, 1.0), false); // user a [0,1]
        handle_prompt(&mut s, &cfg, "b", at(base, 1.0), false); // user b [0,1]
        handle_stop(&mut s, &cfg, "a", at(base, 5.0), false); // agent a [1,5]
        handle_stop(&mut s, &cfg, "b", at(base, 6.0), false); // agent b [1,6]
        let b = breakdown(&s.intervals);
        assert!((b.total - 6.0).abs() < 0.01); // union [0,6]
        assert!((b.agent - 5.0).abs() < 0.01); // union [1,6]
        assert!((b.user - 1.0).abs() < 0.01); // union [0,1]
                                              // overlap: users [0,1]=1 + agents [1,5]=4 → 5
        assert!((b.overlap - 5.0).abs() < 0.01);
    }

    #[test]
    fn single_session_matches_legacy_sum() {
        // A linear session's union total equals the old simple sum.
        let base = NOW;
        let cfg = config();
        let mut s = state();
        handle_session_start(&mut s, "a", base);
        let mut t = base;
        for (gap, is_prompt) in [(2.0, true), (4.0, false), (3.0, true), (5.0, false)] {
            t += gap * 60.0;
            if is_prompt {
                handle_prompt(&mut s, &cfg, "a", t, false);
            } else {
                handle_stop(&mut s, &cfg, "a", t, false);
            }
        }
        assert!((s.accumulated_minutes - 14.0).abs() < 0.01);
        assert_eq!(breakdown(&s.intervals).overlap, 0.0);
    }

    #[test]
    fn upgrade_baseline_preserves_existing_total() {
        let base = NOW;
        let cfg = config();
        let mut s = state();
        s.accumulated_minutes = 50.0; // migrated from old scalar model
                                      // A session that started before the upgrade has no cursor; first prompt
                                      // opens an agent cursor, first stop records the first interval + seeds.
        handle_session_start(&mut s, "a", base);
        handle_prompt(&mut s, &cfg, "a", at(base, 2.0), false); // closes user [0,2]
        assert!((s.accumulated_minutes - 52.0).abs() < 0.01); // 50 migrated + 2
        assert!(s.intervals.iter().any(|i| i.session_id == "migrated"));
    }

    #[test]
    fn upgrade_baseline_survives_event_without_open_cursor() {
        // Migrated state (accumulated > 0, no intervals, no cursors — as a
        // pre-upgrade session has). The first post-upgrade event can be a
        // prompt/stop/session-end for a session that has no open cursor yet.
        // Such an event closes no cursor and records no interval, so the
        // recompute must NOT zero the migrated baseline.
        let cfg = config();
        for event in ["prompt", "stop", "session-end"] {
            let mut s = state();
            s.accumulated_minutes = 90.0;
            match event {
                "prompt" => {
                    handle_prompt(&mut s, &cfg, "a", NOW, false);
                }
                "stop" => handle_stop(&mut s, &cfg, "a", NOW, false),
                _ => handle_session_end(&mut s, &cfg, "a", NOW, false),
            }
            assert!(
                (s.accumulated_minutes - 90.0).abs() < 0.01,
                "{event}: migrated baseline wiped, got {}",
                s.accumulated_minutes
            );
        }
    }

    #[test]
    fn upgrade_baseline_survives_first_gap_over_cap() {
        // Migrated state (accumulated > 0, no intervals) where the very first
        // recorded gap exceeds the cap and is dropped. The drop must not wipe
        // the migrated baseline.
        let base = NOW;
        let cfg = config(); // max_prompt_gap_minutes = 10
        let mut s = state();
        s.accumulated_minutes = 90.0; // migrated from old scalar model
        handle_session_start(&mut s, "a", base); // opens user cursor
        handle_prompt(&mut s, &cfg, "a", at(base, 30.0), false); // 30m gap > 10m cap → dropped
        assert!((s.accumulated_minutes - 90.0).abs() < 0.01);
        assert!(s.intervals.iter().any(|i| i.session_id == "migrated"));
    }

    fn dt(h: u32, min: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 3, 16)
            .unwrap()
            .and_hms_opt(h, min, 0)
            .unwrap()
    }

    #[test]
    fn bedtime_fires_in_window() {
        let mut s = state();
        assert!(check_bedtime(&mut s, &config(), dt(1, 30)));
        assert!(s.bedtime_nudge_given);
    }

    #[test]
    fn bedtime_not_in_window() {
        let mut s = state();
        assert!(!check_bedtime(&mut s, &config(), dt(22, 0)));
        assert!(!s.bedtime_nudge_given);
    }

    #[test]
    fn bedtime_only_fires_once() {
        let mut s = state();
        s.bedtime_nudge_given = true;
        assert!(!check_bedtime(&mut s, &config(), dt(1, 30)));
    }

    #[test]
    fn bedtime_wraps_midnight() {
        let mut s = state();
        let cfg = Config {
            reset_hour: 0,
            ..config()
        };
        assert!(check_bedtime(&mut s, &cfg, dt(23, 15)));
    }

    #[test]
    fn live_view_counts_a_running_agent_turn() {
        let base = NOW;
        let cfg = config();
        let mut s = state();
        handle_session_start(&mut s, "a", base);
        handle_prompt(&mut s, &cfg, "a", at(base, 2.0), false); // 2m user, agent opens
        assert!((s.accumulated_minutes - 2.0).abs() < 0.01); // persisted lags

        let v = live_view(&s, &cfg, at(base, 32.0), Project::All);
        assert!((v.total - 32.0).abs() < 0.01);
        assert!((v.agent - 30.0).abs() < 0.01);
        assert!((v.user - 2.0).abs() < 0.01);
        assert_eq!(v.agents_running, 1);
        // Nothing was mutated: the view is a projection.
        assert!((s.accumulated_minutes - 2.0).abs() < 0.01);
        assert_eq!(s.intervals.len(), 1);
    }

    #[test]
    fn live_view_ignores_an_agent_cursor_past_the_cap() {
        // A session that crashed mid-turn leaves its cursor open for the rest
        // of the day; it must not read as two hours of running agent.
        let cfg = config(); // max_generation_minutes = 120
        let mut s = state();
        handle_prompt(&mut s, &cfg, "a", NOW, false);
        let running = live_view(&s, &cfg, at(NOW, 119.0), Project::All);
        assert!((running.total - 119.0).abs() < 0.01);
        let dead = live_view(&s, &cfg, at(NOW, 600.0), Project::All);
        assert_eq!(dead.total, 0.0);
        assert_eq!(dead.agents_running, 0);
    }

    #[test]
    fn live_view_user_gap_counts_until_it_exceeds_the_limit() {
        let cfg = config(); // max_prompt_gap_minutes = 10
        let mut s = state();
        handle_session_start(&mut s, "a", NOW);
        let within = live_view(&s, &cfg, at(NOW, 8.0), Project::All);
        assert!((within.total - 8.0).abs() < 0.01);
        assert!((within.user - 8.0).abs() < 0.01);
        let away = live_view(&s, &cfg, at(NOW, 11.0), Project::All);
        assert_eq!(away.total, 0.0);
        // The nudge lens never counts an unconfirmed reading gap.
        let nudge = live_view(&s, &cfg, at(NOW, 8.0), Project::AgentOnly);
        assert_eq!(nudge.total, 0.0);
    }

    #[test]
    fn live_view_dedupes_parallel_sessions() {
        let cfg = config();
        let mut s = state();
        // Two agents generating over the same 30 minutes, one user reading in
        // a third session for the first 5 of them.
        handle_prompt(&mut s, &cfg, "a", NOW, false);
        handle_prompt(&mut s, &cfg, "b", NOW, false);
        handle_session_start(&mut s, "c", NOW);
        handle_prompt(&mut s, &cfg, "c", at(NOW, 5.0), false);
        handle_stop(&mut s, &cfg, "c", at(NOW, 6.0), false);

        let v = live_view(&s, &cfg, at(NOW, 30.0), Project::AgentOnly);
        assert!((v.total - 30.0).abs() < 0.01); // one half hour, not three
        assert!((v.agent - 30.0).abs() < 0.01);
        assert!((v.user - 5.0).abs() < 0.01);
        assert!((v.overlap - 30.0).abs() < 0.01);
        assert_eq!(v.agents_running, 2);
    }

    #[test]
    fn live_view_carries_a_migrated_baseline() {
        let cfg = config();
        let mut s = state();
        s.accumulated_minutes = 45.0; // pre-interval total, no intervals yet
        handle_prompt(&mut s, &cfg, "a", NOW, false);
        let v = live_view(&s, &cfg, at(NOW, 10.0), Project::All);
        assert!((v.total - 55.0).abs() < 0.01);
    }

    #[test]
    fn duplicate_events_do_not_double_count() {
        // The plugin and `garlic setup` can both be wired up, so every event
        // arrives twice a few milliseconds apart. The union must not move.
        let cfg = config();
        let mut once = state();
        let mut twice = state();
        let script: [(&str, f64); 5] = [
            ("start", 0.0),
            ("prompt", 3.0),
            ("stop", 8.0),
            ("prompt", 9.0),
            ("stop", 20.0),
        ];
        for (ev, m) in script {
            for (s, repeats) in [(&mut once, 1), (&mut twice, 2)] {
                for r in 0..repeats {
                    let t = at(NOW, m) + r as f64 * 0.05;
                    match ev {
                        "start" => handle_session_start(s, "a", t),
                        "prompt" => {
                            handle_prompt(s, &cfg, "a", t, false);
                        }
                        _ => handle_stop(s, &cfg, "a", t, false),
                    }
                }
            }
        }
        assert!((once.accumulated_minutes - 20.0).abs() < 0.01);
        assert!((twice.accumulated_minutes - once.accumulated_minutes).abs() < 0.01);
        let (a, b) = (breakdown(&once.intervals), breakdown(&twice.intervals));
        assert!((a.agent - b.agent).abs() < 0.01);
        assert!((a.user - b.user).abs() < 0.01);
    }

    #[test]
    fn interrupted_turn_stop_keeps_the_gap_rule() {
        // An interrupt is a stop: the time after it is a user gap, so stepping
        // away for an hour afterwards counts for nothing instead of being
        // booked as an hour of agent generation.
        let cfg = config();
        let mut s = state();
        handle_prompt(&mut s, &cfg, "a", NOW, false);
        handle_stop(&mut s, &cfg, "a", at(NOW, 2.0), false); // interrupted at 2m
        handle_prompt(&mut s, &cfg, "a", at(NOW, 62.0), false); // back an hour later
        assert!((s.accumulated_minutes - 2.0).abs() < 0.01);
    }
}
