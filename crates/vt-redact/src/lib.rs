//! Streaming secret redaction (ADR-0007). **Fails closed.**
//!
//! If anything in this crate errors, panics or times out, the caller drops
//! the outbound request and tells the user. There is no fail-open path and
//! there will never be a "redact later". Recall wins every tie with
//! precision.
//!
//! Layers: vendored gitleaks regexes in one `RegexSet` ([`rules`]), a
//! Shannon-entropy layer ([`entropy`]), a sliding window that survives any
//! chunk boundary ([`stream`]), and a stateful multi-line PEM mode ([`pem`]).
//! Property tests in `tests/` assert no secret survives any split pattern.

pub mod entropy;
pub mod pem;
pub mod pipeline;
pub mod rules;
pub mod stream;

pub use pipeline::{RedactError, Redacted};
