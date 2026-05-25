//! Implementations of garlic's subcommands.
//!
//! Commands write user-facing output to an injected `out`/`err` writer and
//! return a process exit code, so they're deterministic under test.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, Timelike};

use crate::config::{load_config, save_config, Config, CONFIG_KEYS, VALID_NUDGE_STYLES};
use crate::format::format_duration;
use crate::intervals::breakdown;
use crate::paths::{ClaudePaths, Paths};
use crate::remote::Remote;
use crate::setup::install_hooks;
use crate::state::{load_state, load_state_for_date, save_state, State};
use crate::sync::flush;
use crate::version::{check_latest_version, CRATES_API_URL};

const GARLIC: &str = "\u{1f9c4}";
const VAMPIRE: &str = "\u{1f9db}";
const CROSS: &str = "\u{2716}";
const ARROW: &str = "\u{2190}";
const MIDDOT: &str = "\u{00b7}";
const CONCURRENT: &str = "\u{21c4}";

const MONTH_NAMES: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const MONTH_ABBR: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// Read a confirmation/answer line, returning `None` on EOF (cancel).
pub trait Confirm {
    fn ask(&mut self, prompt: &str) -> Option<String>;
}

/// Build an ASCII progress bar. `fraction` is 0.0–1.0+.
fn progress_bar(fraction: f64, width: usize) -> String {
    let filled = ((fraction * width as f64) as usize).min(width);
    let mut bar = String::with_capacity(width * 3);
    for _ in 0..filled {
        bar.push('\u{2588}');
    }
    for _ in 0..(width - filled) {
        bar.push('\u{2591}');
    }
    bar
}

/// Apply the daily reset-hour shift to a wall-clock time.
fn shift(now_local: NaiveDateTime, reset_hour: i64) -> NaiveDateTime {
    if (now_local.hour() as i64) < reset_hour {
        now_local - Duration::days(1)
    } else {
        now_local
    }
}

pub fn cmd_version(paths: &Paths, now: f64, out: &mut dyn Write) -> i32 {
    let version = env!("CARGO_PKG_VERSION");
    let _ = writeln!(out, "garlic {version}");
    if let Some(latest) = check_latest_version(version, &paths.version_cache(), CRATES_API_URL, now)
    {
        let _ = writeln!(out, "  update available: {latest}");
        let _ = writeln!(
            out,
            "  run: cargo install garlic-ward --force  (or: cargo binstall garlic-ward)"
        );
    }
    0
}

#[allow(clippy::too_many_arguments)]
pub fn cmd_status(
    remote: Option<&Remote>,
    paths: &Paths,
    json: bool,
    week: bool,
    month: bool,
    now_local: NaiveDateTime,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let config = load_config(paths);

    // Week/month are retrospective summaries built from per-day history, which
    // the local state always has in full. The backend's history can have gaps
    // in the default (non-blocking) mode — it's only built when the backend
    // processes a request — so these views stay purely local. Only the live
    // "today" view below consults the backend for the merged cross-machine
    // total.
    if week || month {
        let today = shift(now_local, config.reset_hour).date();
        let state = load_state_for_date(paths, &today.format("%Y-%m-%d").to_string());
        return if week {
            week_core(&state, &config, today, out)
        } else {
            stats_core(&state, today, out)
        };
    }

    // Local-first: when a backend is configured, flush this machine's intervals
    // and read back the merged (shared) total in one round trip. If it's
    // unreachable, fall back to local state with a note — we always have data,
    // so an offline backend never blocks a status check.
    let mut local = load_state(paths, config.reset_hour);
    let state = match remote {
        Some(r) => match flush(r, paths, &config, &mut local, false) {
            Ok(resp) => resp.state,
            Err(e) => {
                let _ = writeln!(
                    err,
                    "garlic: shared backend unavailable ({e}); showing local time"
                );
                local
            }
        },
        None => local,
    };
    status_core(&state, &config, json, out)
}

/// Render `status` from an already-loaded state and config (shared by the
/// local and remote paths).
fn status_core(state: &State, config: &Config, json: bool, out: &mut dyn Write) -> i32 {
    let minutes = state.accumulated_minutes;
    let split = breakdown(&state.intervals);
    let mut thresholds = config.nudge_thresholds_minutes.clone();
    thresholds.sort_unstable();
    let nudges_given = &state.nudges_given;
    let ignored = state.ignored;
    let next_threshold = thresholds
        .iter()
        .find(|t| !nudges_given.contains(t))
        .copied();

    if json {
        let obj = serde_json::json!({
            "accumulated_minutes": minutes,
            "agent_minutes": split.agent,
            "user_minutes": split.user,
            "overlap_minutes": split.overlap,
            "thresholds": thresholds,
            "nudges_given": nudges_given,
            "next_threshold": next_threshold,
            "ignored": ignored,
            "date": state.date,
        });
        let _ = writeln!(out, "{}", serde_json::to_string(&obj).unwrap());
        return 0;
    }

    let time_str = format_duration(minutes);

    if ignored {
        let _ = writeln!(
            out,
            "{VAMPIRE} {time_str} of active coding today (nudging paused)"
        );
        let _ = writeln!(out, "   run 'garlic ignore' to resume");
    } else {
        let fraction = match next_threshold {
            Some(t) if t > 0 => minutes / t as f64,
            _ => 1.0,
        };
        let icon = if fraction < 0.85 { GARLIC } else { VAMPIRE };
        let _ = writeln!(out, "{icon} {time_str} of active coding today");
    }

    // Agent vs. user split, and concurrency as a neutral fact (never weighted).
    if split.agent > 0.0 || split.user > 0.0 {
        let _ = writeln!(
            out,
            "   agent {} {MIDDOT} user {}",
            format_duration(split.agent),
            format_duration(split.user),
        );
    }
    if split.overlap > 0.0 {
        let _ = writeln!(
            out,
            "   {} with 2+ sessions running at once",
            format_duration(split.overlap),
        );
    }

    if thresholds.is_empty() {
        return 0;
    }

    let crossed: Vec<i64> = thresholds
        .iter()
        .filter(|t| nudges_given.contains(t))
        .copied()
        .collect();
    if !crossed.is_empty() {
        let marks = crossed
            .iter()
            .map(|t| format!("{CROSS} {}", format_duration(*t as f64)))
            .collect::<Vec<_>>()
            .join("  ");
        let _ = writeln!(out, "  {marks}");
    }

    if thresholds.iter().all(|t| nudges_given.contains(t)) {
        let _ = writeln!(
            out,
            "\n  {VAMPIRE} You've crossed every threshold. Respect."
        );
    } else {
        if let Some(next_t) = next_threshold {
            let prev_t = thresholds
                .iter()
                .filter(|t| nudges_given.contains(t))
                .copied()
                .max()
                .unwrap_or(0);
            let span = next_t - prev_t;
            let progress = if span > 0 {
                (minutes - prev_t as f64) / span as f64
            } else {
                1.0
            };
            let bar = progress_bar(progress, 20);
            let remaining = format_duration((next_t as f64 - minutes).max(0.0));
            let _ = writeln!(
                out,
                "  {bar} {} ({remaining} to go)",
                format_duration(next_t as f64)
            );
        }
        let max_t = *thresholds.last().unwrap();
        let day_fraction = if max_t > 0 {
            minutes / max_t as f64
        } else {
            1.0
        };
        let day_bar = progress_bar(day_fraction, 20);
        let _ = writeln!(out, "  {day_bar} {} day", format_duration(max_t as f64));
    }

    0
}

