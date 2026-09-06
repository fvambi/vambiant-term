//! The fail-closed entry point.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use crate::rules::{Compiled, RULES};

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

/// Default time budget for one payload.
pub const DEFAULT_BUDGET: Duration = Duration::from_millis(250);

fn compiled() -> Result<&'static Compiled, RedactError> {
    static CELL: OnceLock<Option<Compiled>> = OnceLock::new();
    CELL.get_or_init(|| Compiled::new().ok())
        .as_ref()
        .ok_or(RedactError::Internal { stage: "rules" })
}

/// Redacts `text` in place of every rule match, or refuses. A panic in
/// any layer is caught and reported as `Internal`; exceeding `budget`
/// is `Timeout`. Both mean: do not send.
pub fn redact(text: &str) -> Result<Redacted, RedactError> {
    redact_within(text, DEFAULT_BUDGET)
}

/// [`redact`] with an explicit budget.
pub fn redact_within(text: &str, budget: Duration) -> Result<Redacted, RedactError> {
    let start = Instant::now();
    let rules = compiled()?;
    let result = std::panic::catch_unwind(|| apply(rules, text, start, budget));
    match result {
        Ok(r) => r,
        Err(_) => Err(RedactError::Internal { stage: "apply" }),
    }
}

fn apply(
    rules: &Compiled,
    text: &str,
    start: Instant,
    budget: Duration,
) -> Result<Redacted, RedactError> {
    let hits = rules.set.matches(text);
    if !hits.matched_any() {
        return Ok(Redacted {
            text: text.to_owned(),
            replacements: 0,
        });
    }
    let mut out = text.to_owned();
    let mut replacements = 0usize;
    for idx in &hits {
        if start.elapsed() > budget {
            return Err(RedactError::Timeout {
                ms: u64::try_from(budget.as_millis()).unwrap_or(u64::MAX),
            });
        }
        let rule = &RULES[idx];
        let re = &rules.regexes[idx];
        let placeholder = format!("[REDACTED:{}]", rule.name);
        // Rebuild the string from the right so earlier offsets stay valid.
        let mut spans: Vec<(usize, usize)> = re
            .captures_iter(&out)
            .filter_map(|c| {
                let m = match rule.value_group {
                    Some(g) => c.get(g)?,
                    None => c.get(0)?,
                };
                Some((m.start(), m.end()))
            })
            .collect();
        spans.sort_unstable();
        spans.dedup();
        for (s, e) in spans.into_iter().rev() {
            out.replace_range(s..e, &placeholder);
            replacements += 1;
        }
    }
    Ok(Redacted {
        text: out,
        replacements,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(s: &str) -> Redacted {
        redact(s).unwrap()
    }

    #[test]
    fn every_rule_fires_on_its_shape() {
        let cases = [
            ("AKIAIOSFODNN7EXAMPLE", "aws-access-key-id"),
            (
                "aws_secret_access_key = wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
                "aws-secret-access-key",
            ),
            ("ghp_abcdefghijklmnopqrstuvwxyz0123456789", "github-token"),
            (
                "github_pat_11ABCDEFG0abcdefghijklmnop",
                "github-fine-grained-token",
            ),
            (
                "sk-ant-api03-abcdefghijklmnopqrstuvwxyz",
                "anthropic-api-key",
            ),
            ("sk-proj-abcdefghijklmnopqrstuvwxyz1234", "openai-api-key"),
            ("xoxb-123456789012-abcdefghijkl", "slack-token"),
            ("AIzaSyA1234567890abcdefghijklmnopqrstuv", "google-api-key"),
            ("sk_live_abcdefghijklmnop1234", "stripe-key"),
            (
                "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0In0.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV",
                "jwt",
            ),
            (
                "Authorization: Bearer abcdefghijklmnopqrstuvwxyz012345",
                "bearer-token",
            ),
            (
                "postgres://user:s3cretpassw0rd@db.internal/app",
                "url-credentials",
            ),
            ("export API_KEY=abcdefgh12345678", "assignment"),
            (
                "-----BEGIN RSA PRIVATE KEY-----\nMIIEow\nAB==\n-----END RSA PRIVATE KEY-----",
                "private-key",
            ),
        ];
        for (input, rule) in cases {
            let out = r(input);
            assert!(
                out.replacements >= 1,
                "{rule}: nothing redacted in {input:?}"
            );
            assert!(
                out.text.contains(&format!("[REDACTED:{rule}]")),
                "{rule}: {}",
                out.text
            );
        }
    }

    #[test]
    fn partial_rules_keep_the_key_name() {
        let out = r("password=hunter2hunter2 user=bob");
        assert_eq!(out.text, "password=[REDACTED:assignment] user=bob");
        let out = r("git clone https://alice:tok3n_value@github.com/x/y");
        assert_eq!(
            out.text,
            "git clone https://alice:[REDACTED:url-credentials]@github.com/x/y"
        );
    }

    #[test]
    fn clean_text_passes_through_untouched() {
        let out = r("ls -la && cargo test --workspace");
        assert_eq!(out.replacements, 0);
        assert_eq!(out.text, "ls -la && cargo test --workspace");
        assert_eq!(r("").text, "");
    }

    #[test]
    fn a_zero_budget_fails_closed_when_there_is_work() {
        let err = redact_within("token=abcdefghijklmnop", Duration::ZERO).unwrap_err();
        assert!(matches!(err, RedactError::Timeout { .. }), "{err}");
        assert!(err.to_string().contains("nothing was sent"));
        // No matches: nothing to time out on.
        assert!(redact_within("plain", Duration::ZERO).is_ok());
    }
}
