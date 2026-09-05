//! Session rows and the re-adoption table the daemon reads on restart.

use rusqlite::{OptionalExtension, params};
use vt_proto::agent::{AgentKind, AgentState};
use vt_proto::session::{Capabilities, SessionId, SessionInfo};

use crate::error::StoreError;
use crate::schema::Store;

/// What the daemon persists about a session beyond [`SessionInfo`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionRecord {
    /// Public view.
    pub info: SessionInfo,
    /// Program and arguments (empty = login shell).
    pub argv: Vec<String>,
    /// Extra environment.
    pub env: Vec<(String, String)>,
    /// Slave tty path (informational).
    pub pty_path: Option<String>,
    /// Control socket of the session's fd holder, for re-adoption.
    pub hold_socket: Option<String>,
    /// Exit code once ended.
    pub exit_code: Option<i32>,
}

fn kind_str(k: AgentKind) -> &'static str {
    match k {
        AgentKind::Claude => "claude",
        AgentKind::Codex => "codex",
        AgentKind::Generic => "generic",
    }
}

fn kind_from(s: &str) -> AgentKind {
    match s {
        "claude" => AgentKind::Claude,
        "codex" => AgentKind::Codex,
        _ => AgentKind::Generic,
    }
}

fn state_json(s: AgentState) -> String {
    serde_json::to_string(&s)
        .unwrap_or_else(|_| "\"crashed\"".into())
        .trim_matches('"')
        .to_owned()
}

fn state_from(s: &str) -> AgentState {
    serde_json::from_str(&format!("\"{s}\"")).unwrap_or(AgentState::Crashed)
}

impl Store {
    /// Insert or replace a session.
    pub fn upsert_session(&self, rec: &SessionRecord) -> Result<(), StoreError> {
        let i = &rec.info;
        self.conn
            .execute(
                "INSERT INTO sessions (id, name, agent, state, cwd, argv, env, pid, pty_path, cols, rows, orphaned, created_at, exit_code, hold_socket)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
                 ON CONFLICT(id) DO UPDATE SET name=excluded.name, state=excluded.state, cwd=excluded.cwd,
                   pid=excluded.pid, pty_path=excluded.pty_path, cols=excluded.cols, rows=excluded.rows,
                   orphaned=excluded.orphaned, exit_code=excluded.exit_code, hold_socket=excluded.hold_socket",
                params![
                    i.id.0,
                    i.name,
                    kind_str(i.agent),
                    state_json(i.state),
                    i.cwd.display().to_string(),
                    serde_json::to_string(&rec.argv).unwrap_or_default(),
                    serde_json::to_string(&rec.env).unwrap_or_default(),
                    i.pid,
                    rec.pty_path,
                    i.size.map_or(80, |s| i64::from(s.0)),
                    i.size.map_or(24, |s| i64::from(s.1)),
                    i32::from(i.orphaned),
                    i.created_at,
                    rec.exit_code,
                    rec.hold_socket,
                ],
            )
            .map_err(|source| StoreError::Query { what: "upsert session", source })?;
        Ok(())
    }

    /// Mark a session ended.
    pub fn end_session(
        &self,
        id: &SessionId,
        exit_code: Option<i32>,
        at: &str,
    ) -> Result<(), StoreError> {
        self.conn
            .execute(
                "UPDATE sessions SET state='stopped', ended_at=?2, exit_code=?3, pid=NULL WHERE id=?1",
                params![id.0, at, exit_code],
            )
            .map_err(|source| StoreError::Query { what: "end session", source })?;
        Ok(())
    }

    /// Mark a session orphaned (could not be re-adopted). Never deletes.
    pub fn mark_orphaned(&self, id: &SessionId) -> Result<(), StoreError> {
        self.conn
            .execute("UPDATE sessions SET orphaned=1 WHERE id=?1", params![id.0])
            .map_err(|source| StoreError::Query {
                what: "mark orphaned",
                source,
            })?;
        Ok(())
    }

    /// One session.
    pub fn session(&self, id: &SessionId) -> Result<Option<SessionRecord>, StoreError> {
        self.conn
            .query_row(
                &format!("{SELECT_SESSION} WHERE id=?1"),
                params![id.0],
                row_to_record,
            )
            .optional()
            .map_err(|source| StoreError::Query {
                what: "get session",
                source,
            })
    }

