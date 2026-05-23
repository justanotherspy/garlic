//! Bearer-token authentication and namespace derivation.
//!
//! A token is accepted only if it appears in the server's configured allowlist.
//! The Redis namespace is the SHA-256 hex of the token, so raw tokens never
//! appear in keys and each token gets fully isolated state.

use axum::http::HeaderMap;
use sha2::{Digest, Sha256};

use crate::config::AppConfig;
use crate::error::AppError;

/// Validate the `Authorization: Bearer <token>` header against the allowlist
/// and return the caller's storage namespace.
pub fn authenticate(headers: &HeaderMap, config: &AppConfig) -> Result<String, AppError> {
    let header = headers
        .get(axum::http::header::AUTHORIZATION)
        .ok_or(AppError::MissingToken)?
        .to_str()
        .map_err(|_| AppError::MissingToken)?;

    let token = header
        .strip_prefix("Bearer ")
        .or_else(|| header.strip_prefix("bearer "))
        .ok_or(AppError::MissingToken)?
        .trim();

    if token.is_empty() || !config.tokens.contains(token) {
        return Err(AppError::InvalidToken);
    }

    Ok(namespace_for(token))
}

/// SHA-256 hex digest of the token, used as the per-user key namespace.
pub fn namespace_for(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::net::SocketAddr;

    fn config_with(tokens: &[&str]) -> AppConfig {
        AppConfig {
            bind_addr: "0.0.0.0:8080".parse::<SocketAddr>().unwrap(),
            redis_url: "redis://localhost".to_string(),
            tokens: tokens.iter().map(|t| t.to_string()).collect::<HashSet<_>>(),
            tls_cert: None,
            tls_key: None,
        }
    }

    #[test]
    fn accepts_valid_bearer_token() {
        let cfg = config_with(&["secret"]);
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            "Bearer secret".parse().unwrap(),
        );
        let ns = authenticate(&headers, &cfg).unwrap();
        assert_eq!(ns, namespace_for("secret"));
        assert_eq!(ns.len(), 64); // sha256 hex
    }

    #[test]
    fn rejects_unknown_token() {
        let cfg = config_with(&["secret"]);
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            "Bearer nope".parse().unwrap(),
        );
        assert!(matches!(
            authenticate(&headers, &cfg),
            Err(AppError::InvalidToken)
        ));
    }

    #[test]
    fn rejects_missing_header() {
        let cfg = config_with(&["secret"]);
        let headers = HeaderMap::new();
        assert!(matches!(
            authenticate(&headers, &cfg),
            Err(AppError::MissingToken)
        ));
    }

    #[test]
    fn distinct_tokens_get_distinct_namespaces() {
        assert_ne!(namespace_for("a"), namespace_for("b"));
    }
}
