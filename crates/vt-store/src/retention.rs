//! Retention windows and the sweep run on daemon start.

use rusqlite::params;

use crate::error::StoreError;
use crate::schema::Store;

/// Retention policy in days (docs/02 §7 defaults).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Retention {
    /// Agent events and blocks.
    pub events_days: u32,
    /// Egress log.
    pub egress_days: u32,
    /// Ended sessions (their events go with them).
    pub sessions_days: u32,
}

impl Default for Retention {
    fn default() -> Self {
        Self {
            events_days: 90,
            egress_days: 365,
            sessions_days: 90,
        }
    }
}

/// What a sweep removed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Swept {
    /// Rows removed from `agent_events`.
    pub events: usize,
    /// Rows removed from `blocks`.
    pub blocks: usize,
    /// Rows removed from `egress`.
    pub egress: usize,
    /// Ended sessions removed.
    pub sessions: usize,
}

impl Store {
    /// Delete everything older than the policy relative to `now` (RFC 3339 UTC,
    /// passed in so tests are deterministic). Live sessions are never touched.
    pub fn sweep(&self, policy: Retention, now: &str) -> Result<Swept, StoreError> {
        let cutoff = |days: u32| format!("datetime(?1, '-{days} days')");
        let run = |sql: String, what: &'static str| {
            self.conn
                .execute(&sql, params![now])
                .map_err(|source| StoreError::Query { what, source })
        };
        Ok(Swept {
            events: run(
                format!(
                    "DELETE FROM agent_events WHERE datetime(at) < {}",
                    cutoff(policy.events_days)
                ),
                "sweep events",
            )?,
            blocks: run(
                format!(
                    "DELETE FROM blocks WHERE datetime(started_at) < {}",
                    cutoff(policy.events_days)
                ),
                "sweep blocks",
            )?,
            egress: run(
                format!(
                    "DELETE FROM egress WHERE datetime(at) < {}",
                    cutoff(policy.egress_days)
                ),
                "sweep egress",
            )?,
            sessions: run(
                format!(
                    "DELETE FROM sessions WHERE ended_at IS NOT NULL AND datetime(ended_at) < {}",
                    cutoff(policy.sessions_days)
                ),
                "sweep sessions",
            )?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::egress::EgressRecord;

    #[test]
    fn sweeps_only_old_rows() {
        let store = Store::open_in_memory().unwrap();
        let mk = |at: &str| EgressRecord {
            at: at.into(),
            provider: "p".into(),
            model: "m".into(),
            purpose: "ask".into(),
            bytes_sent: 1,
            redactions: 0,
            payload: None,
        };
        store.record_egress(&mk("2025-01-01T00:00:00Z")).unwrap();
        store.record_egress(&mk("2026-09-01T00:00:00Z")).unwrap();
        let swept = store
            .sweep(Retention::default(), "2026-09-05T00:00:00Z")
            .unwrap();
        assert_eq!(swept.egress, 1);
        assert_eq!(store.recent_egress(10).unwrap().len(), 1);
    }
}
