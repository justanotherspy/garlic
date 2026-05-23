//! garlic-backend binary entrypoint.
//!
//! Serves plain HTTP. For HTTPS, run behind a TLS-terminating reverse proxy
//! (see `backend/README.md` and the `caddy` service in `docker-compose.yml`).

use std::sync::Arc;

use garlic_backend::config::AppConfig;
use garlic_backend::engine::SystemClock;
use garlic_backend::store::{RedisStore, Store};
use garlic_backend::{build_router, AppState};

#[tokio::main]
async fn main() {
    init_tracing();

    let config = match AppConfig::from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("garlic-backend: configuration error: {e}");
            std::process::exit(2);
        }
    };

    let conn = match build_redis(&config.redis_url).await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("garlic-backend: failed to connect to redis: {e}");
            std::process::exit(1);
        }
    };

    let bind_addr = config.bind_addr;
    let state = AppState {
        store: Arc::new(Store::Redis(RedisStore::new(conn))),
        config: Arc::new(config),
        clock: Arc::new(SystemClock),
    };
    let app = build_router(state);

    let listener = match tokio::net::TcpListener::bind(bind_addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("garlic-backend: failed to bind {bind_addr}: {e}");
            std::process::exit(1);
        }
    };

    tracing::info!(
        "garlic-backend {} listening on http://{bind_addr}",
        env!("CARGO_PKG_VERSION")
    );

    let result = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await;

    if let Err(e) = result {
        eprintln!("garlic-backend: server error: {e}");
        std::process::exit(1);
    }
}

async fn build_redis(url: &str) -> redis::RedisResult<redis::aio::ConnectionManager> {
    let client = redis::Client::open(url)?;
    redis::aio::ConnectionManager::new(client).await
}

fn init_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    fmt().with_env_filter(filter).init();
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{signal, SignalKind};
        if let Ok(mut s) = signal(SignalKind::terminate()) {
            s.recv().await;
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("shutdown signal received; draining connections");
}
