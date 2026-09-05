//! Opening the database and the [`Store`] handle.

use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::error::StoreError;
use crate::migrations;

/// An open `state.db`. One per daemon; SQLite serialises writers itself.
#[derive(Debug)]
pub struct Store {
    pub(crate) conn: Connection,
    path: PathBuf,
}

impl Store {
    /// Open (creating if needed) and migrate to the current schema.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| StoreError::Open {
                path: path.to_path_buf(),
                source: rusqlite::Error::SqliteFailure(
                    rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CANTOPEN),
                    Some(e.to_string()),
                ),
            })?;
        }
        let conn = Connection::open(path).map_err(|source| StoreError::Open {
            path: path.to_path_buf(),
            source,
        })?;
        // A foreign or truncated file fails here, before any migration touches it.
        if let Err(e) = conn.query_row("PRAGMA schema_version", [], |r| r.get::<_, i64>(0)) {
            return Err(StoreError::Corrupt {
                path: path.to_path_buf(),
                detail: e.to_string(),
            });
        }
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;
             PRAGMA busy_timeout = 5000;",
        )
        .map_err(|source| StoreError::Query {
            what: "pragmas",
            source,
        })?;
        let mut store = Self {
            conn,
            path: path.to_path_buf(),
        };
        migrations::migrate(&mut store)?;
        Ok(store)
    }

    /// In-memory store for tests and dry runs.
    pub fn open_in_memory() -> Result<Self, StoreError> {
        let conn = Connection::open_in_memory().map_err(|source| StoreError::Open {
            path: PathBuf::from(":memory:"),
            source,
        })?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .map_err(|source| StoreError::Query {
                what: "pragmas",
                source,
            })?;
        let mut store = Self {
            conn,
            path: PathBuf::from(":memory:"),
        };
        migrations::migrate(&mut store)?;
        Ok(store)
    }

    /// Path of the database file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Current `user_version`.
    pub fn schema_version(&self) -> Result<i64, StoreError> {
        self.conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(|source| StoreError::Query {
                what: "user_version",
                source,
            })
    }
}
