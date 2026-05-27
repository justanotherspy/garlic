//! Pure time-accounting logic, ported faithfully from garlic's `engine.py`
//! and the rollover half of `state.py::load_state`.
//!
//! Every function is synchronous and side-effect free apart from mutating the
//! `State` it is handed, which lets the storage layer run them inside a locked
//! read-modify-write critical section. Time is read through the `Clock` trait
//! so the server clock is the single source of truth across all clients.

use std::collections::HashSet;

use chrono::{Duration, Timelike};

use crate::model::{HistoryEntry, Interval, Kind, OpenCursor, State, TimeConfig, HISTORY_MAX};

/// Intervals closer than this (seconds) are treated as abutting for coalescing.
const COALESCE_EPSILON_SECS: f64 = 1.0;

/// Per-day derived totals (minutes): union total, per-kind unions, and the
/// concurrent (2+ sessions) wall-clock time tracked as a neutral fact.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DayBreakdown {
    pub total: f64,
    pub agent: f64,
    pub user: f64,
    pub overlap: f64,
}

/// Sweep-line over closed intervals; unions overlapping time so two parallel
/// agents over the same wall-clock hour count once, and reports concurrency.
pub fn breakdown(intervals: &[Interval]) -> DayBreakdown {
    let mut events: Vec<(f64, i32, Kind)> = Vec::with_capacity(intervals.len() * 2);
    for iv in intervals {
        if iv.end <= iv.start {
            continue;
        }
        events.push((iv.start, 1, iv.kind));
        events.push((iv.end, -1, iv.kind));
    }
    if events.is_empty() {
        return DayBreakdown::default();
    }
    events.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    let (mut total, mut agent, mut user, mut overlap) = (0.0, 0.0, 0.0, 0.0);
    let (mut active, mut active_agent, mut active_user) = (0i32, 0i32, 0i32);
    let mut last: Option<f64> = None;
    for (t, delta, kind) in events {
        if let Some(prev) = last {
            let dt = t - prev;
            if dt > 0.0 {
                if active >= 1 {
                    total += dt;
                }
                if active >= 2 {
                    overlap += dt;
                }
                if active_agent >= 1 {
                    agent += dt;
                }
                if active_user >= 1 {
                    user += dt;
                }
            }
        }
        active += delta;
        match kind {
            Kind::Agent => active_agent += delta,
            Kind::User => active_user += delta,
        }
        last = Some(t);
    }
    DayBreakdown {
        total: total / 60.0,
        agent: agent / 60.0,
        user: user / 60.0,
        overlap: overlap / 60.0,
    }
}

/// Append a closed interval, coalescing into the same session's most recent
/// interval when they share a kind and abut.
fn push_interval(intervals: &mut Vec<Interval>, iv: Interval) {
    if iv.end <= iv.start {
        return;
    }
    if let Some(prev) = intervals
        .iter_mut()
        .rev()
        .find(|p| p.session_id == iv.session_id)
    {
        if prev.kind == iv.kind && (iv.start - prev.end).abs() <= COALESCE_EPSILON_SECS {
            prev.end = iv.end.max(prev.end);
            return;
        }
    }
    intervals.push(iv);
}

fn take_cursor(state: &mut State, session_id: &str) -> Option<OpenCursor> {
    state
        .open
        .iter()
        .position(|c| c.session_id == session_id)
        .map(|pos| state.open.remove(pos))
}

fn open_cursor(state: &mut State, session_id: &str, kind: Kind, now: f64) {
    state.open.retain(|c| c.session_id != session_id);
    state.open.push(OpenCursor {
        session_id: session_id.to_string(),
        kind,
        start: now,
    });
}

