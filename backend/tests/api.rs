//! End-to-end tests that drive the real router through the in-memory store
//! with an injectable clock, so we can assert behavior without Redis or sleeps.

use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::body::{to_bytes, Body};
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use chrono::{NaiveDate, NaiveDateTime};
use serde_json::{json, Value};
use tower::ServiceExt;

use garlic_backend::config::AppConfig;
use garlic_backend::engine::Clock;
use garlic_backend::store::{MemoryStore, Store};
use garlic_backend::{build_router, AppState};

const TOKEN: &str = "test-token";
const OTHER_TOKEN: &str = "other-token";

struct TestClock {
    unix: Mutex<f64>,
    local: Mutex<NaiveDateTime>,
}

impl TestClock {
    fn new(unix: f64, local: NaiveDateTime) -> Self {
        TestClock {
            unix: Mutex::new(unix),
            local: Mutex::new(local),
        }
    }
    fn set_unix(&self, v: f64) {
        *self.unix.lock().unwrap() = v;
    }
    fn advance_secs(&self, secs: f64) {
        *self.unix.lock().unwrap() += secs;
    }
    fn set_local(&self, dt: NaiveDateTime) {
        *self.local.lock().unwrap() = dt;
    }
}

impl Clock for TestClock {
    fn now_unix(&self) -> f64 {
        *self.unix.lock().unwrap()
    }
    fn local_now(&self) -> NaiveDateTime {
        *self.local.lock().unwrap()
    }
}

fn local_at(y: i32, m: u32, d: u32, h: u32, min: u32) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(y, m, d)
        .unwrap()
        .and_hms_opt(h, min, 0)
        .unwrap()
}

struct Harness {
    app: Router,
    clock: Arc<TestClock>,
}

fn harness() -> Harness {
    let clock = Arc::new(TestClock::new(1_000.0, local_at(2026, 3, 16, 12, 0)));
    let dyn_clock: Arc<dyn Clock> = clock.clone();
    let config = AppConfig {
        bind_addr: "0.0.0.0:8080".parse::<SocketAddr>().unwrap(),
        redis_url: "redis://unused".to_string(),
        tokens: [TOKEN, OTHER_TOKEN]
            .iter()
            .map(|s| s.to_string())
            .collect::<HashSet<_>>(),
        tls_cert: None,
        tls_key: None,
    };
    let state = AppState {
        store: Arc::new(Store::Memory(MemoryStore::new())),
        config: Arc::new(config),
        clock: dyn_clock,
    };
    Harness {
        app: build_router(state),
        clock,
    }
}

async fn call(
    app: &Router,
    method: Method,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    let req = match body {
        Some(v) => builder
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(v.to_string()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    };
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let value: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, value)
}

async fn post(app: &Router, uri: &str, body: Value) -> (StatusCode, Value) {
    call(app, Method::POST, uri, Some(TOKEN), Some(body)).await
}

