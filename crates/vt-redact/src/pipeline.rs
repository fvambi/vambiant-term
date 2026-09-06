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
        let (out, replacements) = redact_entropy(text);
        return Ok(Redacted {
            text: out,
            replacements,
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
    let (out, extra) = redact_entropy(&out);
    Ok(Redacted {
        text: out,
        replacements: replacements + extra,
    })
}

/// Layer 2 (docs/05 §3.3): unprefixed high-entropy values. A token of 32+
/// characters drawn from three or more character classes with Shannon
/// entropy of 3.8 bits per character or more is a credential shape no
/// prefix rule knows (a bare AWS secret, a random API key). Hex digests
/// (two classes) and paths (a `/` with no other symbol class) survive.
pub fn looks_random(token: &str) -> bool {
    let t = token.trim_matches(|c: char| ",.;)]}>'\"".contains(c));
    if t.chars().count() < 32 || t.starts_with("http") || t.starts_with('/') || t.starts_with('~') {
        return false;
    }
    let (mut upper, mut lower, mut digit, mut symbol) = (false, false, false, false);
    for c in t.chars() {
        match c {
            'A'..='Z' => upper = true,
            'a'..='z' => lower = true,
            '0'..='9' => digit = true,
            '+' | '/' | '=' | '-' | '_' | '.' => symbol = true,
            _ => return false,
        }
    }
    let classes = [upper, lower, digit, symbol].iter().filter(|b| **b).count();
    if classes < 3 || t.matches('/').count() > 3 {
        return false;
    }
    let mut counts = std::collections::HashMap::new();
    for c in t.chars() {
        *counts.entry(c).or_insert(0u32) += 1;
    }
    let n = f64::from(u32::try_from(t.chars().count()).unwrap_or(u32::MAX));
    let entropy: f64 = counts
        .values()
        .map(|&k| {
            let p = f64::from(k) / n;
            -p * p.log2()
        })
        .sum();
    entropy >= 3.8
}

/// Replaces every random-looking token with `[REDACTED:entropy]`.
pub(crate) fn redact_entropy(text: &str) -> (String, usize) {
    let mut out = String::with_capacity(text.len());
    let mut replacements = 0;
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let mut first = true;
        for word in line.split(' ') {
            if !first {
                out.push(' ');
            }
            first = false;
            if looks_random(word) {
                let trimmed = word.trim_matches(|c: char| ",.;)]}>'\"".contains(c));
                let (lead, rest) = word.split_at(word.find(trimmed).unwrap_or(0));
                let tail = &rest[trimmed.len()..];
                out.push_str(lead);
                out.push_str("[REDACTED:entropy]");
                out.push_str(tail);
                replacements += 1;
            } else {
                out.push_str(word);
            }
        }
    }
    (out, replacements)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(s: &str) -> Redacted {
        redact(s).unwrap()
    }

    #[test]
    fn entropy_catches_bare_secrets_and_spares_hashes_and_paths() {
        let key = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";
        assert!(looks_random(key), "an AWS secret has no prefix");
        let r = redact(&format!(
            "export AWS_SECRET_ACCESS_KEY={key}\nsha 3b18e512dba79e4c8300dd08aeb37f8e728b8dad"
        ))
        .unwrap();
        assert!(!r.text.contains(key), "{}", r.text);
        assert!(
            r.text.contains("3b18e512dba79e4c8300dd08aeb37f8e728b8dad"),
            "a git hash survives: {}",
            r.text
        );
        let r = redact("token: XyZ9aB3cD4eF5gH6iJ7kL8mN9oP0qR1sT2uV3wX4").unwrap();
        assert!(r.text.contains("[REDACTED:"), "{}", r.text);
        for clean in [
            "/Users/me/Library/Application Support/Code/User/settings.json",
            "https://example.com/a/very/long/path/that/is/not/a/secret/at/all",
            "the quick brown fox jumps over the lazy dog again and again",
            "0123456789abcdef0123456789abcdef0123456789abcdef",
            "ThisIsAPerfectlyNormalCamelCaseIdentifierName",
        ] {
            assert!(!looks_random(clean), "{clean}");
            assert_eq!(redact(clean).unwrap().replacements, 0, "{clean}");
        }
        let json = r#"{"secret_key": "hunter2hunter2", "debug": true}"#;
        let r = redact(json).unwrap();
        assert!(
            !r.text.contains("hunter2hunter2") && r.text.contains("\"secret_key\""),
            "{}",
            r.text
        );
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
