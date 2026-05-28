//! Backend configuration, loaded from environment variables at startup.

use std::collections::HashSet;
use std::net::SocketAddr;
use std::path::PathBuf;

/// Resolved runtime configuration.
#[derive(Debug, Clone)]
pub struct AppConfig {
    /// Address to bind the HTTP(S) listener to.
    pub bind_addr: SocketAddr,
    /// Redis connection string (e.g. `redis://127.0.0.1:6379`).
    pub redis_url: String,
    /// Set of accepted bearer tokens. Each token maps to an isolated namespace.
    pub tokens: HashSet<String>,
    /// PEM certificate chain path; enables HTTPS when set together with the key.
    pub tls_cert: Option<PathBuf>,
    /// PEM private key path.
    pub tls_key: Option<PathBuf>,
}

impl AppConfig {
    /// Build configuration from the process environment.
    ///
    /// Required:
    /// - `GARLIC_REDIS_URL` (or `REDIS_URL`)
    /// - `GARLIC_AUTH_TOKENS` — comma-separated list of accepted tokens
    ///
    /// Optional:
    /// - `GARLIC_BIND` (default `0.0.0.0:8080`)
    /// - `GARLIC_TLS_CERT` + `GARLIC_TLS_KEY` — reserved for TLS configuration.
    ///   The binary itself serves plain HTTP and expects TLS to be terminated by
    ///   a reverse proxy (see `backend/README.md`); setting these only validates
    ///   that both are present and emits a startup warning.
    pub fn from_env() -> Result<Self, String> {
        let bind_raw = env_or("GARLIC_BIND", "0.0.0.0:8080");
        let bind_addr: SocketAddr = bind_raw
            .parse()
            .map_err(|e| format!("GARLIC_BIND is not a valid socket address ({bind_raw}): {e}"))?;

        let redis_url = std::env::var("GARLIC_REDIS_URL")
            .or_else(|_| std::env::var("REDIS_URL"))
            .map_err(|_| "GARLIC_REDIS_URL (or REDIS_URL) must be set".to_string())?;

        let tokens_raw = std::env::var("GARLIC_AUTH_TOKENS").map_err(|_| {
            "GARLIC_AUTH_TOKENS must be set (comma-separated bearer tokens)".to_string()
        })?;
        let tokens: HashSet<String> = tokens_raw
            .split(',')
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .collect();
        if tokens.is_empty() {
            return Err("GARLIC_AUTH_TOKENS contained no non-empty tokens".to_string());
        }

        let tls_cert = std::env::var("GARLIC_TLS_CERT").ok().map(PathBuf::from);
        let tls_key = std::env::var("GARLIC_TLS_KEY").ok().map(PathBuf::from);
        if tls_cert.is_some() != tls_key.is_some() {
            return Err(
                "GARLIC_TLS_CERT and GARLIC_TLS_KEY must both be set to enable HTTPS".to_string(),
            );
        }

        Ok(AppConfig {
            bind_addr,
            redis_url,
            tokens,
            tls_cert,
            tls_key,
        })
    }

    /// Whether native TLS is configured.
    pub fn tls_enabled(&self) -> bool {
        self.tls_cert.is_some() && self.tls_key.is_some()
    }
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}