pub fn cmd_statusline(paths: &Paths, out: &mut dyn Write) -> i32 {
    let config = load_config(paths);
    let state = load_state(paths, config.reset_hour);
    statusline_core(&state, &config, out)
}

fn statusline_core(state: &State, config: &Config, out: &mut dyn Write) -> i32 {
    let minutes = state.accumulated_minutes;
    let mut thresholds = config.nudge_thresholds_minutes.clone();
    thresholds.sort_unstable();
    let nudges_given = &state.nudges_given;

    let time_str = format_duration(minutes);
    let next_t = thresholds
        .iter()
        .find(|t| !nudges_given.contains(t))
        .copied();
    let fraction = match next_t {
        Some(t) => minutes / t as f64,
        None => 1.0,
    };
    let icon = if fraction >= 0.85 { VAMPIRE } else { GARLIC };

    let mut line = if let Some(max_t) = thresholds.last() {
        format!("{icon} {time_str} / {}", format_duration(*max_t as f64))
    } else {
        format!("{icon} {time_str}")
    };
    // Concurrency marker: 2+ sessions overlapped at some point today.
    if breakdown(&state.intervals).overlap > 0.0 {
        line.push_str(&format!(" {CONCURRENT}"));
    }
    if state.ignored {
        line.push_str(" (paused)");
    }
    let _ = writeln!(out, "{line}");
    0
}

fn week_core(state: &State, config: &Config, today: NaiveDate, out: &mut dyn Write) -> i32 {
    let today_str = today.format("%Y-%m-%d").to_string();
    let mut history_map: BTreeMap<String, f64> = BTreeMap::new();
    for entry in &state.history {
        history_map.insert(entry.date.clone(), entry.minutes);
    }

    let target = config
        .nudge_thresholds_minutes
        .iter()
        .copied()
        .max()
        .unwrap_or(240);
    let bar_max = target;

    let _ = writeln!(out, "{GARLIC} Weekly usage (last 7 days)\n");

    let mut total = 0.0;
    let mut under_target = 0;
    for i in (0..=6).rev() {
        let d = today - Duration::days(i);
        let date_str = d.format("%Y-%m-%d").to_string();
        let minutes = if date_str == today_str {
            state.accumulated_minutes
        } else {
            history_map.get(&date_str).copied().unwrap_or(0.0)
        };
        let day_label = d.format("%a %m/%d").to_string();
        let time_str = format_duration(minutes);
        let fraction = if bar_max > 0 {
            minutes / bar_max as f64
        } else {
            0.0
        };
        let bar = progress_bar(fraction, 16);
        let marker = if date_str == today_str {
            format!("  {ARROW} today")
        } else {
            String::new()
        };
        let _ = writeln!(out, "  {day_label}   {time_str:>7}  {bar}{marker}");
        total += minutes;
        if minutes < target as f64 {
            under_target += 1;
        }
    }

    let _ = writeln!(
        out,
        "\n  Total: {} {MIDDOT} {under_target} of 7 days under {} target",
        format_duration(total),
        format_duration(target as f64)
    );
    0
}

fn stats_core(state: &State, today: NaiveDate, out: &mut dyn Write) -> i32 {
    let today_str = today.format("%Y-%m-%d").to_string();
    let mut history_map: BTreeMap<String, f64> = BTreeMap::new();
    for entry in &state.history {
        history_map.insert(entry.date.clone(), entry.minutes);
    }

    let today_minutes = state.accumulated_minutes;
    if today_minutes > 0.0 {
        history_map.insert(today_str.clone(), today_minutes);
    }

    // This calendar month.
    let month_prefix = today.format("%Y-%m").to_string();
    let month_entries: Vec<f64> = history_map
        .iter()
        .filter(|(d, _)| d.starts_with(&month_prefix))
        .map(|(_, m)| *m)
        .collect();
    let month_total: f64 = month_entries.iter().sum();
    let month_active: Vec<f64> = month_entries.iter().copied().filter(|m| *m > 0.0).collect();
    let month_avg = if month_active.is_empty() {
        0.0
    } else {
        month_active.iter().sum::<f64>() / month_active.len() as f64
    };

    // Busiest day across recorded history.
    let mut busiest: Option<(String, f64)> = None;
    for (d, m) in &history_map {
        if *m > 0.0 && busiest.as_ref().is_none_or(|b| *m > b.1) {
            busiest = Some((d.clone(), *m));
        }
    }

    // Active dates (any recorded time).
    let active_dates: BTreeSet<String> = history_map
        .iter()
        .filter(|(_, m)| **m > 0.0)
        .map(|(d, _)| d.clone())
        .collect();

    // Current streak: consecutive days ending today (or yesterday if today=0).
    let mut current_streak = 0;
    let mut cursor = today;
    if !active_dates.contains(&cursor.format("%Y-%m-%d").to_string()) {
        cursor -= Duration::days(1);
    }
    while active_dates.contains(&cursor.format("%Y-%m-%d").to_string()) {
        current_streak += 1;
        cursor -= Duration::days(1);
    }

    // Longest streak across sorted active dates.
    let mut longest_streak = 0;
    let mut sorted: Vec<NaiveDate> = active_dates
        .iter()
        .filter_map(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
        .collect();
    sorted.sort_unstable();
    if !sorted.is_empty() {
        longest_streak = 1;
        let mut run = 1;
        for i in 1..sorted.len() {
            if (sorted[i] - sorted[i - 1]).num_days() == 1 {
                run += 1;
                longest_streak = longest_streak.max(run);
            } else {
                run = 1;
            }
        }
    }

    // Rolling 30 days (inclusive of today).
    let thirty_ago = today - Duration::days(29);
    let mut rolling_total = 0.0;
    let mut rolling_active = 0;
    for (d, m) in &history_map {
        if let Ok(date) = NaiveDate::parse_from_str(d, "%Y-%m-%d") {
            if thirty_ago <= date && date <= today {
                rolling_total += m;
                if *m > 0.0 {
                    rolling_active += 1;
                }
            }
        }
    }

    let month_label = format!(
        "{} {}",
        MONTH_NAMES[(today.month() - 1) as usize],
        today.year()
    );
    let _ = writeln!(out, "{GARLIC} Stats — {month_label}\n");
    let _ = writeln!(
        out,
        "  This month:      {:>8}   ({} active day{})",
        format_duration(month_total),
        month_active.len(),
        plural(month_active.len() as i64),
    );
    if !month_active.is_empty() {
        let _ = writeln!(
            out,
            "  Daily avg:       {:>8}   (active days only)",
            format_duration(month_avg)
        );
    }
    if let Some((date, minutes)) = &busiest {
        if let Ok(b) = NaiveDate::parse_from_str(date, "%Y-%m-%d") {
            let label = format!(
                "{} {} {:02}",
                weekday_abbr(b),
                MONTH_ABBR[(b.month() - 1) as usize],
                b.day()
            );
            let _ = writeln!(
                out,
                "  Busiest day:     {:>8}   ({label})",
                format_duration(*minutes)
            );
        }
    }
    let _ = writeln!(
        out,
        "  Current streak:  {current_streak:>5} day{}",
        plural(current_streak)
    );
    let _ = writeln!(
        out,
        "  Longest streak:  {longest_streak:>5} day{}",
        plural(longest_streak)
    );
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "  Rolling 30d:     {:>8}   ({rolling_active} active day{})",
        format_duration(rolling_total),
        plural(rolling_active),
    );
    let _ = writeln!(out, "\n  Note: based on the last 30 days of history.");
    0
}

