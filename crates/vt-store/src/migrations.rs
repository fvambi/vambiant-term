//! Ordered, idempotent migrations keyed by SQLite's `user_version`.
//!
//! Rule: never edit a shipped migration; append a new one. Each step runs in
//! a transaction and bumps `user_version` only on success, so a crash midway
//! re-runs the same step next time.

use crate::error::StoreError;
use crate::schema::Store;

/// Every migration in order. Index + 1 is the resulting `user_version`.
const MIGRATIONS: &[&str] = &[
    // v1 — M2: sessions, agent events, decisions, egress, worktrees, blocks.
    "CREATE TABLE sessions (
        id          TEXT PRIMARY KEY,
        name        TEXT NOT NULL,
        agent       TEXT NOT NULL,            -- claude | codex | generic
        state       TEXT NOT NULL,            -- AgentState snake_case
        cwd         TEXT NOT NULL,
        argv        TEXT NOT NULL,            -- JSON array
        env         TEXT NOT NULL DEFAULT '[]',
        pid         INTEGER,                  -- child pid while owned
        pty_path    TEXT,                     -- slave tty path for re-adoption
        cols        INTEGER NOT NULL DEFAULT 80,
        rows        INTEGER NOT NULL DEFAULT 24,
        orphaned    INTEGER NOT NULL DEFAULT 0,
        created_at  TEXT NOT NULL,            -- RFC 3339 UTC
        ended_at    TEXT,
        exit_code   INTEGER
    );
    CREATE INDEX sessions_state ON sessions(state);

    CREATE TABLE agent_events (
        seq         INTEGER PRIMARY KEY AUTOINCREMENT,
        session_id  TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
        at          TEXT NOT NULL,
        kind        TEXT NOT NULL,            -- AgentEvent tag
        payload     TEXT NOT NULL             -- JSON
    );
    CREATE INDEX agent_events_session ON agent_events(session_id, seq);

    CREATE TABLE decisions (
        id          TEXT PRIMARY KEY,         -- ApprovalId
        session_id  TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
        requested_at TEXT NOT NULL,
        resolved_at TEXT,
        tool        TEXT NOT NULL,
        request     TEXT NOT NULL,            -- JSON ApprovalRequest
        decision    TEXT,                     -- JSON Decision
        decided_by  TEXT                      -- JSON DecisionSource
    );
    CREATE INDEX decisions_pending ON decisions(resolved_at) WHERE resolved_at IS NULL;

    CREATE TABLE egress (
        seq         INTEGER PRIMARY KEY AUTOINCREMENT,
        at          TEXT NOT NULL,
        provider    TEXT NOT NULL,
        model       TEXT NOT NULL,
        purpose     TEXT NOT NULL,            -- suggest | ask | explain | classify
        bytes_sent  INTEGER NOT NULL,
        redactions  INTEGER NOT NULL,
        payload     TEXT                      -- redacted payload, when retained
    );

    CREATE TABLE worktrees (
        path        TEXT PRIMARY KEY,
        repo        TEXT NOT NULL,
        branch      TEXT,
        session_id  TEXT REFERENCES sessions(id) ON DELETE SET NULL,
        created_at  TEXT NOT NULL,
        adopted     INTEGER NOT NULL DEFAULT 0
    );

    CREATE TABLE blocks (
        seq         INTEGER PRIMARY KEY AUTOINCREMENT,
        session_id  TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
        kind        TEXT NOT NULL,
        confidence  TEXT NOT NULL,            -- marked | heuristic
        start_line  INTEGER NOT NULL,
        end_line    INTEGER,
        cmdline     TEXT,
        exit_code   INTEGER,
        started_at  TEXT NOT NULL
    );
    CREATE INDEX blocks_session ON blocks(session_id, seq);",
];

/// Newest schema version this binary understands.
#[allow(clippy::cast_possible_wrap)] // a handful of migrations, never 2^63
pub const CURRENT_VERSION: i64 = MIGRATIONS.len() as i64;

pub(crate) fn migrate(store: &mut Store) -> Result<(), StoreError> {
    let found = store.schema_version()?;
    if found > CURRENT_VERSION {
        return Err(StoreError::TooNew {
            path: store.path().to_path_buf(),
            found,
            supported: CURRENT_VERSION,
        });
    }
    for (i, sql) in MIGRATIONS.iter().enumerate() {
        let target = i64::try_from(i).unwrap_or(i64::MAX).saturating_add(1);
        if target <= found {
            continue;
        }
        let tx = store
            .conn
            .transaction()
            .map_err(|source| StoreError::Query {
                what: "begin migration",
                source,
            })?;
        tx.execute_batch(sql).map_err(|source| StoreError::Query {
            what: "apply migration",
            source,
        })?;
        tx.pragma_update(None, "user_version", target)
            .map_err(|source| StoreError::Query {
                what: "bump user_version",
                source,
            })?;
        tx.commit().map_err(|source| StoreError::Query {
            what: "commit migration",
            source,
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_fresh_and_is_idempotent() {
        let mut store = Store::open_in_memory().unwrap();
        assert_eq!(store.schema_version().unwrap(), CURRENT_VERSION);
        migrate(&mut store).unwrap();
        assert_eq!(store.schema_version().unwrap(), CURRENT_VERSION);
        let tables: Vec<String> = store
            .conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            tables,
            [
                "agent_events",
                "blocks",
                "decisions",
                "egress",
                "sessions",
                "worktrees"
            ]
        );
    }

    #[test]
    fn refuses_a_newer_database() {
        let dir = std::env::temp_dir().join(format!("vt-store-new-{}", std::process::id()));
        let path = dir.join("state.db");
        {
            let store = Store::open(&path).unwrap();
            store
                .conn
                .pragma_update(None, "user_version", CURRENT_VERSION + 5)
                .unwrap();
        }
        match Store::open(&path) {
            Err(StoreError::TooNew {
                found, supported, ..
            }) => {
                assert_eq!(found, CURRENT_VERSION + 5);
                assert_eq!(supported, CURRENT_VERSION);
            }
            other => panic!("expected TooNew, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn reports_a_foreign_file_instead_of_crashing() {
        let dir = std::env::temp_dir().join(format!("vt-store-corrupt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("state.db");
        std::fs::write(&path, b"this is not a database, it is 40 bytes of junk!!").unwrap();
        match Store::open(&path) {
            Err(StoreError::Corrupt { .. }) => {}
            other => panic!("expected Corrupt, got {other:?}"),
        }
        assert!(path.exists(), "nothing is deleted automatically");
        let _ = std::fs::remove_dir_all(dir);
    }
}
