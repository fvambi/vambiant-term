//! Store errors that tell the user which file is wrong.

use std::path::PathBuf;

/// Everything that can go wrong with `state.db`.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// Could not open or create the database.
    #[error("cannot open state database {path}: {source}")]
    Open {
        /// Database path.
        path: PathBuf,
        /// SQLite error.
        #[source]
        source: rusqlite::Error,
    },
    /// The file exists but is not a database we can use.
    #[error(
        "state database {path} is corrupt or foreign ({detail}); move it aside to start fresh, nothing is deleted automatically"
    )]
    Corrupt {
        /// Database path.
        path: PathBuf,
        /// What SQLite said.
        detail: String,
    },
    /// The database is newer than this binary.
    #[error(
        "state database {path} has schema version {found}, this build knows up to {supported}: upgrade Vambiant Term"
    )]
    TooNew {
        /// Database path.
        path: PathBuf,
        /// Version on disk.
        found: i64,
        /// Newest version this binary handles.
        supported: i64,
    },
    /// A query failed.
    #[error("state database query failed ({what}): {source}")]
    Query {
        /// What was being done.
        what: &'static str,
        /// SQLite error.
        #[source]
        source: rusqlite::Error,
    },
}
