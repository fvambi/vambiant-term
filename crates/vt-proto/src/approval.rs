//! Approval requests and decisions — the inbox's data model.

use serde::{Deserialize, Serialize};

/// Opaque approval id, unique per daemon lifetime.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ApprovalId(pub String);

/// A decision the agent is blocked on.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApprovalRequest {
    /// Id.
    pub id: ApprovalId,
    /// Tool name as the vendor names it.
    pub tool: String,
    /// Tool input; the inbox may edit it before allowing.
    pub input: serde_json::Value,
    /// Vendor-supplied reason/description, if any.
    pub reason: Option<String>,
    /// Which vendor surface raised it (`PermissionRequest`, `PreToolUse`,
    /// Codex `item/commandExecution/requestApproval`, …).
    pub source: String,
}

/// What the human (or policy) decided.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "behavior", rename_all = "snake_case")]
pub enum Decision {
    /// Allow, optionally with edited input.
    Allow {
        /// Replacement input, if edited.
        updated_input: Option<serde_json::Value>,
    },
    /// Deny with a reason the agent sees.
    Deny {
        /// Reason.
        reason: String,
    },
}

/// Who decided. Autonomy decisions are always attributable (docs/00 §7).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionSource {
    /// A person, via the app or CLI.
    Human,
    /// A named policy rule, opt-in per workspace.
    Policy {
        /// Rule name.
        rule: String,
    },
}
