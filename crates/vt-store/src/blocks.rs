//! Command blocks (OSC 133/633 segmentation). Persisted so a reopened
//! session shows its history as a timeline, not a wall of text.

use rusqlite::params;
use vt_blocks::{Block, BlockKind, Confidence};
use vt_proto::session::SessionId;

use crate::error::StoreError;
use crate::schema::Store;

/// A stored block with its row id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredBlock {
    /// Row id, ascending with insertion.
    pub seq: i64,
    /// The block.
    pub block: Block,
    /// User bookmark (jump target, listed in the gutter).
    pub bookmarked: bool,
}

fn kind_str(kind: &BlockKind) -> (&'static str, Option<String>, Option<i32>) {
    match kind {
        BlockKind::Prompt => ("prompt", None, None),
        BlockKind::Command { cmdline, exit } => ("command", cmdline.clone(), *exit),
    }
}

fn confidence_str(c: Confidence) -> &'static str {
    match c {
        Confidence::Marked => "marked",
        Confidence::Heuristic => "heuristic",
    }
}

impl Store {
    /// Append a closed block; returns its row id.
    pub fn append_block(
        &self,
        session: &SessionId,
        at: &str,
        block: &Block,
    ) -> Result<i64, StoreError> {
        let (kind, cmdline, exit) = kind_str(&block.kind);
        self.conn
            .execute(
                "INSERT INTO blocks (session_id, kind, confidence, start_line, end_line, cmdline, exit_code, started_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    session.0,
                    kind,
                    confidence_str(block.confidence),
                    i64::try_from(block.start_line).unwrap_or(i64::MAX),
                    block.end_line.map(|l| i64::try_from(l).unwrap_or(i64::MAX)),
                    cmdline,
                    exit,
                    at,
                ],
            )
            .map_err(|source| StoreError::Query { what: "append block", source })?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Set or clear a block's bookmark. `Ok(false)` when no such block.
    pub fn set_block_bookmark(&self, seq: i64, on: bool) -> Result<bool, StoreError> {
        let n = self
            .conn
            .execute(
                "UPDATE blocks SET bookmarked=?2 WHERE seq=?1",
                params![seq, i32::from(on)],
            )
            .map_err(|source| StoreError::Query {
                what: "set bookmark",
                source,
            })?;
        Ok(n == 1)
    }

    /// Blocks for a session after `after_seq`, oldest first, at most `limit`.
    pub fn blocks(
        &self,
        session: &SessionId,
        after_seq: i64,
        limit: usize,
    ) -> Result<Vec<StoredBlock>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT seq, kind, confidence, start_line, end_line, cmdline, exit_code, bookmarked \
                 FROM blocks WHERE session_id=?1 AND seq>?2 ORDER BY seq LIMIT ?3",
            )
            .map_err(|source| StoreError::Query {
                what: "prepare blocks",
                source,
            })?;
        let rows = stmt
            .query_map(
                params![
                    session.0,
                    after_seq,
                    i64::try_from(limit).unwrap_or(i64::MAX)
                ],
                |row| {
                    let seq: i64 = row.get(0)?;
                    let kind: String = row.get(1)?;
                    let confidence: String = row.get(2)?;
                    let start: i64 = row.get(3)?;
                    let end: Option<i64> = row.get(4)?;
                    let cmdline: Option<String> = row.get(5)?;
                    let exit: Option<i32> = row.get(6)?;
                    let bookmarked: i32 = row.get(7)?;
                    Ok((
                        seq,
                        kind,
                        confidence,
                        start,
                        end,
                        cmdline,
                        exit,
                        bookmarked != 0,
                    ))
                },
            )
            .map_err(|source| StoreError::Query {
                what: "query blocks",
                source,
            })?;
        let mut out = Vec::new();
        for row in rows {
            let (seq, kind, confidence, start, end, cmdline, exit, bookmarked) =
                row.map_err(|source| StoreError::Query {
                    what: "read block",
                    source,
                })?;
            let kind = match kind.as_str() {
                "command" => BlockKind::Command { cmdline, exit },
                _ => BlockKind::Prompt,
            };
            out.push(StoredBlock {
                seq,
                block: Block {
                    kind,
                    confidence: if confidence == "marked" {
                        Confidence::Marked
                    } else {
                        Confidence::Heuristic
                    },
                    start_line: u64::try_from(start).unwrap_or(0),
                    end_line: end.map(|e| u64::try_from(e).unwrap_or(0)),
                },
                bookmarked,
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: &SessionId) -> crate::sessions::SessionRecord {
        use vt_proto::agent::{AgentKind, AgentState};
        use vt_proto::session::{Capabilities, SessionInfo};
        crate::sessions::SessionRecord {
            info: SessionInfo {
                id: id.clone(),
                name: "s".into(),
                agent: AgentKind::Generic,
                state: AgentState::Idle,
                capabilities: Capabilities::default(),
                cwd: "/".into(),
                pid: None,
                size: None,
                orphaned: false,
                readopted: false,
                degraded: None,
                created_at: "t".into(),
            },
            argv: vec![],
            env: vec![],
            pty_path: None,
            exit_code: None,
            hold_socket: None,
            agent_token: None,
        }
    }

    #[test]
    fn blocks_round_trip_in_order() {
        let store = Store::open_in_memory().unwrap();
        let sid = SessionId("s".into());
        store.upsert_session(&record(&sid)).unwrap();
        let a = Block {
            kind: BlockKind::Prompt,
            confidence: Confidence::Marked,
            start_line: 0,
            end_line: Some(0),
        };
        let b = Block {
            kind: BlockKind::Command {
                cmdline: Some("ls".into()),
                exit: Some(2),
            },
            confidence: Confidence::Heuristic,
            start_line: 1,
            end_line: Some(4),
        };
        store
            .append_block(&sid, "2026-09-06T00:00:00Z", &a)
            .unwrap();
        store
            .append_block(&sid, "2026-09-06T00:00:01Z", &b)
            .unwrap();
        let all = store.blocks(&sid, 0, 10).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].block, a);
        assert_eq!(all[1].block, b);
        let after = store.blocks(&sid, all[0].seq, 10).unwrap();
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].block, b);

        assert!(!all[1].bookmarked);
        assert!(store.set_block_bookmark(all[1].seq, true).unwrap());
        assert!(store.blocks(&sid, 0, 10).unwrap()[1].bookmarked);
        assert!(store.set_block_bookmark(all[1].seq, false).unwrap());
        assert!(!store.blocks(&sid, 0, 10).unwrap()[1].bookmarked);
        assert!(
            !store.set_block_bookmark(9_999, true).unwrap(),
            "unknown seq"
        );
    }
}
