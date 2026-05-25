//! garlic-backend: a self-hostable HTTP state service for the garlic CLI.
//!
//! The backend owns garlic's *state* (the thing that must be shared across
//! agents and environments); the *config* that governs time accounting stays
//! with the client and is sent on each request. Redis is the data layer, so a
//! single small Redis instance is all that's needed to self-host.

pub mod auth;
pub mod config;
pub mod engine;
pub mod error;
pub mod model;
pub mod routes;
pub mod store;

use std::sync::Arc;

use axum::routing::{get, post};
use axum::Router;

use crate::config::AppConfig;
use crate::engine::Clock;
use crate::store::Store;

/// Shared application state handed to every handler.
#[derive(Clone)]
pub struct AppState {
    pub store: Arc<Store>,
    pub config: Arc<AppConfig>,
    pub clock: Arc<dyn Clock>,
}

/// Build the full router with all routes wired to `state`.
pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(routes::health))
        .route("/version", get(routes::version))
        .route("/v1/state", get(routes::get_state))
        .route("/v1/events/session-start", post(routes::post_session_start))
        .route("/v1/events/prompt", post(routes::post_prompt))
        .route("/v1/events/stop", post(routes::post_stop))
        .route("/v1/events/session-end", post(routes::post_session_end))
        .route("/v1/intervals", post(routes::post_intervals))
        .route("/v1/ignore", post(routes::post_ignore))
        .route("/v1/reset", post(routes::post_reset))
        .with_state(state)
}