    /// Every session that has not ended (what the daemon must re-adopt on start).
    pub fn live_sessions(&self) -> Result<Vec<SessionRecord>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(&format!(
                "{SELECT_SESSION} WHERE ended_at IS NULL ORDER BY created_at"
            ))
            .map_err(|source| StoreError::Query {
                what: "prepare live sessions",
                source,
            })?;
        let rows = stmt
            .query_map([], row_to_record)
            .map_err(|source| StoreError::Query {
                what: "live sessions",
                source,
            })?;
        rows.collect::<Result<_, _>>()
            .map_err(|source| StoreError::Query {
                what: "read live sessions",
                source,
            })
    }

    /// Every session, newest first.
    pub fn all_sessions(&self) -> Result<Vec<SessionRecord>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(&format!("{SELECT_SESSION} ORDER BY created_at DESC"))
            .map_err(|source| StoreError::Query {
                what: "prepare sessions",
                source,
            })?;
        let rows = stmt
            .query_map([], row_to_record)
            .map_err(|source| StoreError::Query {
                what: "sessions",
                source,
            })?;
        rows.collect::<Result<_, _>>()
            .map_err(|source| StoreError::Query {
                what: "read sessions",
                source,
            })
    }
}

const SELECT_SESSION: &str = "SELECT id, name, agent, state, cwd, argv, env, pid, pty_path, cols, rows, orphaned, created_at, exit_code, hold_socket FROM sessions";

fn row_to_record(r: &rusqlite::Row<'_>) -> rusqlite::Result<SessionRecord> {
    let agent = kind_from(&r.get::<_, String>(2)?);
    let argv: Vec<String> = serde_json::from_str(&r.get::<_, String>(5)?).unwrap_or_default();
    let env: Vec<(String, String)> =
        serde_json::from_str(&r.get::<_, String>(6)?).unwrap_or_default();
    let cols: i64 = r.get(9)?;
    let rows: i64 = r.get(10)?;
    Ok(SessionRecord {
        info: SessionInfo {
            id: SessionId(r.get(0)?),
            name: r.get(1)?,
            agent,
            state: state_from(&r.get::<_, String>(3)?),
            capabilities: Capabilities::default(),
            cwd: std::path::PathBuf::from(r.get::<_, String>(4)?),
            pid: r
                .get::<_, Option<i64>>(7)?
                .and_then(|p| u32::try_from(p).ok()),
            size: Some((
                u16::try_from(cols).unwrap_or(80),
                u16::try_from(rows).unwrap_or(24),
            )),
            orphaned: r.get::<_, i64>(11)? != 0,
            readopted: false,
            created_at: r.get(12)?,
        },
        argv,
        env,
        pty_path: r.get(8)?,
        exit_code: r.get(13)?,
        hold_socket: r.get(14)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(id: &str) -> SessionRecord {
        SessionRecord {
            info: SessionInfo {
                id: SessionId(id.into()),
                name: format!("name-{id}"),
                agent: AgentKind::Claude,
                state: AgentState::Idle,
                capabilities: Capabilities::default(),
                cwd: "/tmp/x".into(),
                pid: Some(4242),
                size: Some((120, 40)),
                orphaned: false,
                readopted: false,
                created_at: "2026-09-05T10:00:00Z".into(),
            },
            argv: vec!["claude".into(), "--bg".into()],
            env: vec![("A".into(), "b".into())],
            pty_path: Some("/dev/ttys009".into()),
            exit_code: None,
            hold_socket: Some("/tmp/hold-s.sock".into()),
        }
    }

    #[test]
    fn upsert_read_end_orphan() {
        let store = Store::open_in_memory().unwrap();
        store.upsert_session(&rec("s1")).unwrap();
        store.upsert_session(&rec("s2")).unwrap();
        let got = store.session(&SessionId("s1".into())).unwrap().unwrap();
        assert_eq!(got, rec("s1"));
        assert_eq!(store.live_sessions().unwrap().len(), 2);

        store
            .end_session(&SessionId("s1".into()), Some(0), "2026-09-05T11:00:00Z")
            .unwrap();
        let live = store.live_sessions().unwrap();
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].info.id.0, "s2");
        let ended = store.session(&SessionId("s1".into())).unwrap().unwrap();
        assert_eq!(ended.info.state, AgentState::Stopped);
        assert_eq!(ended.exit_code, Some(0));
        assert_eq!(ended.info.pid, None);

        store.mark_orphaned(&SessionId("s2".into())).unwrap();
        assert!(
            store
                .session(&SessionId("s2".into()))
                .unwrap()
                .unwrap()
                .info
                .orphaned
        );
        assert_eq!(
            store.all_sessions().unwrap().len(),
            2,
            "orphaned is never dropped"
        );
        assert!(store.session(&SessionId("nope".into())).unwrap().is_none());
    }
}
