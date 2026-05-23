//! Update check against crates.io, with a 24-hour on-disk cache.

use std::fs;
use std::path::Path;
use std::time::Duration;

use serde::Deserialize;

const VERSION_CACHE_TTL: f64 = 24.0 * 3600.0;
pub const CRATES_API_URL: &str = "https://crates.io/api/v1/crates/garlic-ward";
const USER_AGENT: &str = "garlic-ward (https://github.com/justanotherspy/garlic)";

#[derive(Deserialize, Default)]
struct VersionCache {
    #[serde(default)]
    checked_at: f64,
    #[serde(default)]
    latest_version: String,
}

/// Parse a PEP 440-ish / SemVer version string into a comparable vector.
pub fn parse_version(v: &str) -> Vec<u64> {
    v.split('.')
        .map(|seg| {
            let digits: String = seg.chars().take_while(char::is_ascii_digit).collect();
            digits.parse().unwrap_or(0)
        })
        .collect()
}

/// Fetch the latest `garlic-ward` version from crates.io. Returns it if newer
/// than `current`, else `None`.
///
/// The result is cached in `cache_path` for 24 hours so repeated invocations of
/// `garlic version` don't hit the network every time. `now` is the current Unix
/// time (injected for tests); `base_url` is the crates.io endpoint (overridable
/// for tests).
pub fn check_latest_version(
    current: &str,
    cache_path: &Path,
    base_url: &str,
    now: f64,
) -> Option<String> {
    // Return cached result if still fresh.
    if let Ok(text) = fs::read_to_string(cache_path) {
        if let Ok(cache) = toml::from_str::<VersionCache>(&text) {
            if now - cache.checked_at < VERSION_CACHE_TTL {
                return if cache.latest_version.is_empty() {
                    None
                } else {
                    Some(cache.latest_version)
                };
            }
        }
    }

    // Live network check.
    let mut latest_to_cache = String::new();
    let mut result = None;
    let mut network_ok = false;
    if let Some(latest) = fetch_latest(base_url) {
        network_ok = true;
        if parse_version(&latest) > parse_version(current) {
            latest_to_cache = latest.clone();
            result = Some(latest);
        }
    }

    // Persist so the next 24 h use the cache, not the network.
    if network_ok {
        if let Some(parent) = cache_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(
            cache_path,
            format!("checked_at = {now}\nlatest_version = \"{latest_to_cache}\"\n"),
        );
    }

    result
}

/// GET the crates.io endpoint and extract the latest stable version.
fn fetch_latest(base_url: &str) -> Option<String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(3)))
        .build()
        .into();
    let mut response = agent
        .get(base_url)
        .header("User-Agent", USER_AGENT)
        .call()
        .ok()?;
    let body = response.body_mut().read_to_string().ok()?;
    let data: serde_json::Value = serde_json::from_str(&body).ok()?;
    let krate = data.get("crate")?;
    krate
        .get("max_stable_version")
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            krate
                .get("newest_version")
                .and_then(serde_json::Value::as_str)
        })
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;
    use tempfile::TempDir;

    const NOW: f64 = 1_000_000.0;

    fn mock_server(version: &str) -> MockServer {
        let server = MockServer::start();
        let body = format!("{{\"crate\":{{\"max_stable_version\":\"{version}\"}}}}");
        server.mock(|when, then| {
            when.method(GET).path("/api/v1/crates/garlic-ward");
            then.status(200)
                .header("content-type", "application/json")
                .body(body);
        });
        server
    }

    #[test]
    fn parse_version_works() {
        assert_eq!(parse_version("1.2.3"), vec![1, 2, 3]);
        assert!(parse_version("0.1.0") < parse_version("0.2.0"));
        assert!(parse_version("1.0.0") > parse_version("0.99.99"));
    }

    #[test]
    fn network_failure_returns_none_no_cache() {
        let tmp = TempDir::new().unwrap();
        let cache = tmp.path().join("version_cache.toml");
        // Unroutable base URL — fetch fails.
        let result = check_latest_version("0.1.0", &cache, "http://127.0.0.1:0/x", NOW);
        assert_eq!(result, None);
        assert!(!cache.exists());
    }

    #[test]
    fn returns_newer_version() {
        let tmp = TempDir::new().unwrap();
        let cache = tmp.path().join("version_cache.toml");
        let server = mock_server("99.0.0");
        let url = server.url("/api/v1/crates/garlic-ward");
        assert_eq!(
            check_latest_version("0.1.0", &cache, &url, NOW),
            Some("99.0.0".to_string())
        );
    }

    #[test]
    fn returns_none_when_current() {
        let tmp = TempDir::new().unwrap();
        let cache = tmp.path().join("version_cache.toml");
        let server = mock_server("0.1.0");
        let url = server.url("/api/v1/crates/garlic-ward");
        assert_eq!(check_latest_version("0.1.0", &cache, &url, NOW), None);
    }

    #[test]
    fn writes_cache_on_success() {
        let tmp = TempDir::new().unwrap();
        let cache = tmp.path().join("version_cache.toml");
        let server = mock_server("99.0.0");
        let url = server.url("/api/v1/crates/garlic-ward");
        check_latest_version("0.1.0", &cache, &url, NOW);
        assert!(cache.exists());
        let data: VersionCache = toml::from_str(&fs::read_to_string(&cache).unwrap()).unwrap();
        assert_eq!(data.latest_version, "99.0.0");
        assert_eq!(data.checked_at, NOW);
    }

    #[test]
    fn writes_empty_latest_when_current() {
        let tmp = TempDir::new().unwrap();
        let cache = tmp.path().join("version_cache.toml");
        let server = mock_server("0.1.0");
        let url = server.url("/api/v1/crates/garlic-ward");
        check_latest_version("0.1.0", &cache, &url, NOW);
        let data: VersionCache = toml::from_str(&fs::read_to_string(&cache).unwrap()).unwrap();
        assert_eq!(data.latest_version, "");
    }

    #[test]
    fn uses_fresh_cache_without_network() {
        let tmp = TempDir::new().unwrap();
        let cache = tmp.path().join("version_cache.toml");
        fs::write(
            &cache,
            format!("checked_at = {NOW}\nlatest_version = \"5.0.0\"\n"),
        )
        .unwrap();
        // Bogus URL: if the cache weren't used this would fail, not return 5.0.0.
        let result = check_latest_version("0.1.0", &cache, "http://127.0.0.1:0/x", NOW + 10.0);
        assert_eq!(result, Some("5.0.0".to_string()));
    }

    #[test]
    fn fresh_cache_empty_latest_returns_none() {
        let tmp = TempDir::new().unwrap();
        let cache = tmp.path().join("version_cache.toml");
        fs::write(
            &cache,
            format!("checked_at = {NOW}\nlatest_version = \"\"\n"),
        )
        .unwrap();
        let result = check_latest_version("0.1.0", &cache, "http://127.0.0.1:0/x", NOW + 10.0);
        assert_eq!(result, None);
    }

    #[test]
    fn ignores_stale_cache() {
        let tmp = TempDir::new().unwrap();
        let cache = tmp.path().join("version_cache.toml");
        let stale = NOW - 25.0 * 3600.0;
        fs::write(
            &cache,
            format!("checked_at = {stale}\nlatest_version = \"1.0.0\"\n"),
        )
        .unwrap();
        let server = mock_server("99.0.0");
        let url = server.url("/api/v1/crates/garlic-ward");
        assert_eq!(
            check_latest_version("0.1.0", &cache, &url, NOW),
            Some("99.0.0".to_string())
        );
    }
}
