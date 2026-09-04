//! The never-auto floor. **Hard-coded. Not configurable.**
//!
//! Classes at or above this floor are never auto-approved by any rule, and
//! the generic adapter is never auto-answered at all. Tests in `tests/`
//! feed a deliberately hostile `.vambiant-term/policy.toml` and assert it
//! cannot move this.

use crate::classify::SafetyClass;

/// Minimum class that always requires a human.
pub const NEVER_AUTO_AT_OR_ABOVE: SafetyClass = SafetyClass::Risky;
