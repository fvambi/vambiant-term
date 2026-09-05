//! Watchdog for deferred decisions (docs/03 §4.6).
//!
//! Vendor permission prompts never time out, so every pending approval is
//! tracked with its age. Nothing here decides anything; it only makes
//! "still waiting" visible and lets the daemon escalate (re-notify) at a
//! configurable interval.

use std::time::{Duration, Instant};

use vt_proto::approval::ApprovalId;

/// One pending decision.
#[derive(Clone, Debug)]
pub struct Pending {
    /// Id.
    pub id: ApprovalId,
    /// When it arrived.
    pub since: Instant,
    /// How many reminders were raised.
    pub reminders: u32,
}

/// Tracks pending approvals.
#[derive(Debug, Default)]
pub struct Watchdog {
    pending: Vec<Pending>,
}

impl Watchdog {
    /// Track a new request.
    pub fn arm(&mut self, id: ApprovalId) {
        if !self.pending.iter().any(|p| p.id == id) {
            self.pending.push(Pending {
                id,
                since: Instant::now(),
                reminders: 0,
            });
        }
    }

    /// Stop tracking (answered or withdrawn).
    pub fn disarm(&mut self, id: &ApprovalId) {
        self.pending.retain(|p| p.id != *id);
    }

    /// Ids that have waited longer than `after` since arrival or the last
    /// reminder; bumps their reminder count.
    pub fn due(&mut self, after: Duration) -> Vec<ApprovalId> {
        let now = Instant::now();
        let mut out = Vec::new();
        for p in &mut self.pending {
            let waited = now.duration_since(p.since);
            if waited >= after * (p.reminders + 1) {
                p.reminders += 1;
                out.push(p.id.clone());
            }
        }
        out
    }

    /// Everything still waiting, oldest first.
    pub fn pending(&self) -> &[Pending] {
        &self.pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reminders_escalate_until_disarmed() {
        let mut w = Watchdog::default();
        let id = ApprovalId("a".into());
        w.arm(id.clone());
        w.arm(id.clone());
        assert_eq!(w.pending().len(), 1);
        assert!(w.due(Duration::from_secs(60)).is_empty());
        assert_eq!(w.due(Duration::ZERO), vec![id.clone()]);
        assert_eq!(w.pending()[0].reminders, 1);
        w.disarm(&id);
        assert!(w.pending().is_empty());
    }
}
