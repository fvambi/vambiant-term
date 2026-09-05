//! JSON-RPC 2.0 envelope used on the `vtermd` Unix socket (newline-delimited).
//!
//! Only the envelope lives here; method names and their parameter types are
//! in [`crate::session`] and friends so every client speaks the same vocabulary.

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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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

impl Request {
    /// Build a request.
    pub fn new(id: Id, method: impl Into<String>, params: Option<serde_json::Value>) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            method: method.into(),
            params,
            id: Some(id),
        }
    }

    /// Build a notification.
    pub fn notification(method: impl Into<String>, params: Option<serde_json::Value>) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            method: method.into(),
            params,
            id: None,
        }
    }

    /// `true` when this carries no id.
    pub fn is_notification(&self) -> bool {
        self.id.is_none()
    }
}

/// Error object.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RpcError {
    /// Numeric code (JSON-RPC reserved range or ours, see constants).
    pub code: i64,
    /// Human-readable message a user can act on.
    pub message: String,
    /// Optional structured detail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl RpcError {
    /// Standard: the request was not valid JSON.
    pub const PARSE_ERROR: i64 = -32700;
    /// Standard: not a valid request object.
    pub const INVALID_REQUEST: i64 = -32600;
    /// Standard: unknown method.
    pub const METHOD_NOT_FOUND: i64 = -32601;
    /// Standard: bad params.
    pub const INVALID_PARAMS: i64 = -32602;
    /// Standard: server fault.
    pub const INTERNAL: i64 = -32603;
    /// Ours: the peer is not the socket owner.
    pub const UNAUTHORIZED: i64 = -32001;
    /// Ours: no such session.
    pub const NO_SUCH_SESSION: i64 = -32002;

    /// Convenience constructor.
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }
}

/// A response to a request with an id.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Response {
    /// Always `"2.0"`.
    pub jsonrpc: String,
    /// Result on success.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    /// Error on failure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
    /// Echoed id; `null` when the request id could not be read.
    pub id: Option<Id>,
}

impl Response {
    /// Success.
    pub fn ok(id: Option<Id>, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            result: Some(result),
            error: None,
            id,
        }
    }

    /// Failure.
    pub fn err(id: Option<Id>, error: RpcError) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            result: None,
            error: Some(error),
            id,
        }
    }
}

/// Anything that can arrive on the wire.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Message {
    /// Request or notification.
    Request(Request),
    /// Response.
    Response(Response),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_distinguishes_notifications() {
        let req = Request::new(Id::Number(1), "session.list", None);
        let s = serde_json::to_string(&req).unwrap();
        assert_eq!(s, r#"{"jsonrpc":"2.0","method":"session.list","id":1}"#);
        let back: Message = serde_json::from_str(&s).unwrap();
        assert_eq!(back, Message::Request(req));
        let n = Request::notification("session.changed", Some(serde_json::json!({"id": "s1"})));
        assert!(n.is_notification());
        let r: Message = serde_json::from_str(
            r#"{"jsonrpc":"2.0","error":{"code":-32601,"message":"nope"},"id":"x"}"#,
        )
        .unwrap();
        match r {
            Message::Response(resp) => {
                assert_eq!(resp.error.unwrap().code, RpcError::METHOD_NOT_FOUND);
            }
            Message::Request(_) => panic!("expected response"),
        }
    }
}
