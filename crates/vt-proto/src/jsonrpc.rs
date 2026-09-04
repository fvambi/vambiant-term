//! JSON-RPC 2.0 envelope used on the `vtermd` Unix socket.

use serde::{Deserialize, Serialize};

/// Request id (number or string, per the spec).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Id {
    /// Numeric id.
    Number(u64),
    /// String id.
    String(String),
}

/// A request or notification (no `id`).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    /// Always `"2.0"`.
    pub jsonrpc: String,
    /// Method name, e.g. `session.list`.
    pub method: String,
    /// Parameters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
    /// Id; absent for notifications.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Id>,
}
