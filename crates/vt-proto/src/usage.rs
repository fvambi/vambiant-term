//! Token and cost accounting.

use serde::{Deserialize, Serialize};

/// Usage snapshot. `cost_usd` is a **client-side list-price estimate**
/// wherever it comes from Claude Code's `total_cost_usd`; the UI labels it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    /// Input tokens.
    pub input: u64,
    /// Output tokens.
    pub output: u64,
    /// Cache read tokens.
    pub cache_read: u64,
    /// Cache write tokens.
    pub cache_write: u64,
    /// Estimated cost.
    pub cost_usd: Option<f64>,
    /// Context window used, percent.
    pub context_used_pct: Option<f32>,
}
