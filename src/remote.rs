//! Client for the optional self-hosted garlic backend.
//!
//! When `GARLIC_URL` and `GARLIC_TOKEN` are both set, garlic routes its state
//! through the backend instead of the local `state.toml`, so several machines
//! and ephemeral cloud environments can share one daily total. The backend owns
//! *state*; the *config* that governs time accounting stays on the client and
//! is sent with each request (see `backend/README.md`).
//!
//! The server clock is authoritative, so the client never sends timestamps —
//! it just reports which event happened and lets the backend account for time.

use std::time::Duration;

use serde::Deserialize;

use crate::config::Config;
use crate::intervals::Interval;
use crate::state::State;

/// Per-request timeout. Hooks run on every prompt, so this is kept short: a
/// slow or unreachable backend must never stall a Claude Code session.
const TIMEOUT_SECS: u64 = 5;

/// A configured connection to a garlic backend.
pub struct Remote {
    base_url: String,
    token: String,
    agent: ureq::Agent,
}

/// The envelope every backend state endpoint returns. `state` deserializes
/// straight into the CLI's [`State`] because the backend mirrors its field
/// names exactly.
#[derive(Debug, Deserialize)]
pub struct RemoteResponse {
    pub state: State,
    #[serde(default)]
    pub crossed_threshold: Option<i64>,
    #[serde(default)]
    pub bedtime: bool,
}

impl Remote {
    /// Build a client from the environment, or `None` when shared-state mode is
    /// not configured (either variable unset or empty).
    pub fn from_env() -> Option<Remote> {
        let url = std::env::var("GARLIC_URL").ok()?;
        let token = std::env::var("GARLIC_TOKEN").ok()?;
        Remote::new(url.trim(), token.trim())
    }

    /// Build a client for an explicit URL and token (used by tests). Returns
    /// `None` if either is empty.
    pub fn new(base_url: &str, token: &str) -> Option<Remote> {
        let base_url = base_url.trim_end_matches('/').to_string();
        if base_url.is_empty() || token.is_empty() {
            return None;
        }
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(TIMEOUT_SECS)))
            .build()
            .into();
        Some(Remote {
            base_url,
            token: token.to_string(),
            agent,
        })
    }

    /// The subset of config the backend understands, as a JSON body.
    fn config_body(config: &Config) -> serde_json::Value {
        serde_json::json!({
            "max_prompt_gap_minutes": config.max_prompt_gap_minutes,
            "max_generation_minutes": config.max_generation_minutes,
            "reset_hour": config.reset_hour,
            "nudge_thresholds_minutes": config.nudge_thresholds_minutes,
        })
    }

    fn auth(&self) -> String {
        format!("Bearer {}", self.token)
    }

    /// `GET /v1/state` — current state, applying any daily rollover server-side.
    pub fn get_state(&self, reset_hour: i64) -> Result<RemoteResponse, String> {
        let url = format!("{}/v1/state?reset_hour={reset_hour}", self.base_url);
        let text = self
            .agent
            .get(&url)
            .header("Authorization", &self.auth())
            .call()
            .map_err(|e| format!("{url}: {e}"))?
            .body_mut()
            .read_to_string()
            .map_err(|e| format!("{url}: {e}"))?;
        parse_body(&url, &text)
    }

    /// `POST /v1/intervals` — merge this client's locally-accounted closed
    /// intervals into the shared total. This is the local-first sync path:
    /// hooks account time offline and the daily total is reconciled here.
    ///
    /// `check_nudges` asks the backend to evaluate thresholds / bedtime against
    /// the merged total (set only for a prompt in synchronous mode); a plain
    /// sync leaves it `false`.
    pub fn push_intervals(
        &self,
        config: &Config,
        intervals: &[Interval],
        check_nudges: bool,
    ) -> Result<RemoteResponse, String> {
        let mut body = Self::config_body(config);
        body["intervals"] =
            serde_json::to_value(intervals).unwrap_or_else(|_| serde_json::json!([]));
        body["check_nudges"] = serde_json::json!(check_nudges);
        self.post("/v1/intervals", body)
    }

    pub fn reset(&self, config: &Config) -> Result<RemoteResponse, String> {
        self.post("/v1/reset", Self::config_body(config))
    }

    /// `POST /v1/ignore`. `set` of `None` toggles the flag server-side.
    pub fn ignore(&self, config: &Config, set: Option<bool>) -> Result<RemoteResponse, String> {
        let mut body = Self::config_body(config);
        if let Some(value) = set {
            body["set"] = serde_json::json!(value);
        }
        self.post("/v1/ignore", body)
    }

    fn post(&self, path: &str, body: serde_json::Value) -> Result<RemoteResponse, String> {
        let url = format!("{}{path}", self.base_url);
        let text = self
            .agent
            .post(&url)
            .header("Authorization", &self.auth())
            .header("Content-Type", "application/json")
            .send(body.to_string())
            .map_err(|e| format!("{url}: {e}"))?
            .body_mut()
            .read_to_string()
            .map_err(|e| format!("{url}: {e}"))?;
        parse_body(&url, &text)
    }
}

