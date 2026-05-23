//! Pure time-accounting logic, ported faithfully from garlic's `engine.py`
//! and the rollover half of `state.py::load_state`.
//!
//! Every function is synchronous and side-effect free apart from mutating the
//! `State` it is handed, which lets the storage layer run them inside a locked
//! read-modify-write critical section. Time is read through the `Clock` trait
//! so the server clock is the single source of truth across all clients.

use chrono::{Duration, Timelike};

use crate::model::{HistoryEntry, State, TimeConfig, HISTORY_MAX};

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

/// Record a session-start timestamp (mirrors `handle_session_start`).
pub fn apply_session_start(state: &mut State, clock: &dyn Clock) {
    state.last_event_time = clock.now_unix();
}

/// Accumulate the user's thinking gap (capped, dropped if over the cap) and
/// return the highest newly-crossed nudge threshold, if any.
///
/// Mirrors `handle_prompt` + `_check_thresholds`.
pub fn apply_prompt(state: &mut State, config: &TimeConfig, clock: &dyn Clock) -> Option<i64> {
    let now = clock.now_unix();
    let last = state.last_event_time;
    if last > 0.0 {
        let raw_gap = (now - last) / 60.0;
        let gap = if raw_gap <= config.max_prompt_gap_minutes {
            raw_gap
        } else {
            0.0
        };
        state.accumulated_minutes += gap;
    }
    state.last_event_time = now;
    check_thresholds(state, config)
}

/// Accumulate generation time, clamped to `max_generation_minutes`
/// (mirrors `handle_stop`).
pub fn apply_stop(state: &mut State, config: &TimeConfig, clock: &dyn Clock) {
    accumulate_generation(state, config, clock);
    // handle_stop leaves last_event_time at `now` (set inside the helper).
}

/// Finalize in-flight generation time and clear `last_event_time` so a killed
/// session can't leak time into the next one (mirrors `handle_session_end`).
pub fn apply_session_end(state: &mut State, config: &TimeConfig, clock: &dyn Clock) {
    accumulate_generation(state, config, clock);
    state.last_event_time = 0.0;
}

fn accumulate_generation(state: &mut State, config: &TimeConfig, clock: &dyn Clock) {
    let now = clock.now_unix();
    let last = state.last_event_time;
    if last > 0.0 {
        let raw_gap = (now - last) / 60.0;
        let gap = raw_gap.min(config.max_generation_minutes);
        state.accumulated_minutes += gap;
    }
    state.last_event_time = now;
}

/// Return the highest threshold newly crossed this call, marking it as given.
/// Only one (the highest) is marked per call, matching `_check_thresholds`.
fn check_thresholds(state: &mut State, config: &TimeConfig) -> Option<i64> {
    let mut thresholds = config.nudge_thresholds_minutes.clone();
    thresholds.sort_unstable();
    for &threshold in thresholds.iter().rev() {
        if state.accumulated_minutes >= threshold as f64 && !state.nudges_given.contains(&threshold)
        {
            state.nudges_given.push(threshold);
            return Some(threshold);
        }
    }
    None
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
}

/// Set or toggle the `ignored` flag; returns the new value.
pub fn set_ignored(state: &mut State, value: Option<bool>) -> bool {
    state.ignored = value.unwrap_or(!state.ignored);
    state.ignored
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
    fn prompt_accumulates_capped_gap() {
        let now = 1_710_567_900.0;
        let clock = FixedClock::new(now);
        let mut state = State {
            last_event_time: now - 300.0, // 5 minutes ago
            ..State::default()
        };
        apply_prompt(&mut state, &config(), &clock);
        assert!((state.accumulated_minutes - 5.0).abs() < 0.01);
        assert_eq!(state.last_event_time, now);
    }

    #[test]
    fn prompt_drops_gap_over_cap() {
        let now = 1_710_567_900.0;
        let clock = FixedClock::new(now);
        let mut state = State {
            last_event_time: now - 1800.0, // 30 minutes ago, cap is 10
            ..State::default()
        };
        apply_prompt(&mut state, &config(), &clock);
        assert_eq!(state.accumulated_minutes, 0.0);
    }

    #[test]
    fn first_prompt_does_not_accumulate() {
        let now = 1_710_567_900.0;
        let clock = FixedClock::new(now);
        let mut state = State::default();
        apply_prompt(&mut state, &config(), &clock);
        assert_eq!(state.accumulated_minutes, 0.0);
        assert_eq!(state.last_event_time, now);
    }

    #[test]
    fn prompt_crosses_threshold_marks_only_highest() {
        let now = 1_710_567_900.0;
        let clock = FixedClock::new(now);
        let mut state = State {
            accumulated_minutes: 58.0,
            last_event_time: now - 300.0,
            ..State::default()
        };
        let crossed = apply_prompt(&mut state, &config(), &clock);
        assert_eq!(crossed, Some(60));
        assert!(state.nudges_given.contains(&60));
    }

    #[test]
    fn threshold_fires_once() {
        let now = 1_710_567_900.0;
        let clock = FixedClock::new(now);
        let mut state = State {
            accumulated_minutes: 65.0,
            nudges_given: vec![60],
            last_event_time: now - 60.0,
            ..State::default()
        };
        // Only 1 minute gap; still at ~66m, 60 already given, 120 not reached.
        let crossed = apply_prompt(&mut state, &config(), &clock);
        assert_eq!(crossed, None);
    }

    #[test]
    fn stop_clamps_generation_time() {
        let now = 1_710_567_900.0;
        let clock = FixedClock::new(now);
        let mut state = State {
            last_event_time: now - 9000.0, // 150 minutes, clamps to 120
            ..State::default()
        };
        apply_stop(&mut state, &config(), &clock);
        assert!((state.accumulated_minutes - 120.0).abs() < 0.01);
        assert_eq!(state.last_event_time, now);
    }

    #[test]
    fn session_end_clears_last_event_time() {
        let now = 1_710_567_900.0;
        let clock = FixedClock::new(now);
        let mut state = State {
            last_event_time: now - 600.0, // 10 minutes
            ..State::default()
        };
        apply_session_end(&mut state, &config(), &clock);
        assert!((state.accumulated_minutes - 10.0).abs() < 0.01);
        assert_eq!(state.last_event_time, 0.0);
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
