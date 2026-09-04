//! Damage tracking: which lines changed since the last flush.
//!
//! Shape verified against `alacritty_terminal` 0.26.0 in M0: the backend
//! reports either *everything* or a per-line `(left, right)` column span, and
//! cursor movement damages both the old and new cursor lines. We keep that
//! granularity — lines, not cells — because the renderer re-shapes whole
//! runs anyway.

/// Damage accumulated between two flushes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DamageSet {
    /// Every visible cell must be redrawn (resize, scroll region change,
    /// full clear, first frame after attach).
    Full,
    /// Only these lines changed. Sorted by row, at most one entry per row.
    Lines(Vec<LineDamage>),
}

/// Damaged column span on one visible row (inclusive bounds).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineDamage {
    /// Visible row index.
    pub row: u16,
    /// First damaged column.
    pub left: u16,
    /// Last damaged column.
    pub right: u16,
}

impl DamageSet {
    /// `true` when nothing needs redrawing.
    pub fn is_clean(&self) -> bool {
        matches!(self, DamageSet::Lines(lines) if lines.is_empty())
    }
}
