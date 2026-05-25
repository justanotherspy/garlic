//! Interval-based time-tracking primitives shared by the engine and display.
//!
//! Each turn produces a closed [`Interval`] tagged with the session it came
//! from and its [`Kind`] (agent generation vs. user thinking). A session with
//! an in-flight interval keeps an [`OpenCursor`] until its next event closes
//! it. Per-day figures are derived from the closed intervals by [`breakdown`],
//! a sweep-line that unions overlapping time so two parallel agents over the
//! same wall-clock hour count as one hour, not two.

use serde::{Deserialize, Serialize};

/// Whether an interval is agent generation time or user thinking time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// UserPrompt → Stop: the agent was generating.
    Agent,
    /// Stop → UserPrompt: the user was reading, thinking, or typing.
    User,
}

/// A finished span of tracked time for one session.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Interval {
    pub session_id: String,
    pub kind: Kind,
    pub start: f64,
    pub end: f64,
}

/// An in-flight interval for a session that has begun but not yet been closed
/// by its next event.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OpenCursor {
    pub session_id: String,
    pub kind: Kind,
    pub start: f64,
}

/// Per-day derived totals, in minutes.
///
/// `agent` and `user` are independent "lenses": when one session generates
/// while another's user is reading, that wall-clock slice counts toward both,
/// so `agent + user` need not equal `total`. `overlap` is the wall-clock time
/// during which two or more sessions were active at once — a neutral
/// concurrency fact, never folded into `total`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DayBreakdown {
    pub total: f64,
    pub agent: f64,
    pub user: f64,
    pub overlap: f64,
}

/// Intervals closer than this (seconds) are treated as abutting for coalescing.
const COALESCE_EPSILON_SECS: f64 = 1.0;

/// Append a closed interval, merging it into the same session's most recent
/// interval when they share a kind and abut. This keeps the stored vector at
/// roughly O(turns) rather than O(events) on long days without changing any
/// derived figure (a session's own contiguous intervals never overlap).
pub fn push_interval(intervals: &mut Vec<Interval>, iv: Interval) {
    if iv.end <= iv.start {
        return;
    }
    if let Some(prev) = intervals
        .iter_mut()
        .rev()
        .find(|p| p.session_id == iv.session_id)
    {
        if prev.kind == iv.kind && (iv.start - prev.end).abs() <= COALESCE_EPSILON_SECS {
            prev.end = iv.end.max(prev.end);
            return;
        }
    }
    intervals.push(iv);
}

/// Total unioned wall-clock minutes across all intervals.
pub fn union_minutes(intervals: &[Interval]) -> f64 {
    breakdown(intervals).total
}

