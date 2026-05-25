//! Opportunistic sync of locally-accounted intervals to the shared backend.
//!
//! garlic is local-first: hooks always account time into `state.toml` and never
//! block on the network. When a backend is configured, the day's closed
//! intervals are *also* pushed to it so a single shared total can span machines
//! and ephemeral environments. Who drains that outbox is decoupled from the
//! hooks — there are three paths, all funnelling through [`Remote::push_intervals`]:
//!
//! 1. **Opportunistic** — `garlic status` flushes on the way through.
//! 2. **`garlic sync`** — an explicit drain, e.g. a laptop cron job running
//!    *outside* a network-restricted sandbox.
//! 3. **Synchronous hooks** — on ephemeral/managed hosts (set
//!    `GARLIC_SYNC=blocking`), hooks flush inline because the container may
//!    vanish before any interactive command or cron runs.

use crate::config::Config;
use crate::paths::Paths;
use crate::remote::{Remote, RemoteResponse};
use crate::state::{save_state, State};

/// Push this machine's closed intervals to the backend, returning the merged
/// (cross-machine) state. Used by `status`, `sync`, and the synchronous hooks.
///
/// If a `reset` happened while the backend was unreachable (`reset_pending`),
/// the backend is zeroed *before* the push — otherwise the union merge, which
/// only replaces the sessions present in the push, would keep the stale
/// pre-reset intervals and the shared total would never reflect the reset. The
/// cleared flag is persisted so the reset reconciles exactly once; on failure
/// it stays set and is retried on the next flush.
pub fn flush(
    remote: &Remote,
    paths: &Paths,
    config: &Config,
    state: &mut State,
    check_nudges: bool,
) -> Result<RemoteResponse, String> {
    if state.reset_pending {
        remote.reset(config)?;
        state.reset_pending = false;
        let _ = save_state(paths, state);
    }
    remote.push_intervals(config, &state.intervals, check_nudges)
}

/// Whether hooks should flush synchronously inside the hook process.
///
/// Default (unset) keeps hooks fully local so they never block on the network
/// — the laptop case, where a separate `garlic sync` (cron) drains the outbox.
/// `GARLIC_SYNC=blocking` opts into inline flushing for ephemeral containers
/// that have no "next invocation" and may be reclaimed at any moment.
pub fn hooks_block_on_sync() -> bool {
    matches!(
        std::env::var("GARLIC_SYNC").ok().as_deref().map(str::trim),
        Some("blocking" | "sync" | "1")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `GARLIC_SYNC` is process-global, so exercise every branch in one test to
    /// avoid races between parallel tests mutating the same env var.
    #[test]
    fn blocking_is_opt_in() {
        // SAFETY: single-threaded within this test; no other test reads the var.
        unsafe {
            std::env::remove_var("GARLIC_SYNC");
            assert!(!hooks_block_on_sync());
            std::env::set_var("GARLIC_SYNC", "blocking");
            assert!(hooks_block_on_sync());
            std::env::set_var("GARLIC_SYNC", "1");
            assert!(hooks_block_on_sync());
            std::env::set_var("GARLIC_SYNC", "");
            assert!(!hooks_block_on_sync());
            std::env::remove_var("GARLIC_SYNC");
        }
    }
}