fn parse_body(url: &str, text: &str) -> Result<RemoteResponse, String> {
    serde_json::from_str(text).map_err(|e| format!("{url}: malformed response: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;

    fn config() -> Config {
        Config::default()
    }

    #[test]
    fn from_env_requires_both_vars() {
        // new() is the env-independent core; empty inputs yield None.
        assert!(Remote::new("", "tok").is_none());
        assert!(Remote::new("http://x", "").is_none());
        assert!(Remote::new("http://x", "tok").is_some());
    }

    #[test]
    fn trims_trailing_slash_from_base_url() {
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(GET).path("/v1/state");
            then.status(200).body(state_body(10.0));
        });
        // Base URL with a trailing slash must not produce a double slash.
        let remote = Remote::new(&format!("{}/", server.base_url()), "tok").unwrap();
        let resp = remote.get_state(2).unwrap();
        m.assert();
        assert!((resp.state.accumulated_minutes - 10.0).abs() < 0.01);
    }

    #[test]
    fn push_intervals_sends_bearer_config_and_payload() {
        use crate::intervals::Kind;
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(POST)
                .path("/v1/intervals")
                .header("authorization", "Bearer secret")
                .json_body_includes(r#"{"check_nudges":true}"#)
                .json_body_includes(r#"{"nudge_thresholds_minutes":[30,60,90,120,150,180,210,240]}"#)
                .json_body_includes(r#"{"intervals":[{"session_id":"sess-1","kind":"agent"}]}"#);
            then.status(200).body(
                r#"{"state":{"date":"2026-05-23","accumulated_minutes":61.0,"last_event_time":1.0,"nudges_given":[60],"ignored":false,"bedtime_nudge_given":false,"history":[]},"crossed_threshold":60,"bedtime":false}"#,
            );
        });
        let remote = Remote::new(&server.base_url(), "secret").unwrap();
        let intervals = [Interval {
            session_id: "sess-1".to_string(),
            kind: Kind::Agent,
            start: 1000.0,
            end: 4660.0,
        }];
        let resp = remote.push_intervals(&config(), &intervals, true).unwrap();
        m.assert();
        assert_eq!(resp.crossed_threshold, Some(60));
        assert!(!resp.bedtime);
        assert_eq!(resp.state.nudges_given, vec![60]);
    }

    #[test]
    fn ignore_includes_set_when_given() {
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(POST)
                .path("/v1/ignore")
                .json_body_includes(r#"{"set":true}"#);
            then.status(200).body(state_body(0.0));
        });
        let remote = Remote::new(&server.base_url(), "tok").unwrap();
        remote.ignore(&config(), Some(true)).unwrap();
        m.assert();
    }

    #[test]
    fn http_error_status_is_reported() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/v1/intervals");
            then.status(401).body(r#"{"error":"invalid token"}"#);
        });
        let remote = Remote::new(&server.base_url(), "tok").unwrap();
        let err = remote.push_intervals(&config(), &[], false).unwrap_err();
        assert!(err.contains("/v1/intervals"), "got: {err}");
    }

    fn state_body(minutes: f64) -> String {
        format!(
            r#"{{"state":{{"date":"2026-05-23","accumulated_minutes":{minutes},"last_event_time":0.0,"nudges_given":[],"ignored":false,"bedtime_nudge_given":false,"history":[]}},"crossed_threshold":null,"bedtime":false}}"#
        )
    }
}