/// Close `session_id`'s open cursor into an interval, applying the rule for its
/// stored kind (user gaps over the cap dropped; agent time clamped).
fn close_cursor(state: &mut State, config: &TimeConfig, session_id: &str, now: f64) {
    let Some(cursor) = take_cursor(state, session_id) else {
        return;
    };
    let end = match cursor.kind {
        Kind::User => {
            let raw_gap = (now - cursor.start) / 60.0;
            if raw_gap > config.max_prompt_gap_minutes {
                return;
            }
            now
        }
        Kind::Agent => {
            let cap_secs = config.max_generation_minutes * 60.0;
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

/// Preserve a pre-interval `accumulated_minutes` as one synthetic agent
/// interval on the first interval recorded after the backend upgrade.
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

fn recompute_total(state: &mut State) {
    state.accumulated_minutes = breakdown(&state.intervals).total;
}

/// Source of "now". The real implementation uses the server's wall clock and
/// local timezone (via `TZ`); tests inject a fixed clock.
pub trait Clock: Send + Sync {
    /// Seconds since the Unix epoch (server clock).
    fn now_unix(&self) -> f64;
    /// Current local wall-clock time, honoring the process timezone.
    fn local_now(&self) -> chrono::NaiveDateTime;
}

/// Production clock backed by the system time and local timezone.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_unix(&self) -> f64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0)
    }

    fn local_now(&self) -> chrono::NaiveDateTime {
        chrono::Local::now().naive_local()
    }
}

/// Today's tracking date as `YYYY-MM-DD`, shifted so the day rolls over at
/// `reset_hour` rather than midnight (mirrors `state.py::_current_date`).
pub fn current_date(reset_hour: u32, clock: &dyn Clock) -> String {
    let mut now = clock.local_now();
    if now.hour() < reset_hour {
        now -= Duration::days(1);
    }
    now.format("%Y-%m-%d").to_string()
}

/// Apply the daily rollover: if the stored date is not today, archive the
/// previous day into `history` (when it had recorded time) and reset the daily
/// fields. Returns `true` if the date changed (so callers can persist).
///
/// Mirrors the reset branch of `state.py::load_state`.
pub fn apply_rollover(state: &mut State, reset_hour: u32, clock: &dyn Clock) -> bool {
    let today = current_date(reset_hour, clock);
    if state.date == today {
        return false;
    }

    let old_date = std::mem::take(&mut state.date);
    let old_minutes = state.accumulated_minutes;
    let mut history = std::mem::take(&mut state.history);
    if !old_date.is_empty() && old_minutes > 0.0 {
        history.push(HistoryEntry {
            date: old_date,
            minutes: old_minutes,
        });
        if history.len() > HISTORY_MAX {
            let excess = history.len() - HISTORY_MAX;
            history.drain(0..excess);
        }
    }

    *state = State {
        date: today,
        history,
        ..State::default()
    };
    true
}

/// Open a user-thinking cursor for `session_id` (mirrors `handle_session_start`).
pub fn apply_session_start(state: &mut State, session_id: &str, clock: &dyn Clock) {
    open_cursor(state, session_id, Kind::User, clock.now_unix());
}

/// Close the session's user-thinking interval, open an agent interval, and
/// return the highest newly-crossed nudge threshold (mirrors `handle_prompt`).
pub fn apply_prompt(
    state: &mut State,
    config: &TimeConfig,
    session_id: &str,
    clock: &dyn Clock,
) -> Option<i64> {
    let now = clock.now_unix();
    close_cursor(state, config, session_id, now);
    open_cursor(state, session_id, Kind::Agent, now);
    recompute_total(state);
    check_thresholds(state, config)
}

/// Close the session's agent interval (clamped) and open a user-thinking
/// interval (mirrors `handle_stop`).
pub fn apply_stop(state: &mut State, config: &TimeConfig, session_id: &str, clock: &dyn Clock) {
    let now = clock.now_unix();
    close_cursor(state, config, session_id, now);
    open_cursor(state, session_id, Kind::User, now);
    recompute_total(state);
}

/// Finalize the session's in-flight interval and drop its cursor so a killed
/// session can't leak time into the next one (mirrors `handle_session_end`).
pub fn apply_session_end(
    state: &mut State,
    config: &TimeConfig,
    session_id: &str,
    clock: &dyn Clock,
) {
    let now = clock.now_unix();
    close_cursor(state, config, session_id, now);
    state.open.retain(|c| c.session_id != session_id);
    recompute_total(state);
}

/// Return the highest threshold newly crossed this call, marking every
/// newly-crossed threshold as given.
///
/// When a merge jumps the total past several thresholds at once, marking only
/// the highest would let the lower ones re-fire one at a time on later prompts
/// — nagging on every prompt and emitting a less severe message after the final
/// one. We mark them all and return the highest for the nudge.
fn check_thresholds(state: &mut State, config: &TimeConfig) -> Option<i64> {
    let mut thresholds = config.nudge_thresholds_minutes.clone();
    thresholds.sort_unstable();
    let mut highest = None;
    for &threshold in thresholds.iter() {
        if state.accumulated_minutes >= threshold as f64 && !state.nudges_given.contains(&threshold)
        {
            state.nudges_given.push(threshold);
            highest = Some(threshold);
        }
    }
    highest
}

/// Whether we're in the bedtime window (the hour before `reset_hour`) and
/// haven't nudged yet. Sets `bedtime_nudge_given` when it fires.
/// Mirrors `engine.py::check_bedtime`.
pub fn check_bedtime(state: &mut State, config: &TimeConfig, clock: &dyn Clock) -> bool {
    if state.bedtime_nudge_given {
        return false;
    }
    let bedtime_hour = (config.reset_hour + 24 - 1) % 24;
    if clock.local_now().hour() == bedtime_hour {
        state.bedtime_nudge_given = true;
        return true;
    }
    false
}

/// Zero the daily timer (mirrors `cli.py::cmd_reset`). History is preserved.
pub fn reset_daily(state: &mut State) {
    state.accumulated_minutes = 0.0;
    state.nudges_given.clear();
    state.ignored = false;
    state.last_event_time = 0.0;
    state.bedtime_nudge_given = false;
    state.intervals.clear();
    state.open.clear();
}

/// Set or toggle the `ignored` flag; returns the new value.
pub fn set_ignored(state: &mut State, value: Option<bool>) -> bool {
    state.ignored = value.unwrap_or(!state.ignored);
    state.ignored
}

/// Merge a client's closed intervals into the stored set.
///
/// This is the heart of the local-first sync path: clients account time locally
/// (offline-tolerant) and push their day's closed intervals here. Intervals are
/// absolute Unix spans, so the merge is the same union the single-machine engine
/// already does — two machines are just more sessions to union.
///
/// To stay idempotent under re-pushes (a client always sends its *full* day),
/// any stored interval whose `session_id` appears in the incoming set is
/// replaced; intervals from other sessions (i.e. other machines) are kept. The
/// daily total is then recomputed as the union across everything stored.
///
/// When `check_nudges` is set (a prompt event in synchronous mode) the highest
/// newly-crossed threshold and the bedtime window are evaluated against the
/// merged total; a plain sync leaves nudge bookkeeping untouched.
pub fn merge_intervals(
    state: &mut State,
    config: &TimeConfig,
    incoming: Vec<Interval>,
    check_nudges: bool,
    clock: &dyn Clock,
) -> (Option<i64>, bool) {
    if !incoming.is_empty() {
        let sessions: HashSet<&str> = incoming.iter().map(|i| i.session_id.as_str()).collect();
        state
            .intervals
            .retain(|i| !sessions.contains(i.session_id.as_str()));
        for iv in incoming {
            push_interval(&mut state.intervals, iv);
        }
    }
    recompute_total(state);

    if !check_nudges {
        return (None, false);
    }
    let crossed = check_thresholds(state, config);
    // Mirror apply_prompt: only consider bedtime when no threshold fired and
    // nudging isn't paused (check_bedtime has a once-per-night side effect).
    let bedtime = if crossed.is_none() && !state.ignored {
        check_bedtime(state, config, clock)
    } else {
        false
    };
    (crossed, bedtime)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Test clock with a settable unix time and local datetime.
    struct FixedClock {
        unix: Mutex<f64>,
        local: Mutex<chrono::NaiveDateTime>,
    }

    impl FixedClock {
        fn new(unix: f64) -> Self {
            FixedClock {
                unix: Mutex::new(unix),
                local: Mutex::new(
                    chrono::NaiveDate::from_ymd_opt(2026, 3, 16)
                        .unwrap()
                        .and_hms_opt(12, 0, 0)
                        .unwrap(),
                ),
            }
        }
        fn set_unix(&self, v: f64) {
            *self.unix.lock().unwrap() = v;
        }
        fn set_local(&self, dt: chrono::NaiveDateTime) {
            *self.local.lock().unwrap() = dt;
        }
    }

    impl Clock for FixedClock {
        fn now_unix(&self) -> f64 {
            *self.unix.lock().unwrap()
        }
        fn local_now(&self) -> chrono::NaiveDateTime {
            *self.local.lock().unwrap()
        }
    }

    fn config() -> TimeConfig {
        TimeConfig {
            max_prompt_gap_minutes: 10.0,
            max_generation_minutes: 120.0,
            reset_hour: 2,
            nudge_thresholds_minutes: vec![60, 120, 180, 240],
        }
    }

    #[test]
    fn prompt_then_stop_splits_user_and_agent() {
        let base = 1_710_567_900.0;
        let clock = FixedClock::new(base);
        let mut state = State::default();
        apply_session_start(&mut state, "a", &clock);
        clock.set_unix(base + 180.0); // 3m thinking
        apply_prompt(&mut state, &config(), "a", &clock);
        clock.set_unix(base + 300.0); // 2m generation
        apply_stop(&mut state, &config(), "a", &clock);
        let b = breakdown(&state.intervals);
        assert!((b.user - 3.0).abs() < 0.01);
        assert!((b.agent - 2.0).abs() < 0.01);
        assert!((state.accumulated_minutes - 5.0).abs() < 0.01);
    }

    #[test]
    fn prompt_drops_gap_over_cap() {
        let base = 1_710_567_900.0;
        let clock = FixedClock::new(base);
        let mut state = State::default();
        apply_session_start(&mut state, "a", &clock);
        clock.set_unix(base + 1800.0); // 30m > cap 10
        apply_prompt(&mut state, &config(), "a", &clock);
        assert_eq!(state.accumulated_minutes, 0.0);
    }

    #[test]
    fn first_prompt_without_start_only_opens_agent() {
        let now = 1_710_567_900.0;
        let clock = FixedClock::new(now);
        let mut state = State::default();
        apply_prompt(&mut state, &config(), "a", &clock);
        assert_eq!(state.accumulated_minutes, 0.0);
        assert_eq!(state.open.len(), 1);
        assert_eq!(state.open[0].kind, Kind::Agent);
    }

    #[test]
    fn threshold_crosses_on_union_total() {
        let mut state = State {
            accumulated_minutes: 61.0,
            ..State::default()
        };
        assert_eq!(check_thresholds(&mut state, &config()), Some(60));
        assert!(state.nudges_given.contains(&60));
    }

    #[test]
    fn threshold_fires_once() {
        let mut state = State {
            accumulated_minutes: 66.0,
            nudges_given: vec![60],
            ..State::default()
        };
        assert_eq!(check_thresholds(&mut state, &config()), None);
    }

    #[test]
    fn multi_cross_marks_all_lower_thresholds_no_renag() {
        // A merge jumps the total past two thresholds at once: both must be
        // marked so the lower one doesn't re-fire on the next prompt.
        let mut state = State {
            accumulated_minutes: 130.0, // past 60 and 120 (config [60,120,180,240])
            ..State::default()
        };
        assert_eq!(check_thresholds(&mut state, &config()), Some(120));
        assert!(state.nudges_given.contains(&60));
        assert!(state.nudges_given.contains(&120));
        assert_eq!(check_thresholds(&mut state, &config()), None);
    }

    #[test]
    fn stop_clamps_generation_time() {
        let base = 1_710_567_900.0;
        let clock = FixedClock::new(base);
        let mut state = State::default();
        apply_session_start(&mut state, "a", &clock);
        clock.set_unix(base + 60.0); // 1m thinking
        apply_prompt(&mut state, &config(), "a", &clock); // open agent
        clock.set_unix(base + 60.0 + 9000.0); // 150m generation, clamps to 120
        apply_stop(&mut state, &config(), "a", &clock);
        let b = breakdown(&state.intervals);
        assert!((b.agent - 120.0).abs() < 0.01);
    }

    #[test]
    fn session_end_finalizes_and_drops_cursor() {
        let base = 1_710_567_900.0;
        let clock = FixedClock::new(base);
        let mut state = State::default();
        apply_session_start(&mut state, "a", &clock);
        clock.set_unix(base + 60.0);
        apply_prompt(&mut state, &config(), "a", &clock); // open agent
        clock.set_unix(base + 60.0 + 600.0); // 10m generation
        apply_session_end(&mut state, &config(), "a", &clock);
        assert!(state.open.iter().all(|c| c.session_id != "a"));
        assert!((breakdown(&state.intervals).agent - 10.0).abs() < 0.01);
    }

    #[test]
    fn parallel_sessions_union_and_overlap() {
        let base = 1_710_567_900.0;
        let clock = FixedClock::new(base);
        let mut state = State::default();
        apply_session_start(&mut state, "a", &clock);
        apply_session_start(&mut state, "b", &clock);
        clock.set_unix(base + 60.0);
        apply_prompt(&mut state, &config(), "a", &clock); // user a [0,1]
        apply_prompt(&mut state, &config(), "b", &clock); // user b [0,1]
        clock.set_unix(base + 300.0);
        apply_stop(&mut state, &config(), "a", &clock); // agent a [1,5]
        clock.set_unix(base + 360.0);
        apply_stop(&mut state, &config(), "b", &clock); // agent b [1,6]
        let b = breakdown(&state.intervals);
        assert!((b.total - 6.0).abs() < 0.01);
        assert!((b.overlap - 5.0).abs() < 0.01);
    }

    #[test]
    fn rollover_archives_previous_day() {
        let clock = FixedClock::new(1_710_567_900.0);
        // local time noon on 2026-03-16 -> today is 2026-03-16
        let mut state = State {
            date: "2026-03-15".to_string(),
            accumulated_minutes: 123.0,
            nudges_given: vec![60],
            ignored: true,
            ..State::default()
        };
        let changed = apply_rollover(&mut state, 2, &clock);
        assert!(changed);
        assert_eq!(state.date, "2026-03-16");
        assert_eq!(state.accumulated_minutes, 0.0);
        assert!(state.nudges_given.is_empty());
        assert!(!state.ignored);
        assert_eq!(state.history.len(), 1);
        assert_eq!(state.history[0].date, "2026-03-15");
        assert_eq!(state.history[0].minutes, 123.0);
    }

    #[test]
    fn rollover_skips_archive_for_empty_day() {
        let clock = FixedClock::new(1_710_567_900.0);
        let mut state = State::default(); // date == ""
        let changed = apply_rollover(&mut state, 2, &clock);
        assert!(changed);
        assert_eq!(state.date, "2026-03-16");
        assert!(state.history.is_empty());
    }

    #[test]
    fn rollover_noop_same_day() {
        let clock = FixedClock::new(1_710_567_900.0);
        let mut state = State {
            date: "2026-03-16".to_string(),
            accumulated_minutes: 30.0,
            ..State::default()
        };
        let changed = apply_rollover(&mut state, 2, &clock);
        assert!(!changed);
        assert_eq!(state.accumulated_minutes, 30.0);
    }

    #[test]
    fn current_date_shifts_before_reset_hour() {
        let clock = FixedClock::new(0.0);
        // 01:30 local on 2026-03-16, reset_hour 2 -> still 2026-03-15
        clock.set_local(
            chrono::NaiveDate::from_ymd_opt(2026, 3, 16)
                .unwrap()
                .and_hms_opt(1, 30, 0)
                .unwrap(),
        );
        assert_eq!(current_date(2, &clock), "2026-03-15");
    }

    #[test]
    fn history_capped_at_max() {
        let clock = FixedClock::new(1_710_567_900.0);
        let mut history: Vec<HistoryEntry> = (0..HISTORY_MAX)
            .map(|i| HistoryEntry {
                date: format!("2026-01-{:02}", i + 1),
                minutes: 10.0,
            })
            .collect();
        // prepend marker we expect to be evicted
        history.insert(
            0,
            HistoryEntry {
                date: "1999-01-01".to_string(),
                minutes: 1.0,
            },
        );
        let mut state = State {
            date: "2026-03-15".to_string(),
            accumulated_minutes: 5.0,
            history,
            ..State::default()
        };
        apply_rollover(&mut state, 2, &clock);
        assert_eq!(state.history.len(), HISTORY_MAX);
        assert!(state.history.iter().all(|e| e.date != "1999-01-01"));
    }

    #[test]
    fn bedtime_fires_once_in_window() {
        let clock = FixedClock::new(0.0);
        // reset_hour 2 -> bedtime hour 1
        clock.set_local(
            chrono::NaiveDate::from_ymd_opt(2026, 3, 16)
                .unwrap()
                .and_hms_opt(1, 15, 0)
                .unwrap(),
        );
        let mut state = State::default();
        assert!(check_bedtime(&mut state, &config(), &clock));
        assert!(state.bedtime_nudge_given);
        // second call same window -> false
        assert!(!check_bedtime(&mut state, &config(), &clock));
    }

    #[test]
    fn bedtime_outside_window() {
        let clock = FixedClock::new(0.0);
        clock.set_local(
            chrono::NaiveDate::from_ymd_opt(2026, 3, 16)
                .unwrap()
                .and_hms_opt(12, 0, 0)
                .unwrap(),
        );
        let mut state = State::default();
        assert!(!check_bedtime(&mut state, &config(), &clock));
    }

    #[test]
    fn reset_daily_clears_fields_keeps_history() {
        let mut state = State {
            accumulated_minutes: 99.0,
            nudges_given: vec![60, 120],
            ignored: true,
            last_event_time: 123.0,
            bedtime_nudge_given: true,
            history: vec![HistoryEntry {
                date: "2026-03-15".to_string(),
                minutes: 10.0,
            }],
            ..State::default()
        };
        reset_daily(&mut state);
        assert_eq!(state.accumulated_minutes, 0.0);
        assert!(state.nudges_given.is_empty());
        assert!(!state.ignored);
        assert_eq!(state.last_event_time, 0.0);
        assert!(!state.bedtime_nudge_given);
        assert_eq!(state.history.len(), 1);
    }

    #[test]
    fn set_ignored_toggles_and_sets() {
        let mut state = State::default();
        assert!(set_ignored(&mut state, None)); // false -> true
        assert!(!set_ignored(&mut state, None)); // true -> false
        assert!(set_ignored(&mut state, Some(true)));
        assert!(set_ignored(&mut state, Some(true))); // idempotent set
    }

    #[test]
    fn set_unix_helper_is_usable() {
        // guard against dead-code warnings while documenting clock mutability
        let clock = FixedClock::new(1.0);
        clock.set_unix(2.0);
        assert_eq!(clock.now_unix(), 2.0);
    }
}
