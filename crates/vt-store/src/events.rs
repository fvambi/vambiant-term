//! Append-only agent event log.

use rusqlite::params;
use vt_proto::agent::AgentEvent;
use vt_proto::session::SessionId;

use crate::error::StoreError;
use crate::schema::Store;

impl Store {
    /// Append one event; returns its sequence number.
    pub fn append_event(
        &self,
        session: &SessionId,
        at: &str,
        event: &AgentEvent,
    ) -> Result<i64, StoreError> {
        let payload = serde_json::to_string(event).map_err(|e| StoreError::Query {
            what: "serialise event",
            source: rusqlite::Error::ToSqlConversionFailure(Box::new(e)),
        })?;
        let kind = serde_json::to_value(event)
            .ok()
            .and_then(|v| v.get("type").and_then(|t| t.as_str().map(str::to_owned)))
            .unwrap_or_else(|| "unknown".into());
        self.conn
            .execute(
                "INSERT INTO agent_events (session_id, at, kind, payload) VALUES (?1, ?2, ?3, ?4)",
                params![session.0, at, kind, payload],
            )
            .map_err(|source| StoreError::Query {
                what: "append event",
                source,
            })?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Events for a session after `after_seq`, oldest first, at most `limit`.
    pub fn events(
        &self,
        session: &SessionId,
        after_seq: i64,
        limit: usize,
    ) -> Result<Vec<(i64, AgentEvent)>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT seq, payload FROM agent_events WHERE session_id=?1 AND seq>?2 ORDER BY seq LIMIT ?3")
            .map_err(|source| StoreError::Query { what: "prepare events", source })?;
        let rows = stmt
            .query_map(
                params![
                    session.0,
                    after_seq,
                    i64::try_from(limit).unwrap_or(i64::MAX)
                ],
                |r| {
                    let seq: i64 = r.get(0)?;
                    let payload: String = r.get(1)?;
                    Ok((seq, payload))
                },
            )
            .map_err(|source| StoreError::Query {
                what: "events",
                source,
            })?;
        let mut out = Vec::new();
        for row in rows {
            let (seq, payload) = row.map_err(|source| StoreError::Query {
                what: "read event",
                source,
            })?;
            // A payload this binary cannot decode becomes `Unknown`, never a
            // dropped row: the log outlives the schema (ADR-0006).
            let event = serde_json::from_str(&payload).unwrap_or_else(|_| AgentEvent::Unknown {
                name: "undecodable".into(),
                payload: serde_json::from_str(&payload).unwrap_or(serde_json::Value::Null),
            });
            out.push((seq, event));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::SessionRecord;
    use vt_proto::agent::{AgentKind, AgentState};
    use vt_proto::session::{Capabilities, SessionInfo};

    #[test]
    fn append_and_page() {
        let store = Store::open_in_memory().unwrap();
        let id = SessionId("s".into());
        store
            .upsert_session(&SessionRecord {
                info: SessionInfo {
                    id: id.clone(),
                    name: "s".into(),
                    agent: AgentKind::Codex,
                    state: AgentState::Starting,
                    capabilities: Capabilities::default(),
                    cwd: "/".into(),
                    pid: None,
                    size: None,
                    orphaned: false,
                    readopted: false,
                    created_at: "t".into(),
                },
                argv: vec![],
                env: vec![],
                pty_path: None,
                exit_code: None,
                hold_socket: None,
                agent_token: None,
            })
            .unwrap();
        let e1 = AgentEvent::Notification {
            title: None,
            body: "hi".into(),
        };
        let e2 = AgentEvent::SessionEnded {
            reason: "done".into(),
        };
        let s1 = store.append_event(&id, "t1", &e1).unwrap();
        let s2 = store.append_event(&id, "t2", &e2).unwrap();
        assert!(s2 > s1);
        let page = store.events(&id, 0, 10).unwrap();
        assert_eq!(page.len(), 2);
        assert_eq!(page[1].1, e2);
        assert_eq!(store.events(&id, s1, 10).unwrap().len(), 1);
        // Foreign-key cascade: deleting the session removes its events.
        store
            .conn
            .execute("DELETE FROM sessions WHERE id='s'", [])
            .unwrap();
        assert!(store.events(&id, 0, 10).unwrap().is_empty());
    }
}
