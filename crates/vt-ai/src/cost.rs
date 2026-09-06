//! Cost estimates at list price (docs/04 §7). Labelled an estimate
//! wherever shown; local models cost 0 and are still counted.

use serde::{Deserialize, Serialize};

use crate::provider::Usage;

/// USD per million tokens.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
/// List prices for one model.
#[allow(missing_docs)] // field names are the wire format's own vocabulary
pub struct Pricing {
    #[serde(default)]
    pub input: f64,
    #[serde(default)]
    pub output: f64,
    #[serde(default)]
    pub cache_read: f64,
    #[serde(default)]
    pub cache_write: f64,
}

impl Pricing {
    /// Estimated USD for `usage`.
    #[allow(clippy::cast_precision_loss)] // token counts are far below 2^52
    pub fn estimate(&self, u: Usage) -> f64 {
        let m = |tokens: u64, per_m: f64| (tokens as f64) * per_m / 1_000_000.0;
        m(u.input_tokens, self.input)
            + m(u.output_tokens, self.output)
            + m(u.cache_read_tokens, self.cache_read)
            + m(u.cache_write_tokens, self.cache_write)
    }
}
