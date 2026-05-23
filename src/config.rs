//! Load and create `config.toml` with defaults.

use std::fs;

use serde::{Deserialize, Serialize};

use crate::paths::Paths;

pub const VALID_NUDGE_STYLES: [&str; 3] = ["gentle", "firm", "spicy"];

/// Config keys in the canonical order they are written to disk. Used for the
/// "unknown config key" error message so it matches the documented defaults.
pub const CONFIG_KEYS: [&str; 5] = [
    "max_prompt_gap_minutes",
    "max_generation_minutes",
    "reset_hour",
    "nudge_thresholds_minutes",
    "nudge_style",
];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_max_prompt_gap_minutes")]
    pub max_prompt_gap_minutes: i64,
    #[serde(default = "default_max_generation_minutes")]
    pub max_generation_minutes: i64,
    #[serde(default = "default_reset_hour")]
    pub reset_hour: i64,
    #[serde(default = "default_thresholds")]
    pub nudge_thresholds_minutes: Vec<i64>,
    #[serde(default = "default_nudge_style")]
    pub nudge_style: String,
}

fn default_max_prompt_gap_minutes() -> i64 {
    40
}
fn default_max_generation_minutes() -> i64 {
    120
}
fn default_reset_hour() -> i64 {
    2
}
fn default_thresholds() -> Vec<i64> {
    vec![30, 60, 90, 120, 150, 180, 210, 240]
}
fn default_nudge_style() -> String {
    "gentle".to_string()
}

impl Default for Config {
    fn default() -> Config {
        Config {
            max_prompt_gap_minutes: default_max_prompt_gap_minutes(),
            max_generation_minutes: default_max_generation_minutes(),
            reset_hour: default_reset_hour(),
            nudge_thresholds_minutes: default_thresholds(),
            nudge_style: default_nudge_style(),
        }
    }
}

impl Config {
    /// Serialize to the flat TOML layout garlic has always written, so the file
    /// stays human-readable and diff-friendly and matches the documented
    /// format.
    pub fn to_toml(&self) -> String {
        let thresholds = self
            .nudge_thresholds_minutes
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "max_prompt_gap_minutes = {}\n\
             max_generation_minutes = {}\n\
             reset_hour = {}\n\
             nudge_thresholds_minutes = [{}]\n\
             nudge_style = \"{}\"\n",
            self.max_prompt_gap_minutes,
            self.max_generation_minutes,
            self.reset_hour,
            thresholds,
            self.nudge_style,
        )
    }
}

/// Write config to disk, creating the garlic directory if needed.
pub fn save_config(paths: &Paths, config: &Config) -> std::io::Result<()> {
    fs::create_dir_all(paths.dir())?;
    fs::write(paths.config(), config.to_toml())
}

/// Load config from disk, creating it with defaults if missing.
///
/// If the config file is corrupt or has invalid TOML syntax, overwrite it with
/// defaults and continue. This ensures hooks don't break after upgrades that
/// change the config format. Missing keys are filled from defaults.
pub fn load_config(paths: &Paths) -> Config {
    let path = paths.config();
    let contents = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => {
            // Missing (or unreadable) — write defaults and return them.
            let config = Config::default();
            let _ = save_config(paths, &config);
            return config;
        }
    };

    match toml::from_str::<Config>(&contents) {
        Ok(config) => config,
        Err(_) => {
            // Corrupt or old syntax — overwrite with defaults.
            let config = Config::default();
            let _ = save_config(paths, &config);
            config
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn paths() -> (TempDir, Paths) {
        let tmp = TempDir::new().unwrap();
        let paths = Paths::with_dir(tmp.path().join(".garlic"));
        std::fs::create_dir_all(paths.dir()).unwrap();
        (tmp, paths)
    }

    #[test]
    fn creates_default_when_missing() {
        let (_tmp, paths) = paths();
        let config = load_config(&paths);
        assert_eq!(config, Config::default());
        assert!(paths.config().exists());

        // Round-trips back to defaults.
        let on_disk: Config =
            toml::from_str(&std::fs::read_to_string(paths.config()).unwrap()).unwrap();
        assert_eq!(on_disk, Config::default());
    }

    #[test]
    fn reads_existing_and_merges_defaults() {
        let (_tmp, paths) = paths();
        std::fs::write(paths.config(), "nudge_style = \"spicy\"\n").unwrap();

        let config = load_config(&paths);
        assert_eq!(config.nudge_style, "spicy");
        // Defaults still present for unspecified keys.
        assert_eq!(config.max_prompt_gap_minutes, 40);
        assert_eq!(
            config.nudge_thresholds_minutes,
            vec![30, 60, 90, 120, 150, 180, 210, 240]
        );
    }

    #[test]
    fn user_overrides_all() {
        let (_tmp, paths) = paths();
        std::fs::write(
            paths.config(),
            "max_prompt_gap_minutes = 5\n\
             reset_hour = 4\n\
             nudge_thresholds_minutes = [30, 60]\n\
             nudge_style = \"firm\"\n",
        )
        .unwrap();

        let config = load_config(&paths);
        assert_eq!(config.max_prompt_gap_minutes, 5);
        assert_eq!(config.reset_hour, 4);
        assert_eq!(config.nudge_thresholds_minutes, vec![30, 60]);
        assert_eq!(config.nudge_style, "firm");
    }

    #[test]
    fn handles_corrupted_file() {
        let (_tmp, paths) = paths();
        std::fs::write(
            paths.config(),
            "nudge_style = \"spicy\"\n\
             max_prompt_gap_minutes = 60\n\
             bad_line_no_equals\n\
             reset_hour = 4\n",
        )
        .unwrap();

        let config = load_config(&paths);
        assert_eq!(config, Config::default());
        // File overwritten with valid defaults.
        let on_disk: Config =
            toml::from_str(&std::fs::read_to_string(paths.config()).unwrap()).unwrap();
        assert_eq!(on_disk, Config::default());
    }

    #[test]
    fn default_toml_round_trips() {
        let config = Config::default();
        let parsed: Config = toml::from_str(&config.to_toml()).unwrap();
        assert_eq!(parsed, config);
    }
}
