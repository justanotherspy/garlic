//! Axum HTTP handlers.
//!
//! Mutating endpoints take an optional JSON body carrying the client's
//! [`TimeConfig`]; an empty body means "use garlic's defaults". Every state
//! operation applies the daily rollover first (mirroring `load_state`) and
//! runs inside the storage lock.

use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::auth::authenticate;
use crate::engine;
use crate::error::AppError;
use crate::model::{State as GarlicState, TimeConfig};
use crate::AppState;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Uniform response for every state endpoint. `crossed_threshold` and
/// `bedtime` are only meaningful for the prompt event (null/false otherwise).
#[derive(Debug, Serialize)]
pub struct StateResponse {
    pub state: GarlicState,
    pub crossed_threshold: Option<i64>,
    pub bedtime: bool,
}

impl StateResponse {
    fn plain(state: GarlicState) -> Self {
        StateResponse {
            state,
            crossed_threshold: None,
            bedtime: false,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct StateQuery {
    pub reset_hour: Option<u32>,
}

#[derive(Debug, Default, Deserialize)]
pub struct IgnoreRequest {
    #[serde(flatten)]
    pub config: TimeConfig,
    /// Explicit value; when omitted the flag is toggled.
    pub set: Option<bool>,
}

fn parse_config(body: &Bytes) -> Result<TimeConfig, AppError> {
    if body.is_empty() {
        return Ok(TimeConfig::default());
    }
    serde_json::from_slice(body)
        .map_err(|e| AppError::BadRequest(format!("invalid JSON body: {e}")))
}

fn parse_ignore(body: &Bytes) -> Result<IgnoreRequest, AppError> {
    if body.is_empty() {
        return Ok(IgnoreRequest::default());
    }
    serde_json::from_slice(body)
        .map_err(|e| AppError::BadRequest(format!("invalid JSON body: {e}")))
}

/// `GET /health` — liveness plus Redis reachability.
pub async fn health(State(app): State<AppState>) -> Response {
    let redis_ok = app.store.ping().await.is_ok();
    let status = if redis_ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    let body = Json(json!({
        "status": if redis_ok { "ok" } else { "degraded" },
        "redis": if redis_ok { "ok" } else { "down" },
        "version": VERSION,
    }));
    (status, body).into_response()
}

/// `GET /version` — backend name and version.
pub async fn version() -> Json<serde_json::Value> {
    Json(json!({ "name": "garlic-backend", "version": VERSION }))
}

/// `GET /v1/state?reset_hour=N` — current state, applying any daily rollover.
pub async fn get_state(
    State(app): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<StateQuery>,
) -> Result<Json<StateResponse>, AppError> {
    let ns = authenticate(&headers, &app.config)?;
    let reset_hour = q.reset_hour.unwrap_or(2);
    let clock = app.clock.clone();
    let (state, _) = app
        .store
        .mutate(&ns, move |s| {
            engine::apply_rollover(s, reset_hour, clock.as_ref());
        })
        .await?;
    Ok(Json(StateResponse::plain(state)))
}

/// `POST /v1/events/session-start` — record a session-start timestamp.
pub async fn post_session_start(
    State(app): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<StateResponse>, AppError> {
    let ns = authenticate(&headers, &app.config)?;
    let config = parse_config(&body)?;
    let clock = app.clock.clone();
    let (state, _) = app
        .store
        .mutate(&ns, move |s| {
            engine::apply_rollover(s, config.reset_hour, clock.as_ref());
            engine::apply_session_start(s, clock.as_ref());
        })
        .await?;
    Ok(Json(StateResponse::plain(state)))
}

/// `POST /v1/events/prompt` — accumulate thinking time and report nudges.
pub async fn post_prompt(
    State(app): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<StateResponse>, AppError> {
    let ns = authenticate(&headers, &app.config)?;
    let config = parse_config(&body)?;
    let clock = app.clock.clone();
    let (state, (crossed, bedtime)) = app
        .store
        .mutate(&ns, move |s| {
            engine::apply_rollover(s, config.reset_hour, clock.as_ref());
            let crossed = engine::apply_prompt(s, &config, clock.as_ref());
            // Mirror hook_prompt: only consider bedtime when no threshold was
            // crossed and nudging isn't paused (check_bedtime has a side effect).
            let bedtime = if crossed.is_none() && !s.ignored {
                engine::check_bedtime(s, &config, clock.as_ref())
            } else {
                false
            };
            (crossed, bedtime)
        })
        .await?;
    Ok(Json(StateResponse {
        state,
        crossed_threshold: crossed,
        bedtime,
    }))
}

/// `POST /v1/events/stop` — accumulate generation time.
pub async fn post_stop(
    State(app): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<StateResponse>, AppError> {
    let ns = authenticate(&headers, &app.config)?;
    let config = parse_config(&body)?;
    let clock = app.clock.clone();
    let (state, _) = app
        .store
        .mutate(&ns, move |s| {
            engine::apply_rollover(s, config.reset_hour, clock.as_ref());
            engine::apply_stop(s, &config, clock.as_ref());
        })
        .await?;
    Ok(Json(StateResponse::plain(state)))
}

/// `POST /v1/events/session-end` — finalize in-flight time, clear timestamp.
pub async fn post_session_end(
    State(app): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<StateResponse>, AppError> {
    let ns = authenticate(&headers, &app.config)?;
    let config = parse_config(&body)?;
    let clock = app.clock.clone();
    let (state, _) = app
        .store
        .mutate(&ns, move |s| {
            engine::apply_rollover(s, config.reset_hour, clock.as_ref());
            engine::apply_session_end(s, &config, clock.as_ref());
        })
        .await?;
    Ok(Json(StateResponse::plain(state)))
}

/// `POST /v1/ignore` — toggle (or set) the daily nudge-pause flag.
pub async fn post_ignore(
    State(app): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<StateResponse>, AppError> {
    let ns = authenticate(&headers, &app.config)?;
    let req = parse_ignore(&body)?;
    let clock = app.clock.clone();
    let reset_hour = req.config.reset_hour;
    let set = req.set;
    let (state, _) = app
        .store
        .mutate(&ns, move |s| {
            engine::apply_rollover(s, reset_hour, clock.as_ref());
            engine::set_ignored(s, set);
        })
        .await?;
    Ok(Json(StateResponse::plain(state)))
}

/// `POST /v1/reset` — zero today's timer (history is preserved).
pub async fn post_reset(
    State(app): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<StateResponse>, AppError> {
    let ns = authenticate(&headers, &app.config)?;
    let config = parse_config(&body)?;
    let clock = app.clock.clone();
    let (state, _) = app
        .store
        .mutate(&ns, move |s| {
            engine::apply_rollover(s, config.reset_hour, clock.as_ref());
            engine::reset_daily(s);
        })
        .await?;
    Ok(Json(StateResponse::plain(state)))
}