/// Sweep-line over closed intervals computing the union total, the per-kind
/// unions, and concurrent (2+ sessions) wall-clock time.
pub fn breakdown(intervals: &[Interval]) -> DayBreakdown {
    // (time, delta) events; +1 at a start, -1 at an end.
    let mut events: Vec<(f64, i32, Kind)> = Vec::with_capacity(intervals.len() * 2);
    for iv in intervals {
        if iv.end <= iv.start {
            continue;
        }
        events.push((iv.start, 1, iv.kind));
        events.push((iv.end, -1, iv.kind));
    }
    if events.is_empty() {
        return DayBreakdown::default();
    }
    events.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    let (mut total, mut agent, mut user, mut overlap) = (0.0, 0.0, 0.0, 0.0);
    let (mut active, mut active_agent, mut active_user) = (0i32, 0i32, 0i32);
    let mut last: Option<f64> = None;

    for (t, delta, kind) in events {
        if let Some(prev) = last {
            let dt = t - prev;
            if dt > 0.0 {
                if active >= 1 {
                    total += dt;
                }
                if active >= 2 {
                    overlap += dt;
                }
                if active_agent >= 1 {
                    agent += dt;
                }
                if active_user >= 1 {
                    user += dt;
                }
            }
        }
        active += delta;
        match kind {
            Kind::Agent => active_agent += delta,
            Kind::User => active_user += delta,
        }
        last = Some(t);
    }

    DayBreakdown {
        total: total / 60.0,
        agent: agent / 60.0,
        user: user / 60.0,
        overlap: overlap / 60.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn iv(session: &str, kind: Kind, start: f64, end: f64) -> Interval {
        Interval {
            session_id: session.to_string(),
            kind,
            start,
            end,
        }
    }

    #[test]
    fn empty_breakdown_is_zero() {
        assert_eq!(breakdown(&[]), DayBreakdown::default());
    }

    #[test]
    fn single_session_sums_linearly() {
        // One session: user 0-60s, agent 60-180s, user 180-240s (contiguous).
        let intervals = vec![
            iv("a", Kind::User, 0.0, 60.0),
            iv("a", Kind::Agent, 60.0, 180.0),
            iv("a", Kind::User, 180.0, 240.0),
        ];
        let b = breakdown(&intervals);
        assert!((b.total - 4.0).abs() < 1e-9); // 240s = 4m
        assert!((b.agent - 2.0).abs() < 1e-9); // 120s
        assert!((b.user - 2.0).abs() < 1e-9); // 120s
        assert_eq!(b.overlap, 0.0);
    }

    #[test]
    fn parallel_agents_union_not_sum() {
        // Two sessions both generating over the same 60s window.
        let intervals = vec![
            iv("a", Kind::Agent, 0.0, 60.0),
            iv("b", Kind::Agent, 0.0, 60.0),
        ];
        let b = breakdown(&intervals);
        assert!((b.total - 1.0).abs() < 1e-9); // union = 1m, not 2m
        assert!((b.agent - 1.0).abs() < 1e-9);
        assert!((b.overlap - 1.0).abs() < 1e-9); // fully concurrent
    }

    #[test]
    fn partial_overlap_counts_only_concurrent_slice() {
        let intervals = vec![
            iv("a", Kind::Agent, 0.0, 90.0),
            iv("b", Kind::Agent, 60.0, 120.0),
        ];
        let b = breakdown(&intervals);
        assert!((b.total - 2.0).abs() < 1e-9); // union 0-120 = 120s = 2m
        assert!((b.overlap - 0.5).abs() < 1e-9); // 60-90 = 30s overlap
    }

    #[test]
    fn agent_and_user_can_both_count_a_slice() {
        // Session a generating 0-60 while session b's user reads 0-60.
        let intervals = vec![
            iv("a", Kind::Agent, 0.0, 60.0),
            iv("b", Kind::User, 0.0, 60.0),
        ];
        let b = breakdown(&intervals);
        assert!((b.total - 1.0).abs() < 1e-9);
        assert!((b.agent - 1.0).abs() < 1e-9);
        assert!((b.user - 1.0).abs() < 1e-9); // agent + user > total, by design
        assert!((b.overlap - 1.0).abs() < 1e-9);
    }

    #[test]
    fn push_coalesces_abutting_same_kind() {
        let mut v = Vec::new();
        push_interval(&mut v, iv("a", Kind::Agent, 0.0, 60.0));
        push_interval(&mut v, iv("a", Kind::Agent, 60.0, 120.0));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].end, 120.0);
    }

    #[test]
    fn push_does_not_coalesce_across_sessions_or_kinds() {
        let mut v = Vec::new();
        push_interval(&mut v, iv("a", Kind::Agent, 0.0, 60.0));
        push_interval(&mut v, iv("b", Kind::Agent, 60.0, 120.0)); // different session
        push_interval(&mut v, iv("a", Kind::User, 120.0, 180.0)); // different kind
        assert_eq!(v.len(), 3);
    }

    #[test]
    fn push_drops_zero_length() {
        let mut v = Vec::new();
        push_interval(&mut v, iv("a", Kind::Agent, 50.0, 50.0));
        assert!(v.is_empty());
    }
}
