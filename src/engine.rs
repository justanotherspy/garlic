//! Time gap calculation, accumulation, and threshold checking.
//!
//! All functions take an injected `now` so they stay pure and deterministic
//! under test. `now` is a Unix timestamp in seconds (matching `time.time()`);
//! `now_local` is the wall-clock time used only for the bedtime window.

use chrono::{NaiveDateTime, Timelike};

use crate::config::Config;
use crate::state::State;

/// Process a prompt event: accumulate time and check thresholds.
///
/// Returns the threshold (in minutes) that was just crossed, or `None`.
pub fn handle_prompt(state: &mut State, config: &Config, now: f64, debug: bool) -> Option<i64> {
    let last = state.last_event_time;

    if last > 0.0 {
        let raw_gap = (now - last) / 60.0;
        let cap = config.max_prompt_gap_minutes as f64;
        let gap_minutes = if raw_gap <= cap { raw_gap } else { 0.0 };
        state.accumulated_minutes += gap_minutes;

        if debug {
            let dropped = if raw_gap > cap {
                format!(" → dropped (exceeded {cap}m cap)")
            } else {
                String::new()
            };
            eprintln!(
                "[garlic debug] gap: {raw_gap:.2}m{dropped} | total: {:.2}m",
                state.accumulated_minutes
            );
        }
    } else if debug {
        eprintln!("[garlic debug] no last_event_time, skipping gap");
    }

    state.last_event_time = now;

    check_thresholds(state, config)
}

/// Process a stop event: accumulate generation time and update
/// `last_event_time`. Generation time is clamped to `max_generation_minutes`.
pub fn handle_stop(state: &mut State, config: &Config, now: f64, debug: bool) {
    let last = state.last_event_time;

    if last > 0.0 {
        let raw_gap = (now - last) / 60.0;
        let cap = config.max_generation_minutes as f64;
        let gap_minutes = raw_gap.min(cap);
        state.accumulated_minutes += gap_minutes;

        if debug {
            let clamped = if raw_gap > cap {
                format!(" → clamped to {cap:.0}m cap")
            } else {
                String::new()
            };
            eprintln!(
                "[garlic debug] stop gap: {raw_gap:.2}m{clamped} | total: {:.2}m",
                state.accumulated_minutes
            );
        }
    }

    state.last_event_time = now;
}

/// Process a session-start event: record timestamp.
pub fn handle_session_start(state: &mut State, now: f64) {
    state.last_event_time = now;
}

/// Process a session-end event: finalize in-flight time and clear
/// `last_event_time`.
///
/// If the session was killed mid-generation (SIGTERM, crash, network loss),
/// Stop never fires. Without this, `last_event_time` would stay set and the
/// next session's first prompt would compute a huge gap. This accumulates the
/// outstanding gap (clamped to `max_generation_minutes`) and zeroes
/// `last_event_time`.
pub fn handle_session_end(state: &mut State, config: &Config, now: f64, debug: bool) {
    let last = state.last_event_time;

    if last > 0.0 {
        let raw_gap = (now - last) / 60.0;
        let cap = config.max_generation_minutes as f64;
        let gap_minutes = raw_gap.min(cap);
        state.accumulated_minutes += gap_minutes;

        if debug {
            let clamped = if raw_gap > cap {
                format!(" → clamped to {cap:.0}m cap")
            } else {
                String::new()
            };
            eprintln!(
                "[garlic debug] session-end gap: {raw_gap:.2}m{clamped} | total: {:.2}m",
                state.accumulated_minutes
            );
        }
    }

    state.last_event_time = 0.0;
}

