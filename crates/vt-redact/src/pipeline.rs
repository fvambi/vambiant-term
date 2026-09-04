//! The fail-closed entry point.

/// Output of a successful redaction pass.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Redacted {
    /// Redacted text.
    pub text: String,
    /// Number of replacements made; logged to the egress record.
    pub replacements: usize,
}

/// Any error here means the request **must not be sent**.
#[derive(Debug, thiserror::Error)]
pub enum RedactError {
    /// The pipeline exceeded its time budget.
    #[error("redaction timed out after {ms} ms; request dropped, nothing was sent")]
    Timeout {
        /// Budget that was exceeded.
        ms: u64,
    },
    /// A rule set failed to compile or a layer panicked (caught at the boundary).
    #[error("redaction pipeline failed ({stage}); request dropped, nothing was sent")]
    Internal {
        /// Which layer failed.
        stage: &'static str,
    },
}
