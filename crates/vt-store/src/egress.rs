//! Every payload that left the machine, in redacted form: provider, size,
//! redaction count. Retention default 365 days. This is what makes a
//! redaction miss discoverable after the fact.

use rusqlite::params;

use crate::error::StoreError;
use crate::schema::Store;

/// One outbound request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EgressRecord {
    /// RFC 3339 time.
    pub at: String,
    /// Provider profile name.
    pub provider: String,
    /// Model id as sent.
    pub model: String,
    /// `suggest` | `ask` | `explain` | `classify`.
    pub purpose: String,
    /// Bytes on the wire after redaction.
    pub bytes_sent: u64,
    /// Number of redactions applied.
    pub redactions: u64,
    /// The redacted payload, when the user opted to retain it.
    pub payload: Option<String>,
}

impl Store {
    /// Record an outbound request.
    pub fn record_egress(&self, rec: &EgressRecord) -> Result<i64, StoreError> {
        self.conn
            .execute(
                "INSERT INTO egress (at, provider, model, purpose, bytes_sent, redactions, payload) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![rec.at, rec.provider, rec.model, rec.purpose, i64::try_from(rec.bytes_sent).unwrap_or(i64::MAX), i64::try_from(rec.redactions).unwrap_or(i64::MAX), rec.payload],
            )
            .map_err(|source| StoreError::Query { what: "record egress", source })?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Most recent egress records, newest first.
    pub fn recent_egress(&self, limit: usize) -> Result<Vec<EgressRecord>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT at, provider, model, purpose, bytes_sent, redactions, payload FROM egress ORDER BY seq DESC LIMIT ?1")
            .map_err(|source| StoreError::Query { what: "prepare egress", source })?;
        let rows = stmt
            .query_map(params![i64::try_from(limit).unwrap_or(i64::MAX)], |r| {
                Ok(EgressRecord {
                    at: r.get(0)?,
                    provider: r.get(1)?,
                    model: r.get(2)?,
                    purpose: r.get(3)?,
                    bytes_sent: r.get::<_, i64>(4)?.unsigned_abs(),
                    redactions: r.get::<_, i64>(5)?.unsigned_abs(),
                    payload: r.get(6)?,
                })
            })
            .map_err(|source| StoreError::Query {
                what: "egress",
                source,
            })?;
        rows.collect::<Result<_, _>>()
            .map_err(|source| StoreError::Query {
                what: "read egress",
                source,
            })
    }
}
