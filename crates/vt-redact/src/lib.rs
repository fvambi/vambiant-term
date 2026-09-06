//! Secret redaction (ADR-0007). **Fails closed.**
//!
//! If anything in this crate errors, panics or times out, the caller drops
//! the outbound request and tells the user. There is no fail-open path and
//! there will never be a "redact later". Recall wins every tie with
//! precision.
//!
//! Built so far: the rule set ([`rules`]) applied to whole payloads with a
//! time budget and panic containment ([`pipeline`]). Still to build
//! (docs/07 M-SEC): the sliding window over PTY chunk boundaries
//! ([`stream`]), the Shannon-entropy layer ([`entropy`]) and streaming
//! multi-line PEM mode ([`pem`]); those modules are placeholders.

pub mod entropy;
pub mod pem;
pub mod pipeline;
pub mod rules;
pub mod stream;

pub use pipeline::{RedactError, Redacted, redact, redact_within};
