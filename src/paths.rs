//! Filesystem path resolution for garlic's runtime files.
//!
//! All garlic state lives under a single directory, normally `~/.garlic`. The
//! `GARLIC_DIR` environment variable overrides this — production users can
//! relocate it, and tests point it at a temp directory so they never touch the
//! real `~/.garlic/`.

use std::path::{Path, PathBuf};

/// Locations of garlic's config, state, and version-cache files.
#[derive(Clone, Debug)]
pub struct Paths {
    dir: PathBuf,
}

impl Paths {
    /// Resolve paths from the environment: `$GARLIC_DIR` if set, else
    /// `$HOME/.garlic`.
    pub fn resolve() -> Paths {
        let dir = match std::env::var_os("GARLIC_DIR") {
            Some(d) => PathBuf::from(d),
            None => home_dir().join(".garlic"),
        };
        Paths { dir }
    }

    /// Build paths rooted at an explicit directory (used by tests).
    pub fn with_dir(dir: impl Into<PathBuf>) -> Paths {
        Paths { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn config(&self) -> PathBuf {
        self.dir.join("config.toml")
    }

    pub fn state(&self) -> PathBuf {
        self.dir.join("state.toml")
    }

    pub fn version_cache(&self) -> PathBuf {
        self.dir.join("version_cache.toml")
    }
}

/// Locations of the Claude Code files garlic's `setup` touches.
#[derive(Clone, Debug)]
pub struct ClaudePaths {
    dir: PathBuf,
}

impl ClaudePaths {
    /// Resolve `~/.claude` (honoring `$CLAUDE_HOME` for tests).
    pub fn resolve() -> ClaudePaths {
        let dir = match std::env::var_os("CLAUDE_HOME") {
            Some(d) => PathBuf::from(d),
            None => home_dir().join(".claude"),
        };
        ClaudePaths { dir }
    }

    /// Build paths rooted at an explicit `.claude` directory (used by tests).
    pub fn with_dir(dir: impl Into<PathBuf>) -> ClaudePaths {
        ClaudePaths { dir: dir.into() }
    }

    pub fn settings(&self) -> PathBuf {
        self.dir.join("settings.json")
    }

    pub fn commands_dir(&self) -> PathBuf {
        self.dir.join("commands")
    }

    pub fn claude_md(&self) -> PathBuf {
        self.dir.join("CLAUDE.md")
    }
}

fn home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}
