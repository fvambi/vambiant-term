//! IPC errors.

/// Transport and protocol errors.
#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    /// Could not reach the daemon.
    #[error("cannot connect to vtermd at {path}: {source}")]
    Connect {
        /// Socket path tried.
        path: std::path::PathBuf,
        /// OS error.
        #[source]
        source: std::io::Error,
    },
}