/// Return the highest newly-crossed threshold, or `None`.
fn check_thresholds(state: &mut State, config: &Config) -> Option<i64> {
    let accumulated = state.accumulated_minutes;
    let mut thresholds = config.nudge_thresholds_minutes.clone();
    thresholds.sort_unstable();

    for &threshold in thresholds.iter().rev() {
        if accumulated >= threshold as f64 && !state.nudges_given.contains(&threshold) {
            state.nudges_given.push(threshold);
            return Some(threshold);
        }
    }

    None
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
    fn prompt_accumulates_time() {
        let mut s = state();
        s.last_event_time = NOW - 300.0;
        handle_prompt(&mut s, &config(), NOW, false);
        assert!((s.accumulated_minutes - 5.0).abs() < 0.01);
        assert_eq!(s.last_event_time, NOW);
    }

    #[test]
    fn prompt_drops_large_gap() {
        let mut s = state();
        s.last_event_time = NOW - 1800.0;
        handle_prompt(&mut s, &config(), NOW, false);
        assert_eq!(s.accumulated_minutes, 0.0);
    }

    #[test]
    fn prompt_first_event_no_accumulation() {
        let mut s = state();
        handle_prompt(&mut s, &config(), NOW, false);
        assert_eq!(s.accumulated_minutes, 0.0);
        assert_eq!(s.last_event_time, NOW);
    }

    #[test]
    fn prompt_crosses_threshold() {
        let mut s = state();
        s.accumulated_minutes = 58.0;
        s.last_event_time = NOW - 300.0;
        let result = handle_prompt(&mut s, &config(), NOW, false);
        assert_eq!(result, Some(60));
        assert!(s.nudges_given.contains(&60));
    }

    #[test]
    fn prompt_no_duplicate_nudge() {
        let mut s = state();
        s.accumulated_minutes = 58.0;
        s.last_event_time = NOW - 300.0;
        s.nudges_given = vec![60];
        let result = handle_prompt(&mut s, &config(), NOW, false);
        assert_eq!(result, None);
    }

    #[test]
    fn prompt_crosses_multiple_returns_highest() {
        let mut s = state();
        s.accumulated_minutes = 115.0;
        s.last_event_time = NOW - 600.0;
        let result = handle_prompt(&mut s, &config(), NOW, false);
        assert_eq!(result, Some(120));
        assert!(s.nudges_given.contains(&120));
    }

    #[test]
    fn stop_accumulates_generation_time() {
        let mut s = state();
        s.accumulated_minutes = 30.0;
        s.last_event_time = NOW - 180.0;
        handle_stop(&mut s, &config(), NOW, false);
        assert_eq!(s.last_event_time, NOW);
        assert!((s.accumulated_minutes - 33.0).abs() < 0.01);
    }

    #[test]
    fn stop_clamps_at_generation_cap() {
        let mut s = state();
        s.last_event_time = NOW - 6.0 * 3600.0;
        handle_stop(&mut s, &config(), NOW, false);
        assert!((s.accumulated_minutes - 120.0).abs() < 0.01);
    }

    #[test]
    fn stop_counts_normal_generation_in_full() {
        let mut s = state();
        s.last_event_time = NOW - 3600.0;
        handle_stop(&mut s, &config(), NOW, false);
        assert!((s.accumulated_minutes - 60.0).abs() < 0.01);
    }

    #[test]
    fn stop_no_accumulation_without_last_event() {
        let mut s = state();
        handle_stop(&mut s, &config(), NOW, false);
        assert_eq!(s.last_event_time, NOW);
        assert_eq!(s.accumulated_minutes, 0.0);
    }

    #[test]
    fn session_end_accumulates_and_clears() {
        let mut s = state();
        s.accumulated_minutes = 30.0;
        s.last_event_time = NOW - 180.0;
        handle_session_end(&mut s, &config(), NOW, false);
        assert!((s.accumulated_minutes - 33.0).abs() < 0.01);
        assert_eq!(s.last_event_time, 0.0);
    }

    #[test]
    fn session_end_clamps_at_generation_cap() {
        let mut s = state();
        s.last_event_time = NOW - 6.0 * 3600.0;
        handle_session_end(&mut s, &config(), NOW, false);
        assert!((s.accumulated_minutes - 120.0).abs() < 0.01);
        assert_eq!(s.last_event_time, 0.0);
    }

    #[test]
    fn session_end_no_last_event_still_clears() {
        let mut s = state();
        s.accumulated_minutes = 5.0;
        handle_session_end(&mut s, &config(), NOW, false);
        assert_eq!(s.accumulated_minutes, 5.0);
        assert_eq!(s.last_event_time, 0.0);
    }

    #[test]
    fn session_killed_before_stop_does_not_double_count() {
        let base = NOW;
        let cfg = Config {
            max_prompt_gap_minutes: 40,
            ..config()
        };
        let mut s = state();
        s.last_event_time = base;

        handle_session_end(&mut s, &cfg, at(base, 5.0), false);
        assert!((s.accumulated_minutes - 5.0).abs() < 0.01);
        assert_eq!(s.last_event_time, 0.0);

        handle_session_start(&mut s, at(base, 120.0));
        handle_prompt(&mut s, &cfg, at(base, 123.0), false);

        assert!((s.accumulated_minutes - 8.0).abs() < 0.01);
    }

    #[test]
    fn session_start_records_timestamp() {
        let mut s = state();
        handle_session_start(&mut s, NOW);
        assert_eq!(s.last_event_time, NOW);
    }

    #[test]
    fn sequence_prompt_stop_prompt() {
        let base = NOW;
        let cfg = config();
        let mut s = state();

        handle_session_start(&mut s, at(base, 0.0));
        handle_prompt(&mut s, &cfg, at(base, 3.0), false);
        assert!((s.accumulated_minutes - 3.0).abs() < 0.01);

        handle_stop(&mut s, &cfg, at(base, 5.0), false);
        assert!((s.accumulated_minutes - 5.0).abs() < 0.01);
        assert!((s.last_event_time - at(base, 5.0)).abs() < 0.01);

        handle_prompt(&mut s, &cfg, at(base, 7.0), false);
        assert!((s.accumulated_minutes - 7.0).abs() < 0.01);
    }

    #[test]
    fn sequence_multiple_prompts_accumulate() {
        let base = NOW;
        let cfg = config();
        let mut s = state();
        s.last_event_time = base;

        let gaps = [2.0, 4.0, 3.0, 5.0];
        let mut t = base;
        for gap in gaps {
            t += gap * 60.0;
            handle_prompt(&mut s, &cfg, t, false);
        }
        assert!((s.accumulated_minutes - 14.0).abs() < 0.01);
    }

    #[test]
    fn sequence_gap_exceeding_cap_then_normal() {
        let base = NOW;
        let cfg = config();
        let mut s = state();
        s.last_event_time = base;

        handle_prompt(&mut s, &cfg, at(base, 30.0), false);
        assert_eq!(s.accumulated_minutes, 0.0);

        handle_prompt(&mut s, &cfg, at(base, 34.0), false);
        assert!((s.accumulated_minutes - 4.0).abs() < 0.01);
    }

    #[test]
    fn all_thresholds_fire_in_order() {
        let base = NOW;
        let cfg = Config {
            max_prompt_gap_minutes: 70,
            ..config()
        };
        let mut s = state();
        s.last_event_time = base;

        let mut fired = Vec::new();
        for i in 1..=4 {
            if let Some(t) = handle_prompt(&mut s, &cfg, at(base, (i * 65) as f64), false) {
                fired.push(t);
            }
        }
        assert_eq!(fired, vec![60, 120, 180, 240]);
        assert_eq!(s.nudges_given, vec![60, 120, 180, 240]);
    }

    #[test]
    fn threshold_not_refired_after_crossing() {
        let base = NOW;
        let cfg = Config {
            nudge_thresholds_minutes: vec![60],
            ..config()
        };
        let mut s = state();
        s.accumulated_minutes = 59.0;
        s.last_event_time = base;

        assert_eq!(handle_prompt(&mut s, &cfg, at(base, 2.0), false), Some(60));
        assert_eq!(handle_prompt(&mut s, &cfg, at(base, 4.0), false), None);
        assert_eq!(s.nudges_given.iter().filter(|&&t| t == 60).count(), 1);
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
}
