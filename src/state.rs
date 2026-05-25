//! State file (`state.toml`) read/write with file locking.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};

use chrono::{Local, NaiveDateTime, Timelike};
use serde::{Deserialize, Serialize};

use crate::intervals::{Interval, OpenCursor};
use crate::paths::Paths;

pub const HISTORY_MAX: usize = 30;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub date: String,
    pub minutes: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct State {
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub accumulated_minutes: f64,
    #[serde(default)]
    pub last_event_time: f64,
    #[serde(default)]
    pub nudges_given: Vec<i64>,
    #[serde(default)]
    pub ignored: bool,
    #[serde(default)]
    pub bedtime_nudge_given: bool,
    #[serde(default)]
    pub history: Vec<HistoryEntry>,
    /// Closed intervals tracked today (agent/user spans, tagged by session).
    #[serde(default)]
    pub intervals: Vec<Interval>,
    /// In-flight cursors, one per session with an interval still open.
    #[serde(default)]
    pub open: Vec<OpenCursor>,
}

impl State {
    fn fresh(date: &str) -> State {
        State {
            date: date.to_string(),
            ..State::default()
        }
    }

    /// Serialize to TOML. The `toml` crate emits whole-number `f64`s with a
    /// trailing `.0`, so the float fields round-trip without the hand-rolled
    /// formatting garlic used before nested interval tables existed.
    pub fn to_toml(&self) -> String {
        toml::to_string(self).unwrap_or_default()
    }
}

/// Today's date string, shifted by `reset_hour`.
///
/// If it's before `reset_hour` (e.g. 2 AM) we still consider it the previous
/// day for tracking purposes.
pub fn current_date(reset_hour: i64) -> String {
    current_date_at(reset_hour, Local::now().naive_local())
}

/// `current_date` with an injected clock, for deterministic tests.
pub fn current_date_at(reset_hour: i64, now: NaiveDateTime) -> String {
    let shifted = if (now.hour() as i64) < reset_hour {
        now - chrono::Duration::days(1)
    } else {
        now
    };
    shifted.format("%Y-%m-%d").to_string()
}

/// Load state from disk, resetting if the date has rolled over.
pub fn load_state(paths: &Paths, reset_hour: i64) -> State {
    load_state_for_date(paths, &current_date(reset_hour))
}

/// `load_state` with an explicit "today", for deterministic tests.
///
/// Uses a shared `flock` for concurrent-session safety. If the state file is
/// corrupt or has invalid TOML syntax, reset to defaults and continue so hooks
/// don't break after upgrades that change the state format.
pub fn load_state_for_date(paths: &Paths, today: &str) -> State {
    let _ = fs::create_dir_all(paths.dir());
    let path = paths.state();

    let contents = match read_locked(&path) {
        Some(c) => c,
        None => return State::fresh(today),
    };

    let mut state: State = match toml::from_str(&contents) {
        Ok(s) => s,
        Err(_) => return State::fresh(today),
    };

    // Day reset — archive the previous day before resetting.
    if state.date != today {
        let mut history = std::mem::take(&mut state.history);
        if !state.date.is_empty() && state.accumulated_minutes > 0.0 {
            history.push(HistoryEntry {
                date: state.date.clone(),
                minutes: state.accumulated_minutes,
            });
            if history.len() > HISTORY_MAX {
                let start = history.len() - HISTORY_MAX;
                history = history.split_off(start);
            }
        }
        let mut fresh = State::fresh(today);
        fresh.history = history;
        return fresh;
    }

    state
}

/// Read a file's contents under a shared lock, or `None` if it can't be opened.
fn read_locked(path: &std::path::Path) -> Option<String> {
    let file = File::open(path).ok()?;
    let _ = file.lock_shared();
    let result = io::read_to_string(&file).ok();
    let _ = file.unlock();
    result
}