fn plural(n: i64) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

fn weekday_abbr(d: NaiveDate) -> &'static str {
    const WEEKDAY_ABBR: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    WEEKDAY_ABBR[d.weekday().num_days_from_monday() as usize]
}

pub fn cmd_ignore(
    remote: Option<&Remote>,
    paths: &Paths,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let config = load_config(paths);
    let mut state = load_state(paths, config.reset_hour);
    state.ignored = !state.ignored;
    let _ = save_state(paths, &state);
    // Mirror the new flag to the shared backend so other machines agree; the
    // local toggle has already taken effect regardless of reachability.
    if let Some(r) = remote {
        if let Err(e) = r.ignore(&config, Some(state.ignored)) {
            let _ = writeln!(
                err,
                "garlic: shared backend unavailable ({e}); applied locally only"
            );
        }
    }
    write_ignore_state(out, state.ignored);
    0
}

/// Report whether nudging is now paused or resumed.
fn write_ignore_state(out: &mut dyn Write, ignored: bool) {
    if ignored {
        let _ = writeln!(
            out,
            "garlic: nudging disabled for today (tracking continues)"
        );
    } else {
        let _ = writeln!(out, "garlic: nudging resumed");
    }
}

/// A validated config value parsed from a `KEY=VALUE` assignment.
#[derive(Debug, PartialEq)]
pub enum ParsedValue {
    Int(i64),
    Thresholds(Vec<i64>),
    Style(String),
}

/// Parse and validate a config value string for the given key.
pub fn parse_config_value(key: &str, raw: &str) -> Result<ParsedValue, String> {
    match key {
        "nudge_style" => {
            if VALID_NUDGE_STYLES.contains(&raw) {
                Ok(ParsedValue::Style(raw.to_string()))
            } else {
                Err(format!(
                    "nudge_style must be one of: {}",
                    VALID_NUDGE_STYLES.join(", ")
                ))
            }
        }
        "nudge_thresholds_minutes" => {
            let mut values = Vec::new();
            for part in raw.split(',') {
                match part.trim().parse::<i64>() {
                    Ok(v) => values.push(v),
                    Err(_) => {
                        return Err(
                            "nudge_thresholds_minutes must be comma-separated integers (e.g. 60,120,180)"
                                .to_string(),
                        )
                    }
                }
            }
            if values.is_empty() {
                return Err("nudge_thresholds_minutes must not be empty".to_string());
            }
            if values.iter().any(|v| *v <= 0) {
                return Err("nudge_thresholds_minutes values must be positive integers".to_string());
            }
            let mut sorted = values.clone();
            sorted.sort_unstable();
            if values != sorted {
                let got = join_ints(&values);
                let want = join_ints(&sorted);
                return Err(format!(
                    "nudge_thresholds_minutes must be in ascending order (got {got}; did you mean {want}?)"
                ));
            }
            let unique: BTreeSet<i64> = values.iter().copied().collect();
            if unique.len() != values.len() {
                return Err("nudge_thresholds_minutes must not contain duplicates".to_string());
            }
            Ok(ParsedValue::Thresholds(values))
        }
        "reset_hour" => {
            let v = raw
                .parse::<i64>()
                .map_err(|_| format!("{key} must be an integer"))?;
            if !(0..=23).contains(&v) {
                return Err("reset_hour must be between 0 and 23".to_string());
            }
            Ok(ParsedValue::Int(v))
        }
        "max_prompt_gap_minutes" | "max_generation_minutes" => {
            let v = raw
                .parse::<i64>()
                .map_err(|_| format!("{key} must be an integer"))?;
            if v < 1 {
                return Err(format!("{key} must be a positive integer"));
            }
            Ok(ParsedValue::Int(v))
        }
        _ => Err(format!(
            "unknown config key: {key}. Valid keys: {}",
            CONFIG_KEYS.join(", ")
        )),
    }
}

