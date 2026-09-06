//! Every payload that left the machine, in redacted form: provider, size,
//! redaction count. Retention default 365 days. This is what makes a
//! redaction miss discoverable after the fact.

use rusqlite::params;

use crate::error::StoreError;
use crate::schema::Store;

/// One outbound request.
#[derive(Clone, Debug, PartialEq)]
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
    /// List-price estimate in USD (docs/04 §7); 0 for local models.
    pub cost_usd: f64,
    /// The session the request was made for, if any.
    pub session: Option<String>,
}

/// Spend over a period (docs/04 §7).
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Spend {
    /// Requests counted.
    pub requests: u64,
    /// Total list-price estimate.
    pub total_usd: f64,
    /// `(purpose, usd)` descending.
    pub by_purpose: Vec<(String, f64)>,
    /// `(provider, usd)` descending.
    pub by_provider: Vec<(String, f64)>,
}

impl Store {
    /// Record an outbound request.
    pub fn record_egress(&self, rec: &EgressRecord) -> Result<i64, StoreError> {
        self.conn
            .execute(
                "INSERT INTO egress (at, provider, model, purpose, bytes_sent, redactions, payload, cost_usd, session_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![rec.at, rec.provider, rec.model, rec.purpose, i64::try_from(rec.bytes_sent).unwrap_or(i64::MAX), i64::try_from(rec.redactions).unwrap_or(i64::MAX), rec.payload, rec.cost_usd, rec.session],
            )
            .map_err(|source| StoreError::Query { what: "record egress", source })?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Most recent egress records, newest first.
    pub fn recent_egress(&self, limit: usize) -> Result<Vec<EgressRecord>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT at, provider, model, purpose, bytes_sent, redactions, payload, cost_usd, session_id FROM egress ORDER BY seq DESC LIMIT ?1")
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
                    cost_usd: r.get(7)?,
                    session: r.get(8)?,
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

impl Store {
    /// Spend since `since` (RFC 3339 prefix compare: `2026-09-06` is a day,
    /// `2026-09` a month), optionally for one session.
    pub fn spend_since(&self, since: &str, session: Option<&str>) -> Result<Spend, StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT purpose, provider, cost_usd FROM egress WHERE at >= ?1 AND (?2 IS NULL OR session_id = ?2)",
            )
            .map_err(|source| StoreError::Query { what: "prepare spend", source })?;
        let rows = stmt
            .query_map(params![since, session], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, f64>(2)?,
                ))
            })
            .map_err(|source| StoreError::Query {
                what: "spend",
                source,
            })?;
        let mut spend = Spend {
            requests: 0,
            total_usd: 0.0,
            by_purpose: Vec::new(),
            by_provider: Vec::new(),
        };
        let mut purposes = std::collections::BTreeMap::new();
        let mut providers = std::collections::BTreeMap::new();
        for row in rows {
            let (purpose, provider, cost) = row.map_err(|source| StoreError::Query {
                what: "read spend",
                source,
            })?;
            spend.requests += 1;
            spend.total_usd += cost;
            *purposes.entry(purpose).or_insert(0.0) += cost;
            *providers.entry(provider).or_insert(0.0) += cost;
        }
        let sorted = |m: std::collections::BTreeMap<String, f64>| {
            let mut v: Vec<(String, f64)> = m.into_iter().collect();
            v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            v
        };
        spend.by_purpose = sorted(purposes);
        spend.by_provider = sorted(providers);
        Ok(spend)
    }
}
