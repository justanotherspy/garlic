//! Serializable state and config types.
//!
//! `State` mirrors garlic's `state.toml` exactly (same field names) so the
//! CLI can map responses onto its existing model. `TimeConfig` carries the
//! subset of garlic's `config.toml` that governs time accounting; the client
//! sends it with each request so the backend stays purely about *state*.

use serde::{Deserialize, Serialize};

/// Number of past days of per-day totals retained in `history`.
pub const HISTORY_MAX: usize = 30;

/// A single archived day, mirroring garlic's history entries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub date: String,
    pub minutes: f64,
}

/// Per-user daily tracking state. Field names match garlic's `state.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct State {
    pub date: String,
    pub accumulated_minutes: f64,
    pub last_event_time: f64,
    pub nudges_given: Vec<i64>,
    pub ignored: bool,
    pub bedtime_nudge_given: bool,
    pub history: Vec<HistoryEntry>,
}

impl Default for State {
    fn default() -> Self {
        State {
            date: String::new(),
            accumulated_minutes: 0.0,
            last_event_time: 0.0,
            nudges_given: Vec::new(),
            ignored: false,
            bedtime_nudge_given: false,
            history: Vec::new(),
        }
    }
}

fn default_max_prompt_gap() -> f64 {
    40.0
}
fn default_max_generation() -> f64 {
    120.0
}
fn default_reset_hour() -> u32 {
    2
}
fn default_thresholds() -> Vec<i64> {
    vec![30, 60, 90, 120, 150, 180, 210, 240]
}

/// Time-accounting parameters supplied by the client. Every field defaults to
/// garlic's built-in default, so a request body of `{}` is valid and behaves
/// exactly like an unconfigured garlic install.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeConfig {
    #[serde(default = "default_max_prompt_gap")]
    pub max_prompt_gap_minutes: f64,
    #[serde(default = "default_max_generation")]
    pub max_generation_minutes: f64,
    #[serde(default = "default_reset_hour")]
    pub reset_hour: u32,
    #[serde(default = "default_thresholds")]
    pub nudge_thresholds_minutes: Vec<i64>,
}

impl Default for TimeConfig {
    fn default() -> Self {
        TimeConfig {
            max_prompt_gap_minutes: default_max_prompt_gap(),
            max_generation_minutes: default_max_generation(),
            reset_hour: default_reset_hour(),
            nudge_thresholds_minutes: default_thresholds(),
        }
    }
}