/// Write state to disk under an exclusive file lock.
pub fn save_state(paths: &Paths, state: &State) -> io::Result<()> {
    fs::create_dir_all(paths.dir())?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(paths.state())?;
    file.lock()?;
    let result = (|| {
        file.set_len(0)?;
        (&file).write_all(state.to_toml().as_bytes())?;
        (&file).flush()
    })();
    let _ = file.unlock();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intervals::Kind;
    use chrono::NaiveDate;
    use tempfile::TempDir;

    fn paths() -> (TempDir, Paths) {
        let tmp = TempDir::new().unwrap();
        let paths = Paths::with_dir(tmp.path().join(".garlic"));
        std::fs::create_dir_all(paths.dir()).unwrap();
        (tmp, paths)
    }

    fn dt(y: i32, m: u32, d: u32, h: u32, min: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(h, min, 0)
            .unwrap()
    }

    #[test]
    fn current_date_after_reset_hour() {
        assert_eq!(current_date_at(2, dt(2026, 3, 16, 10, 0)), "2026-03-16");
    }

    #[test]
    fn current_date_before_reset_hour() {
        assert_eq!(current_date_at(2, dt(2026, 3, 16, 1, 0)), "2026-03-15");
    }

    #[test]
    fn load_creates_fresh() {
        let (_tmp, paths) = paths();
        let state = load_state_for_date(&paths, "2026-03-16");
        assert_eq!(state.date, "2026-03-16");
        assert_eq!(state.accumulated_minutes, 0.0);
        assert!(state.nudges_given.is_empty());
        assert!(!state.ignored);
    }

    #[test]
    fn load_reads_existing() {
        let (_tmp, paths) = paths();
        std::fs::write(
            paths.state(),
            "date = \"2026-03-16\"\n\
             accumulated_minutes = 45.5\n\
             last_event_time = 1710567890.123\n\
             nudges_given = [60]\n\
             ignored = false\n",
        )
        .unwrap();
        let state = load_state_for_date(&paths, "2026-03-16");
        assert_eq!(state.accumulated_minutes, 45.5);
        assert_eq!(state.nudges_given, vec![60]);
    }

    #[test]
    fn resets_on_new_day_and_archives() {
        let (_tmp, paths) = paths();
        std::fs::write(
            paths.state(),
            "date = \"2026-03-15\"\n\
             accumulated_minutes = 120.0\n\
             last_event_time = 1710567890.123\n\
             nudges_given = [60, 120]\n\
             ignored = true\n",
        )
        .unwrap();
        let state = load_state_for_date(&paths, "2026-03-16");
        assert_eq!(state.date, "2026-03-16");
        assert_eq!(state.accumulated_minutes, 0.0);
        assert!(state.nudges_given.is_empty());
        assert!(!state.ignored);
        // Previous day archived to history.
        assert_eq!(state.history.len(), 1);
        assert_eq!(state.history[0].date, "2026-03-15");
        assert_eq!(state.history[0].minutes, 120.0);
    }

    #[test]
    fn new_day_with_zero_minutes_not_archived() {
        let (_tmp, paths) = paths();
        std::fs::write(
            paths.state(),
            "date = \"2026-03-15\"\n\
             accumulated_minutes = 0.0\n\
             last_event_time = 0.0\n\
             nudges_given = []\n\
             ignored = false\n",
        )
        .unwrap();
        let state = load_state_for_date(&paths, "2026-03-16");
        assert!(state.history.is_empty());
    }

    #[test]
    fn history_trimmed_to_max() {
        let (_tmp, paths) = paths();
        let mut entries = String::new();
        for i in 0..HISTORY_MAX {
            if i > 0 {
                entries.push_str(", ");
            }
            entries.push_str(&format!(
                "{{date = \"2026-01-{:02}\", minutes = 10.0}}",
                i + 1
            ));
        }
        std::fs::write(
            paths.state(),
            format!(
                "date = \"2026-03-15\"\n\
                 accumulated_minutes = 50.0\n\
                 last_event_time = 0.0\n\
                 nudges_given = []\n\
                 ignored = false\n\
                 bedtime_nudge_given = false\n\
                 history = [{entries}]\n"
            ),
        )
        .unwrap();
        let state = load_state_for_date(&paths, "2026-03-16");
        // 30 existing + 1 archived, trimmed back to 30 (oldest dropped).
        assert_eq!(state.history.len(), HISTORY_MAX);
        assert_eq!(state.history.last().unwrap().date, "2026-03-15");
    }

    #[test]
    fn handles_corrupted_file() {
        let (_tmp, paths) = paths();
        std::fs::write(
            paths.state(),
            "date = \"2026-03-16\"\n\
             accumulated_minutes = 45.5\n\
             bad_line_no_equals\n\
             ignored = false\n",
        )
        .unwrap();
        let state = load_state_for_date(&paths, "2026-03-16");
        assert_eq!(state.date, "2026-03-16");
        assert_eq!(state.accumulated_minutes, 0.0);
        assert!(state.nudges_given.is_empty());
    }

    #[test]
    fn save_roundtrip() {
        let (_tmp, paths) = paths();
        let state = State {
            date: "2026-03-16".to_string(),
            accumulated_minutes: 95.3,
            last_event_time: 1710567890.123,
            nudges_given: vec![60],
            ignored: true,
            bedtime_nudge_given: false,
            history: vec![HistoryEntry {
                date: "2026-03-15".to_string(),
                minutes: 120.0,
            }],
            intervals: vec![
                Interval {
                    session_id: "s1".to_string(),
                    kind: Kind::User,
                    start: 1710567000.0,
                    end: 1710567180.0,
                },
                Interval {
                    session_id: "s1".to_string(),
                    kind: Kind::Agent,
                    start: 1710567180.0,
                    end: 1710567890.0,
                },
            ],
            open: vec![OpenCursor {
                session_id: "s1".to_string(),
                kind: Kind::User,
                start: 1710567890.123,
            }],
        };
        save_state(&paths, &state).unwrap();
        let loaded = load_state_for_date(&paths, "2026-03-16");
        assert_eq!(loaded, state);
    }

    #[test]
    fn to_toml_keeps_whole_floats() {
        // Whole-number floats must round-trip as floats, not integers.
        let state = State {
            date: "2026-03-16".to_string(),
            accumulated_minutes: 180.0,
            history: vec![HistoryEntry {
                date: "2026-03-15".to_string(),
                minutes: 120.0,
            }],
            ..State::default()
        };
        let parsed: State = toml::from_str(&state.to_toml()).unwrap();
        assert_eq!(parsed, state);
    }

    #[test]
    fn loads_legacy_state_without_intervals() {
        // Pre-upgrade state.toml has no intervals/open fields.
        let (_tmp, paths) = paths();
        std::fs::write(
            paths.state(),
            "date = \"2026-03-16\"\n\
             accumulated_minutes = 45.5\n\
             last_event_time = 1710567890.123\n\
             nudges_given = [30]\n\
             ignored = false\n\
             bedtime_nudge_given = false\n\
             history = []\n",
        )
        .unwrap();
        let state = load_state_for_date(&paths, "2026-03-16");
        assert_eq!(state.accumulated_minutes, 45.5);
        assert!(state.intervals.is_empty());
        assert!(state.open.is_empty());
    }

    #[test]
    fn concurrent_saves_dont_corrupt() {
        let (_tmp, paths) = paths();
        let mut handles = Vec::new();
        for i in 0..20 {
            let paths = paths.clone();
            handles.push(std::thread::spawn(move || {
                let state = State {
                    date: "2026-03-16".to_string(),
                    accumulated_minutes: i as f64,
                    ..State::default()
                };
                save_state(&paths, &state).unwrap();
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        // File must be valid TOML after all writes.
        let contents = std::fs::read_to_string(paths.state()).unwrap();
        let parsed: State = toml::from_str(&contents).unwrap();
        assert_eq!(parsed.date, "2026-03-16");
    }
}