fn join_ints(values: &[i64]) -> String {
    values
        .iter()
        .map(i64::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

/// Apply a parsed value to the config, returning its display form.
fn apply_value(config: &mut Config, key: &str, value: ParsedValue) -> String {
    match value {
        ParsedValue::Style(s) => {
            config.nudge_style = s.clone();
            s
        }
        ParsedValue::Thresholds(v) => {
            config.nudge_thresholds_minutes = v.clone();
            format!(
                "[{}]",
                v.iter().map(i64::to_string).collect::<Vec<_>>().join(", ")
            )
        }
        ParsedValue::Int(n) => {
            match key {
                "max_prompt_gap_minutes" => config.max_prompt_gap_minutes = n,
                "max_generation_minutes" => config.max_generation_minutes = n,
                "reset_hour" => config.reset_hour = n,
                _ => {}
            }
            n.to_string()
        }
    }
}

pub fn cmd_set(paths: &Paths, assignment: &str, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let Some((key, raw)) = assignment.split_once('=') else {
        let _ = writeln!(err, "garlic: expected KEY=VALUE, got '{assignment}'");
        return 1;
    };
    let key = key.trim();
    let raw = raw.trim();

    let value = match parse_config_value(key, raw) {
        Ok(v) => v,
        Err(e) => {
            let _ = writeln!(err, "garlic: {e}");
            return 1;
        }
    };

    let mut config = load_config(paths);
    let display = apply_value(&mut config, key, value);
    if let Err(e) = save_config(paths, &config) {
        let _ = writeln!(err, "garlic: {e}");
        return 1;
    }
    let _ = writeln!(out, "garlic: set {key} = {display}");
    0
}

/// Outcome of the reset confirmation prompt.
enum ResetChoice {
    Proceed,
    Cancelled,
    Eof,
}

/// Ask whether to reset (unless `yes`), printing the decision message.
fn ask_reset(yes: bool, confirm: &mut dyn Confirm, out: &mut dyn Write) -> ResetChoice {
    if yes {
        return ResetChoice::Proceed;
    }
    match confirm.ask("garlic: reset timer to zero for today? [y/N] ") {
        None => {
            let _ = writeln!(out);
            ResetChoice::Eof
        }
        Some(answer) => {
            if matches!(answer.trim().to_lowercase().as_str(), "y" | "yes") {
                ResetChoice::Proceed
            } else {
                let _ = writeln!(out, "garlic: reset cancelled");
                ResetChoice::Cancelled
            }
        }
    }
}

pub fn cmd_reset(
    remote: Option<&Remote>,
    paths: &Paths,
    yes: bool,
    confirm: &mut dyn Confirm,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    match ask_reset(yes, confirm, out) {
        ResetChoice::Proceed => {}
        ResetChoice::Cancelled => return 0,
        ResetChoice::Eof => return 1,
    }

    let config = load_config(paths);
    let mut state = load_state(paths, config.reset_hour);
    state.accumulated_minutes = 0.0;
    state.nudges_given.clear();
    state.ignored = false;
    state.last_event_time = 0.0;
    state.bedtime_nudge_given = false;
    // Clear the intervals too, otherwise the next sync would re-push them and
    // resurrect the total we just zeroed.
    state.intervals.clear();
    state.open.clear();

    // Zero the shared backend too. If it's unreachable, mark the reset pending
    // so the next sync zeroes it before pushing — otherwise the stale pre-reset
    // intervals would survive the union merge on the server.
    let mut reset_synced = true;
    if let Some(r) = remote {
        if let Err(e) = r.reset(&config) {
            reset_synced = false;
            let _ = writeln!(
                err,
                "garlic: shared backend unavailable ({e}); reset locally, will sync on next run"
            );
        }
    }
    state.reset_pending = !reset_synced;
    let _ = save_state(paths, &state);
    let _ = writeln!(out, "garlic: timer reset for today");
    0
}

/// `garlic sync` — push this machine's locally-accounted intervals to the
/// shared backend and report the merged total. This is the explicit drain for
/// the outbox, e.g. a laptop cron job running outside a network-restricted
/// sandbox. A no-op (with guidance) when no backend is configured.
pub fn cmd_sync(
    remote: Option<&Remote>,
    paths: &Paths,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let Some(r) = remote else {
        let _ = writeln!(
            err,
            "garlic: no backend configured (set GARLIC_URL and GARLIC_TOKEN to enable shared state)"
        );
        return 1;
    };
    let config = load_config(paths);
    let mut state = load_state(paths, config.reset_hour);
    match flush(r, paths, &config, &mut state, false) {
        Ok(resp) => {
            let _ = writeln!(
                out,
                "{GARLIC} synced \u{2014} {} of active coding today (shared)",
                format_duration(resp.state.accumulated_minutes)
            );
            0
        }
        Err(e) => {
            let _ = writeln!(err, "garlic: sync failed: {e}");
            1
        }
    }
}

/// Interactively prompt for config values. Returns `None` if cancelled (EOF).
pub fn prompt_config(confirm: &mut dyn Confirm, out: &mut dyn Write) -> Option<ConfigOverrides> {
    let _ = writeln!(
        out,
        "garlic setup — configure your preferences (press Enter for defaults)\n"
    );
    let defaults = Config::default();
    let mut ov = ConfigOverrides::default();

    // nudge_thresholds_minutes — asked as a single "nudge interval".
    let default_interval = defaults.nudge_thresholds_minutes[0];
    let raw = confirm.ask(&format!(
        "  Nudge interval in minutes [{default_interval}]: "
    ))?;
    let raw = raw.trim();
    if !raw.is_empty() {
        match raw.parse::<i64>() {
            Ok(interval) if interval >= 1 => {
                let cap = *defaults.nudge_thresholds_minutes.last().unwrap();
                let mut v = Vec::new();
                let mut t = interval;
                while t <= cap {
                    v.push(t);
                    t += interval;
                }
                ov.nudge_thresholds_minutes = Some(v);
            }
            _ => {
                let _ = writeln!(out, "  (invalid, using default {default_interval})");
            }
        }
    }

    let default_gap = defaults.max_prompt_gap_minutes;
    let raw = confirm.ask(&format!("  Max prompt gap in minutes [{default_gap}]: "))?;
    let raw = raw.trim();
    if !raw.is_empty() {
        match raw.parse::<i64>() {
            Ok(gap) if gap >= 1 => ov.max_prompt_gap_minutes = Some(gap),
            _ => {
                let _ = writeln!(out, "  (invalid, using default {default_gap})");
            }
        }
    }

    let default_reset = defaults.reset_hour;
    let raw = confirm.ask(&format!("  Daily reset hour (0-23) [{default_reset}]: "))?;
    let raw = raw.trim();
    if !raw.is_empty() {
        match raw.parse::<i64>() {
            Ok(hour) if (0..=23).contains(&hour) => ov.reset_hour = Some(hour),
            _ => {
                let _ = writeln!(out, "  (invalid, using default {default_reset})");
            }
        }
    }

    let default_style = &defaults.nudge_style;
    let raw = confirm.ask(&format!(
        "  Nudge style (gentle/firm/spicy) [{default_style}]: "
    ))?;
    let raw = raw.trim().to_lowercase();
    if !raw.is_empty() {
        if VALID_NUDGE_STYLES.contains(&raw.as_str()) {
            ov.nudge_style = Some(raw);
        } else {
            let _ = writeln!(out, "  (invalid, using default {default_style})");
        }
    }

    let _ = writeln!(out);
    Some(ov)
}

/// Overrides collected interactively or via `--defaults`.
#[derive(Debug, Default, PartialEq)]
pub struct ConfigOverrides {
    pub max_prompt_gap_minutes: Option<i64>,
    pub max_generation_minutes: Option<i64>,
    pub reset_hour: Option<i64>,
    pub nudge_thresholds_minutes: Option<Vec<i64>>,
    pub nudge_style: Option<String>,
}

impl ConfigOverrides {
    pub fn is_empty(&self) -> bool {
        self.max_prompt_gap_minutes.is_none()
            && self.max_generation_minutes.is_none()
            && self.reset_hour.is_none()
            && self.nudge_thresholds_minutes.is_none()
            && self.nudge_style.is_none()
    }

    fn apply(&self, config: &mut Config) {
        if let Some(v) = self.max_prompt_gap_minutes {
            config.max_prompt_gap_minutes = v;
        }
        if let Some(v) = self.max_generation_minutes {
            config.max_generation_minutes = v;
        }
        if let Some(v) = self.reset_hour {
            config.reset_hour = v;
        }
        if let Some(v) = &self.nudge_thresholds_minutes {
            config.nudge_thresholds_minutes = v.clone();
        }
        if let Some(v) = &self.nudge_style {
            config.nudge_style = v.clone();
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn cmd_setup(
    paths: &Paths,
    claude: &ClaudePaths,
    debug: bool,
    yes: bool,
    defaults: bool,
    confirm: &mut dyn Confirm,
    out: &mut dyn Write,
) -> i32 {
    let mut overrides = ConfigOverrides::default();

    if defaults {
        if !yes {
            match confirm.ask("garlic: overwrite config with built-in defaults? [y/N] ") {
                None => {
                    let _ = writeln!(out, "\ngarlic: setup cancelled");
                    return 1;
                }
                Some(answer) => {
                    if !matches!(answer.trim().to_lowercase().as_str(), "y" | "yes") {
                        let _ = writeln!(out, "garlic: setup cancelled");
                        return 0;
                    }
                }
            }
        }
    } else if !yes {
        match prompt_config(confirm, out) {
            None => {
                let _ = writeln!(out, "\ngarlic: setup cancelled");
                return 1;
            }
            Some(ov) => overrides = ov,
        }
    }

    let mut config = load_config(paths);
    if defaults {
        config = Config::default();
    } else {
        overrides.apply(&mut config);
    }
    if defaults || !overrides.is_empty() {
        if let Err(e) = save_config(paths, &config) {
            let _ = writeln!(out, "garlic: {e}");
            return 1;
        }
    }

    if let Err(e) = install_hooks(claude, debug) {
        let _ = writeln!(out, "garlic: {e}");
        return 1;
    }

    let mode = if debug { " (debug mode)" } else { "" };
    let _ = writeln!(
        out,
        "garlic: hooks installed in ~/.claude/settings.json{mode}"
    );
    let _ = writeln!(
        out,
        "garlic: /garlic slash command installed in ~/.claude/commands/"
    );
    if defaults {
        let _ = writeln!(out, "garlic: config reset to built-in defaults");
    }
    if Remote::from_env().is_some() {
        let _ = writeln!(
            out,
            "garlic: shared backend detected — hooks track locally and sync to it.\n\
             \x20      Schedule 'garlic sync' (e.g. cron) to reconcile totals, or set\n\
             \x20      GARLIC_SYNC=blocking on ephemeral hosts to flush inside each hook."
        );
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{save_state, State};
    use tempfile::TempDir;

    /// A scripted `Confirm` that returns queued answers and records prompts.
    struct ScriptedConfirm {
        answers: std::collections::VecDeque<Option<String>>,
        prompts: Vec<String>,
    }

    impl ScriptedConfirm {
        fn new(answers: Vec<Option<&str>>) -> Self {
            ScriptedConfirm {
                answers: answers.into_iter().map(|a| a.map(str::to_string)).collect(),
                prompts: Vec::new(),
            }
        }
    }

    impl Confirm for ScriptedConfirm {
        fn ask(&mut self, prompt: &str) -> Option<String> {
            self.prompts.push(prompt.to_string());
            self.answers.pop_front().flatten()
        }
    }

    fn env() -> (TempDir, Paths) {
        let tmp = TempDir::new().unwrap();
        let paths = Paths::with_dir(tmp.path().join(".garlic"));
        std::fs::create_dir_all(paths.dir()).unwrap();
        (tmp, paths)
    }

    fn write_state(paths: &Paths, today: &str, accumulated: f64, nudges: Vec<i64>, ignored: bool) {
        let state = State {
            date: today.to_string(),
            accumulated_minutes: accumulated,
            nudges_given: nudges,
            ignored,
            ..State::default()
        };
        save_state(paths, &state).unwrap();
    }

    // --- parse_config_value ---

    #[test]
    fn parse_style_valid() {
        for s in ["gentle", "firm", "spicy"] {
            assert_eq!(
                parse_config_value("nudge_style", s).unwrap(),
                ParsedValue::Style(s.to_string())
            );
        }
    }

    #[test]
    fn parse_style_invalid() {
        assert!(parse_config_value("nudge_style", "aggressive")
            .unwrap_err()
            .contains("must be one of"));
    }

    #[test]
    fn parse_int_keys() {
        assert_eq!(
            parse_config_value("max_prompt_gap_minutes", "15").unwrap(),
            ParsedValue::Int(15)
        );
        assert_eq!(
            parse_config_value("reset_hour", "4").unwrap(),
            ParsedValue::Int(4)
        );
    }

    #[test]
    fn parse_int_invalid() {
        assert!(parse_config_value("reset_hour", "abc")
            .unwrap_err()
            .contains("must be an integer"));
    }

    #[test]
    fn parse_reset_hour_out_of_range() {
        for bad in ["24", "99", "-1"] {
            assert!(
                parse_config_value("reset_hour", bad)
                    .unwrap_err()
                    .contains("between 0 and 23"),
                "expected range error for {bad}"
            );
        }
        // Boundaries are accepted.
        assert_eq!(
            parse_config_value("reset_hour", "0").unwrap(),
            ParsedValue::Int(0)
        );
        assert_eq!(
            parse_config_value("reset_hour", "23").unwrap(),
            ParsedValue::Int(23)
        );
    }

    #[test]
    fn parse_caps_must_be_positive() {
        for key in ["max_prompt_gap_minutes", "max_generation_minutes"] {
            for bad in ["0", "-5"] {
                assert!(
                    parse_config_value(key, bad)
                        .unwrap_err()
                        .contains("positive integer"),
                    "expected positivity error for {key}={bad}"
                );
            }
            assert_eq!(parse_config_value(key, "1").unwrap(), ParsedValue::Int(1));
        }
    }

    #[test]
    fn parse_thresholds() {
        assert_eq!(
            parse_config_value("nudge_thresholds_minutes", "30,60,90").unwrap(),
            ParsedValue::Thresholds(vec![30, 60, 90])
        );
    }

    #[test]
    fn parse_thresholds_invalid() {
        assert!(parse_config_value("nudge_thresholds_minutes", "30,abc,90")
            .unwrap_err()
            .contains("comma-separated integers"));
    }

    #[test]
    fn parse_thresholds_unsorted() {
        assert!(parse_config_value("nudge_thresholds_minutes", "240,30,10")
            .unwrap_err()
            .contains("ascending order"));
    }

    #[test]
    fn parse_thresholds_negative() {
        assert!(parse_config_value("nudge_thresholds_minutes", "-10,30,60")
            .unwrap_err()
            .contains("positive integers"));
    }

    #[test]
    fn parse_thresholds_zero() {
        assert!(parse_config_value("nudge_thresholds_minutes", "0,30,60")
            .unwrap_err()
            .contains("positive integers"));
    }

    #[test]
    fn parse_thresholds_duplicates() {
        assert!(parse_config_value("nudge_thresholds_minutes", "30,30,60")
            .unwrap_err()
            .contains("duplicates"));
    }

    #[test]
    fn parse_unknown_key() {
        assert!(parse_config_value("nonexistent", "value")
            .unwrap_err()
            .contains("unknown config key"));
    }

    // --- cmd_set ---

    #[test]
    fn set_nudge_style() {
        let (_t, paths) = env();
        let mut out = Vec::new();
        let mut err = Vec::new();
        assert_eq!(cmd_set(&paths, "nudge_style=spicy", &mut out, &mut err), 0);
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("nudge_style"));
        assert!(text.contains("spicy"));
        assert_eq!(load_config(&paths).nudge_style, "spicy");
    }

    #[test]
    fn set_thresholds() {
        let (_t, paths) = env();
        let mut out = Vec::new();
        let mut err = Vec::new();
        cmd_set(
            &paths,
            "nudge_thresholds_minutes=30,60,120",
            &mut out,
            &mut err,
        );
        assert_eq!(
            load_config(&paths).nudge_thresholds_minutes,
            vec![30, 60, 120]
        );
    }

    #[test]
    fn set_int_value() {
        let (_t, paths) = env();
        let mut out = Vec::new();
        let mut err = Vec::new();
        cmd_set(&paths, "reset_hour=10", &mut out, &mut err);
        assert_eq!(load_config(&paths).reset_hour, 10);
    }

    #[test]
    fn set_invalid_style_exits_1() {
        let (_t, paths) = env();
        let mut out = Vec::new();
        let mut err = Vec::new();
        assert_eq!(
            cmd_set(&paths, "nudge_style=invalid", &mut out, &mut err),
            1
        );
    }

    #[test]
    fn set_unknown_key_exits_1() {
        let (_t, paths) = env();
        let mut out = Vec::new();
        let mut err = Vec::new();
        assert_eq!(cmd_set(&paths, "bogus=5", &mut out, &mut err), 1);
    }

    #[test]
    fn set_no_equals_exits_1() {
        let (_t, paths) = env();
        let mut out = Vec::new();
        let mut err = Vec::new();
        assert_eq!(cmd_set(&paths, "noequals", &mut out, &mut err), 1);
        assert!(String::from_utf8(err)
            .unwrap()
            .contains("expected KEY=VALUE"));
    }

    // --- cmd_reset ---

    #[test]
    fn reset_with_yes_flag() {
        let (_t, paths) = env();
        write_state(
            &paths,
            &crate::state::current_date(2),
            95.0,
            vec![60],
            false,
        );
        let mut confirm = ScriptedConfirm::new(vec![]);
        let mut out = Vec::new();
        let mut err = Vec::new();
        assert_eq!(
            cmd_reset(None, &paths, true, &mut confirm, &mut out, &mut err),
            0
        );
        assert!(String::from_utf8(out).unwrap().contains("timer reset"));
        let state = load_state(&paths, 2);
        assert_eq!(state.accumulated_minutes, 0.0);
        assert!(state.nudges_given.is_empty());
    }

    #[test]
    fn reset_confirmed() {
        let (_t, paths) = env();
        write_state(
            &paths,
            &crate::state::current_date(2),
            95.0,
            vec![60],
            false,
        );
        let mut confirm = ScriptedConfirm::new(vec![Some("y")]);
        let mut out = Vec::new();
        let mut err = Vec::new();
        cmd_reset(None, &paths, false, &mut confirm, &mut out, &mut err);
        assert!(String::from_utf8(out).unwrap().contains("timer reset"));
    }

    #[test]
    fn reset_declined() {
        let (_t, paths) = env();
        write_state(
            &paths,
            &crate::state::current_date(2),
            95.0,
            vec![60],
            false,
        );
        let mut confirm = ScriptedConfirm::new(vec![Some("n")]);
        let mut out = Vec::new();
        let mut err = Vec::new();
        cmd_reset(None, &paths, false, &mut confirm, &mut out, &mut err);
        assert!(String::from_utf8(out).unwrap().contains("cancelled"));
        assert_eq!(load_state(&paths, 2).accumulated_minutes, 95.0);
    }

    // --- prompt_config ---

    #[test]
    fn prompt_config_all_defaults() {
        let mut confirm = ScriptedConfirm::new(vec![Some(""), Some(""), Some(""), Some("")]);
        let mut out = Vec::new();
        let result = prompt_config(&mut confirm, &mut out).unwrap();
        assert_eq!(result, ConfigOverrides::default());
    }

    #[test]
    fn prompt_config_custom_values() {
        let mut confirm =
            ScriptedConfirm::new(vec![Some("15"), Some("20"), Some("4"), Some("spicy")]);
        let mut out = Vec::new();
        let result = prompt_config(&mut confirm, &mut out).unwrap();
        let expected: Vec<i64> = (15..=240).step_by(15).collect();
        assert_eq!(result.nudge_thresholds_minutes, Some(expected));
        assert_eq!(result.max_prompt_gap_minutes, Some(20));
        assert_eq!(result.reset_hour, Some(4));
        assert_eq!(result.nudge_style, Some("spicy".to_string()));
    }

    #[test]
    fn prompt_config_invalid_values_use_defaults() {
        let mut confirm =
            ScriptedConfirm::new(vec![Some("abc"), Some("-5"), Some("99"), Some("bogus")]);
        let mut out = Vec::new();
        let result = prompt_config(&mut confirm, &mut out).unwrap();
        assert_eq!(result, ConfigOverrides::default());
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text.matches("invalid").count(), 4);
    }

    #[test]
    fn prompt_config_default_interval_tracks_config() {
        let expected = Config::default().nudge_thresholds_minutes[0];
        let mut confirm = ScriptedConfirm::new(vec![Some(""), Some(""), Some(""), Some("")]);
        let mut out = Vec::new();
        prompt_config(&mut confirm, &mut out);
        assert!(confirm.prompts[0].contains(&format!("[{expected}]")));
    }

    #[test]
    fn prompt_config_cancel_on_eof() {
        let mut confirm = ScriptedConfirm::new(vec![None]);
        let mut out = Vec::new();
        assert_eq!(prompt_config(&mut confirm, &mut out), None);
    }

    // --- cmd_ignore / cmd_status / cmd_statusline ---

    #[test]
    fn ignore_toggles() {
        let (_t, paths) = env();
        write_state(&paths, &crate::state::current_date(2), 10.0, vec![], false);
        let mut out = Vec::new();
        let mut err = Vec::new();
        cmd_ignore(None, &paths, &mut out, &mut err);
        assert!(load_state(&paths, 2).ignored);
        let mut out2 = Vec::new();
        cmd_ignore(None, &paths, &mut out2, &mut err);
        assert!(!load_state(&paths, 2).ignored);
    }

    #[test]
    fn status_json_schema() {
        let (_t, paths) = env();
        std::fs::write(
            paths.config(),
            "reset_hour = 2\nnudge_thresholds_minutes = [30, 60, 90]\nnudge_style = \"gentle\"\n",
        )
        .unwrap();
        write_state(
            &paths,
            &crate::state::current_date(2),
            45.0,
            vec![30],
            false,
        );
        let mut out = Vec::new();
        cmd_status(
            None,
            &paths,
            true,
            false,
            false,
            dt(2026, 4, 14),
            &mut out,
            &mut Vec::new(),
        );
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text.lines().count(), 1);
        let data: serde_json::Value = serde_json::from_str(text.trim()).unwrap();
        assert_eq!(data["accumulated_minutes"], 45.0);
        assert_eq!(data["thresholds"], serde_json::json!([30, 60, 90]));
        assert_eq!(data["nudges_given"], serde_json::json!([30]));
        assert_eq!(data["next_threshold"], 60);
        assert_eq!(data["ignored"], false);
    }

    #[test]
    fn status_json_all_thresholds_crossed() {
        let (_t, paths) = env();
        std::fs::write(
            paths.config(),
            "reset_hour = 2\nnudge_thresholds_minutes = [30, 60]\nnudge_style = \"gentle\"\n",
        )
        .unwrap();
        write_state(
            &paths,
            &crate::state::current_date(2),
            75.0,
            vec![30, 60],
            false,
        );
        let mut out = Vec::new();
        cmd_status(
            None,
            &paths,
            true,
            false,
            false,
            dt(2026, 4, 14),
            &mut out,
            &mut Vec::new(),
        );
        let data: serde_json::Value =
            serde_json::from_str(String::from_utf8(out).unwrap().trim()).unwrap();
        assert_eq!(data["next_threshold"], serde_json::Value::Null);
    }

    #[test]
    fn statusline_basic() {
        let (_t, paths) = env();
        std::fs::write(
            paths.config(),
            "reset_hour = 2\nnudge_thresholds_minutes = [60, 120, 180, 240]\nnudge_style = \"gentle\"\n",
        )
        .unwrap();
        write_state(&paths, &crate::state::current_date(2), 45.0, vec![], false);
        let mut out = Vec::new();
        cmd_statusline(&paths, &mut out);
        assert_eq!(
            String::from_utf8(out).unwrap().trim(),
            "\u{1f9c4} 45m / 4h 00m"
        );
    }

    #[test]
    fn statusline_vampire_icon() {
        let (_t, paths) = env();
        std::fs::write(
            paths.config(),
            "reset_hour = 2\nnudge_thresholds_minutes = [60, 120]\nnudge_style = \"gentle\"\n",
        )
        .unwrap();
        write_state(&paths, &crate::state::current_date(2), 51.0, vec![], false);
        let mut out = Vec::new();
        cmd_statusline(&paths, &mut out);
        assert!(String::from_utf8(out).unwrap().starts_with('\u{1f9db}'));
    }

    #[test]
    fn statusline_ignored() {
        let (_t, paths) = env();
        std::fs::write(
            paths.config(),
            "reset_hour = 2\nnudge_thresholds_minutes = [60, 120]\nnudge_style = \"gentle\"\n",
        )
        .unwrap();
        write_state(&paths, &crate::state::current_date(2), 30.0, vec![], true);
        let mut out = Vec::new();
        cmd_statusline(&paths, &mut out);
        assert_eq!(
            String::from_utf8(out).unwrap().trim(),
            "\u{1f9c4} 30m / 2h 00m (paused)"
        );
    }

    #[test]
    fn statusline_no_thresholds() {
        let (_t, paths) = env();
        std::fs::write(
            paths.config(),
            "reset_hour = 2\nnudge_thresholds_minutes = []\nnudge_style = \"gentle\"\n",
        )
        .unwrap();
        write_state(&paths, &crate::state::current_date(2), 90.0, vec![], false);
        let mut out = Vec::new();
        cmd_statusline(&paths, &mut out);
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text.trim(), "\u{1f9db} 1h 30m");
        assert!(!text.contains('/'));
    }

    // --- cmd_stats ---

    fn dt(y: i32, m: u32, d: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(10, 0, 0)
            .unwrap()
    }

    fn write_state_with_history(
        paths: &Paths,
        today: &str,
        accumulated: f64,
        history: Vec<(&str, f64)>,
    ) {
        let state = State {
            date: today.to_string(),
            accumulated_minutes: accumulated,
            history: history
                .into_iter()
                .map(|(d, m)| crate::state::HistoryEntry {
                    date: d.to_string(),
                    minutes: m,
                })
                .collect(),
            ..State::default()
        };
        save_state(paths, &state).unwrap();
    }

    #[test]
    fn stats_empty_history() {
        let (_t, paths) = env();
        write_state_with_history(&paths, "2026-04-14", 0.0, vec![]);
        let mut out = Vec::new();
        cmd_status(
            None,
            &paths,
            false,
            false,
            true,
            dt(2026, 4, 14),
            &mut out,
            &mut Vec::new(),
        );
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("April 2026"));
        assert!(text.contains("This month:"));
        assert!(text.contains("0 active days"));
        assert!(text.contains("Current streak:"));
        assert!(text.contains("0 days"));
        assert!(!text.contains("Busiest day:"));
        assert!(text.contains("last 30 days"));
    }

    #[test]
    fn stats_aggregates_and_streaks() {
        let (_t, paths) = env();
        write_state_with_history(
            &paths,
            "2026-04-14",
            60.0,
            vec![
                ("2026-04-10", 30.0),
                ("2026-04-12", 90.0),
                ("2026-04-13", 180.0),
            ],
        );
        let mut out = Vec::new();
        cmd_status(
            None,
            &paths,
            false,
            false,
            true,
            dt(2026, 4, 14),
            &mut out,
            &mut Vec::new(),
        );
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("6h 00m")); // month total
        assert!(text.contains("4 active days"));
        assert!(text.contains("3h 00m")); // busiest
        assert!(text.contains("Mon Apr 13"));
        assert!(text.contains("Current streak:      3 days"));
        assert!(text.contains("Longest streak:      3 days"));
    }

    #[test]
    fn stats_current_streak_excludes_today_when_zero() {
        let (_t, paths) = env();
        write_state_with_history(
            &paths,
            "2026-04-14",
            0.0,
            vec![("2026-04-12", 60.0), ("2026-04-13", 60.0)],
        );
        let mut out = Vec::new();
        cmd_status(
            None,
            &paths,
            false,
            false,
            true,
            dt(2026, 4, 14),
            &mut out,
            &mut Vec::new(),
        );
        assert!(String::from_utf8(out)
            .unwrap()
            .contains("Current streak:      2 days"));
    }

    #[test]
    fn stats_rolling_30d_window() {
        let (_t, paths) = env();
        write_state_with_history(
            &paths,
            "2026-04-14",
            0.0,
            vec![("2026-03-05", 120.0), ("2026-04-04", 60.0)],
        );
        let mut out = Vec::new();
        cmd_status(
            None,
            &paths,
            false,
            false,
            true,
            dt(2026, 4, 14),
            &mut out,
            &mut Vec::new(),
        );
        let text = String::from_utf8(out).unwrap();
        let rolling = text.lines().find(|l| l.contains("Rolling 30d")).unwrap();
        assert!(rolling.contains("1h 00m"));
        assert!(rolling.contains("1 active day"));
    }

    #[test]
    fn week_renders_seven_days() {
        let (_t, paths) = env();
        std::fs::write(
            paths.config(),
            "reset_hour = 2\nnudge_thresholds_minutes = [60, 120, 180, 240]\nnudge_style = \"gentle\"\n",
        )
        .unwrap();
        write_state_with_history(&paths, "2026-04-14", 60.0, vec![("2026-04-13", 120.0)]);
        let mut out = Vec::new();
        cmd_status(
            None,
            &paths,
            false,
            true,
            false,
            dt(2026, 4, 14),
            &mut out,
            &mut Vec::new(),
        );
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("Weekly usage (last 7 days)"));
        assert!(text.contains("today"));
        assert!(text.contains("Total:"));
    }

    // --- cmd_setup ---

    fn claude_env(tmp: &TempDir) -> ClaudePaths {
        ClaudePaths::with_dir(tmp.path().join(".claude"))
    }

    #[test]
    fn setup_yes_skips_prompts() {
        let (tmp, paths) = env();
        let claude = claude_env(&tmp);
        let mut confirm = ScriptedConfirm::new(vec![]);
        let mut out = Vec::new();
        let code = cmd_setup(&paths, &claude, false, true, false, &mut confirm, &mut out);
        assert_eq!(code, 0);
        assert!(String::from_utf8(out).unwrap().contains("hooks installed"));
        assert!(claude.settings().exists());
    }

    #[test]
    fn setup_defaults_with_yes() {
        let (tmp, paths) = env();
        let claude = claude_env(&tmp);
        std::fs::write(paths.config(), "nudge_style = \"spicy\"\n").unwrap();
        let mut confirm = ScriptedConfirm::new(vec![]);
        let mut out = Vec::new();
        cmd_setup(&paths, &claude, false, true, true, &mut confirm, &mut out);
        assert!(String::from_utf8(out)
            .unwrap()
            .contains("config reset to built-in defaults"));
        assert_eq!(load_config(&paths), Config::default());
    }

    #[test]
    fn setup_defaults_declined() {
        let (tmp, paths) = env();
        let claude = claude_env(&tmp);
        let mut confirm = ScriptedConfirm::new(vec![Some("n")]);
        let mut out = Vec::new();
        cmd_setup(&paths, &claude, false, false, true, &mut confirm, &mut out);
        assert!(String::from_utf8(out).unwrap().contains("setup cancelled"));
        assert!(!claude.settings().exists());
    }

    #[test]
    fn setup_interactive_passes_overrides() {
        let (tmp, paths) = env();
        let claude = claude_env(&tmp);
        let mut confirm = ScriptedConfirm::new(vec![Some("60"), Some(""), Some(""), Some("firm")]);
        let mut out = Vec::new();
        cmd_setup(&paths, &claude, false, false, false, &mut confirm, &mut out);
        let config = load_config(&paths);
        assert_eq!(config.nudge_thresholds_minutes, vec![60, 120, 180, 240]);
        assert_eq!(config.nudge_style, "firm");
    }
}