#[tokio::test]
async fn health_reports_ok() {
    let h = harness();
    let (status, body) = call(&h.app, Method::GET, "/health", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");
    assert_eq!(body["redis"], "ok");
}

#[tokio::test]
async fn version_endpoint() {
    let h = harness();
    let (status, body) = call(&h.app, Method::GET, "/version", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"], "garlic-backend");
    assert!(body["version"].is_string());
}

#[tokio::test]
async fn requires_auth() {
    let h = harness();
    let (status, _) = call(&h.app, Method::GET, "/v1/state", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, _) = call(
        &h.app,
        Method::POST,
        "/v1/events/prompt",
        Some("wrong"),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn prompt_accumulates_thinking_gap() {
    let h = harness();
    // session-start stamps last_event_time at t=1000
    let (status, _) = post(&h.app, "/v1/events/session-start", json!({})).await;
    assert_eq!(status, StatusCode::OK);

    // 5 minutes pass, then a prompt arrives
    h.clock.advance_secs(300.0);
    let (status, body) = post(&h.app, "/v1/events/prompt", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    let mins = body["state"]["accumulated_minutes"].as_f64().unwrap();
    assert!((mins - 5.0).abs() < 0.01, "expected ~5m, got {mins}");
    assert!(body["crossed_threshold"].is_null());
    assert_eq!(body["bedtime"], false);
}

#[tokio::test]
async fn prompt_reports_crossed_threshold() {
    let h = harness();
    let cfg = json!({ "nudge_thresholds_minutes": [1, 2], "max_prompt_gap_minutes": 40 });
    post(&h.app, "/v1/events/session-start", cfg.clone()).await;

    h.clock.advance_secs(90.0); // 1.5 minutes -> crosses the 1-minute threshold
    let (status, body) = post(&h.app, "/v1/events/prompt", cfg).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["crossed_threshold"], 1);
    assert_eq!(body["state"]["nudges_given"], json!([1]));
}

#[tokio::test]
async fn prompt_reports_bedtime_in_window() {
    let h = harness();
    // reset_hour 2 -> bedtime hour is 1
    h.clock.set_local(local_at(2026, 3, 16, 1, 30));
    post(&h.app, "/v1/events/session-start", json!({})).await;

    let (status, body) = post(&h.app, "/v1/events/prompt", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["bedtime"], true);
    assert_eq!(body["state"]["bedtime_nudge_given"], true);
}

#[tokio::test]
async fn stop_clamps_generation_time() {
    let h = harness();
    post(&h.app, "/v1/events/session-start", json!({})).await;
    // Prompt immediately (zero thinking gap), opening the agent interval.
    post(&h.app, "/v1/events/prompt", json!({})).await;
    // 150 minutes of generation elapse; default max_generation_minutes is 120.
    h.clock.advance_secs(150.0 * 60.0);
    let (status, body) = post(&h.app, "/v1/events/stop", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    let mins = body["state"]["accumulated_minutes"].as_f64().unwrap();
    assert!(
        (mins - 120.0).abs() < 0.01,
        "expected clamp to 120m, got {mins}"
    );
}

#[tokio::test]
async fn parallel_sessions_union_not_sum() {
    let h = harness();
    // Two sessions under one token, generating over overlapping windows.
    post(
        &h.app,
        "/v1/events/session-start",
        json!({ "session_id": "a" }),
    )
    .await;
    post(
        &h.app,
        "/v1/events/session-start",
        json!({ "session_id": "b" }),
    )
    .await;
    h.clock.advance_secs(60.0); // 1m thinking each
    post(&h.app, "/v1/events/prompt", json!({ "session_id": "a" })).await;
    post(&h.app, "/v1/events/prompt", json!({ "session_id": "b" })).await;
    h.clock.set_unix(1_000.0 + 300.0);
    post(&h.app, "/v1/events/stop", json!({ "session_id": "a" })).await; // agent a [60,300]
    h.clock.set_unix(1_000.0 + 360.0);
    let (status, body) = post(&h.app, "/v1/events/stop", json!({ "session_id": "b" })).await; // agent b [60,360]
    assert_eq!(status, StatusCode::OK);
    // Union wall-clock is 6m (0..360s), not the 11m a naive sum would give.
    let mins = body["state"]["accumulated_minutes"].as_f64().unwrap();
    assert!((mins - 6.0).abs() < 0.01, "expected union ~6m, got {mins}");
    // Intervals are tracked per session.
    let intervals = body["state"]["intervals"].as_array().unwrap();
    assert!(intervals.iter().any(|i| i["session_id"] == "a"));
    assert!(intervals.iter().any(|i| i["session_id"] == "b"));
}

#[tokio::test]
async fn intervals_merge_unions_across_clients() {
    let h = harness();
    // Client A pushes one 5-minute agent span for session "a".
    let (status, body) = post(
        &h.app,
        "/v1/intervals",
        json!({
            "intervals": [
                { "session_id": "a", "kind": "agent", "start": 1000.0, "end": 1300.0 }
            ]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!((body["state"]["accumulated_minutes"].as_f64().unwrap() - 5.0).abs() < 0.01);
    assert!(body["crossed_threshold"].is_null());

    // Client B (a different machine, different session) pushes an overlapping
    // span. The union is what matters, not the sum.
    let (_, body) = post(
        &h.app,
        "/v1/intervals",
        json!({
            "intervals": [
                { "session_id": "b", "kind": "agent", "start": 1180.0, "end": 1480.0 }
            ]
        }),
    )
    .await;
    // Union [1000,1480] = 480s = 8m (a sum would be 10m).
    let mins = body["state"]["accumulated_minutes"].as_f64().unwrap();
    assert!((mins - 8.0).abs() < 0.01, "expected union ~8m, got {mins}");

    // A re-push from client A (its full, unchanged set) is idempotent: B's
    // contribution is preserved, the total is unchanged.
    let (_, body) = post(
        &h.app,
        "/v1/intervals",
        json!({
            "intervals": [
                { "session_id": "a", "kind": "agent", "start": 1000.0, "end": 1300.0 }
            ]
        }),
    )
    .await;
    let mins = body["state"]["accumulated_minutes"].as_f64().unwrap();
    assert!(
        (mins - 8.0).abs() < 0.01,
        "re-push should be idempotent, got {mins}"
    );
    let sessions: Vec<&str> = body["state"]["intervals"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["session_id"].as_str().unwrap())
        .collect();
    assert!(sessions.contains(&"a") && sessions.contains(&"b"));
}

#[tokio::test]
async fn intervals_check_nudges_reports_threshold() {
    let h = harness();
    // A single 2-minute span with a 1-minute threshold and check_nudges set.
    let (status, body) = post(
        &h.app,
        "/v1/intervals",
        json!({
            "nudge_thresholds_minutes": [1, 2],
            "intervals": [
                { "session_id": "a", "kind": "agent", "start": 1000.0, "end": 1120.0 }
            ],
            "check_nudges": true
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["crossed_threshold"], 2);
    assert_eq!(body["state"]["nudges_given"], json!([2]));
}

#[tokio::test]
async fn intervals_plain_sync_leaves_nudges_untouched() {
    let h = harness();
    // Same crossing, but without check_nudges: total updates, no nudge fires.
    let (_, body) = post(
        &h.app,
        "/v1/intervals",
        json!({
            "nudge_thresholds_minutes": [1, 2],
            "intervals": [
                { "session_id": "a", "kind": "agent", "start": 1000.0, "end": 1120.0 }
            ]
        }),
    )
    .await;
    assert!((body["state"]["accumulated_minutes"].as_f64().unwrap() - 2.0).abs() < 0.01);
    assert!(body["crossed_threshold"].is_null());
    assert_eq!(body["state"]["nudges_given"], json!([]));
}

#[tokio::test]
async fn ignore_toggles_then_sets() {
    let h = harness();
    let (_, body) = post(&h.app, "/v1/ignore", json!({})).await;
    assert_eq!(body["state"]["ignored"], true);
    let (_, body) = post(&h.app, "/v1/ignore", json!({})).await;
    assert_eq!(body["state"]["ignored"], false);
    let (_, body) = post(&h.app, "/v1/ignore", json!({ "set": true })).await;
    assert_eq!(body["state"]["ignored"], true);
}

#[tokio::test]
async fn reset_zeroes_today() {
    let h = harness();
    post(&h.app, "/v1/events/session-start", json!({})).await;
    h.clock.advance_secs(600.0);
    post(&h.app, "/v1/events/stop", json!({})).await;
    let (_, body) = post(&h.app, "/v1/reset", json!({})).await;
    assert_eq!(body["state"]["accumulated_minutes"], 0.0);
    assert_eq!(body["state"]["last_event_time"], 0.0);
    assert_eq!(body["state"]["nudges_given"], json!([]));
}

#[tokio::test]
async fn daily_rollover_archives_previous_day() {
    let h = harness();
    // Day 1: accumulate ~10 minutes.
    post(&h.app, "/v1/events/session-start", json!({})).await;
    h.clock.advance_secs(600.0);
    post(&h.app, "/v1/events/stop", json!({})).await;

    // Cross into the next day and read state back.
    h.clock.set_local(local_at(2026, 3, 17, 12, 0));
    h.clock.advance_secs(60.0);
    let (status, body) = call(
        &h.app,
        Method::GET,
        "/v1/state?reset_hour=2",
        Some(TOKEN),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["state"]["date"], "2026-03-17");
    assert_eq!(body["state"]["accumulated_minutes"], 0.0);
    let history = body["state"]["history"].as_array().unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0]["date"], "2026-03-16");
    let archived = history[0]["minutes"].as_f64().unwrap();
    assert!(
        (archived - 10.0).abs() < 0.01,
        "expected ~10m archived, got {archived}"
    );
}

#[tokio::test]
async fn namespaces_are_isolated() {
    let h = harness();
    // Accumulate under TOKEN.
    post(&h.app, "/v1/events/session-start", json!({})).await;
    h.clock.advance_secs(300.0);
    post(&h.app, "/v1/events/prompt", json!({})).await;

    // OTHER_TOKEN sees a fresh state.
    let (status, body) = call(
        &h.app,
        Method::GET,
        "/v1/state?reset_hour=2",
        Some(OTHER_TOKEN),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["state"]["accumulated_minutes"], 0.0);
}

#[tokio::test]
async fn malformed_body_is_rejected() {
    let h = harness();
    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/events/prompt")
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{ this is not json"))
        .unwrap();
    let resp = h.app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn empty_body_uses_defaults() {
    let h = harness();
    // No Content-Type, no body at all.
    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/events/session-start")
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let resp = h.app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let _ = h.clock.now_unix(); // clock handle still usable
    h.clock.set_unix(2_000.0);
}
