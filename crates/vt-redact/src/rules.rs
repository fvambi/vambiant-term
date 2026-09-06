//! The rule set: secret shapes we redact before any byte leaves the machine.
//! Written from the public formats of each credential (prefixes and
//! alphabets), not copied from any scanner. Recall wins every tie with
//! precision: a false positive costs a `[REDACTED]`, a false negative
//! costs a key.

use regex::{Regex, RegexSet};

/// One rule: a name for the placeholder and the pattern. `value_group`
/// names the capture holding the secret when only part of the match is
/// sensitive (`password=hunter2` keeps `password=`).
pub struct Rule {
    /// Placeholder name: `[REDACTED:<name>]`.
    pub name: &'static str,
    /// The regex.
    pub pattern: &'static str,
    /// Capture group holding the secret, when the whole match is not it.
    pub value_group: Option<usize>,
}

/// Every rule, in match priority order.
pub const RULES: &[Rule] = &[
    Rule {
        name: "private-key",
        pattern: r"-----BEGIN [A-Z ]*PRIVATE KEY( BLOCK)?-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY( BLOCK)?-----",
        value_group: None,
    },
    Rule {
        name: "aws-access-key-id",
        pattern: r"\b(?:AKIA|ASIA|AGPA|AIDA|AROA|AIPA|ANPA|ANVA)[0-9A-Z]{16}\b",
        value_group: None,
    },
    Rule {
        name: "aws-secret-access-key",
        pattern: r#"(?i)aws_?secret_?access_?key\s*[=:]\s*['"]?([0-9A-Za-z/+]{40})"#,
        value_group: Some(1),
    },
    Rule {
        name: "github-token",
        pattern: r"\b(?:ghp|gho|ghu|ghs|ghr)_[0-9A-Za-z]{36,255}\b",
        value_group: None,
    },
    Rule {
        name: "github-fine-grained-token",
        pattern: r"\bgithub_pat_[0-9A-Za-z_]{22,255}\b",
        value_group: None,
    },
    Rule {
        name: "anthropic-api-key",
        pattern: r"\bsk-ant-[0-9A-Za-z\-_]{20,}\b",
        value_group: None,
    },
    Rule {
        name: "openai-api-key",
        pattern: r"\bsk-(?:proj-|svcacct-)?[0-9A-Za-z_\-]{20,}\b",
        value_group: None,
    },
    Rule {
        name: "slack-token",
        pattern: r"\bxox[abprs]-[0-9A-Za-z\-]{10,}\b",
        value_group: None,
    },
    Rule {
        name: "google-api-key",
        pattern: r"\bAIza[0-9A-Za-z\-_]{35}\b",
        value_group: None,
    },
    Rule {
        name: "stripe-key",
        pattern: r"\b(?:sk|rk|pk)_(?:live|test)_[0-9A-Za-z]{16,}\b",
        value_group: None,
    },
    Rule {
        name: "jwt",
        pattern: r"\beyJ[0-9A-Za-z_\-]{8,}\.[0-9A-Za-z_\-]{8,}\.[0-9A-Za-z_\-]{8,}\b",
        value_group: None,
    },
    Rule {
        name: "bearer-token",
        pattern: r"(?i)\bbearer\s+([A-Za-z0-9\-._~+/]{20,}=*)",
        value_group: Some(1),
    },
    Rule {
        name: "url-credentials",
        pattern: r"(?i)\b[a-z][a-z0-9+.\-]*://[^/\s:@]+:([^@\s/]{3,})@",
        value_group: Some(1),
    },
    Rule {
        name: "assignment",
        pattern: r#"(?i)\b(?:api[_\-]?key|secret[_\-]?key|secret|access[_\-]?token|auth[_\-]?token|token|password|passwd|pwd)\b['"]?\s*[=:]\s*['"]?([^\s'"]{8,})"#,
        value_group: Some(1),
    },
];

/// Compiled rules. Compilation is the one thing that can fail here, and
/// it fails at startup, not at send time.
pub struct Compiled {
    /// Every rule at once, to skip clean payloads in one pass.
    pub set: RegexSet,
    /// The same rules individually, for the replacement pass.
    pub regexes: Vec<Regex>,
}

impl Compiled {
    /// Compiles every rule; a bad pattern is a startup failure.
    pub fn new() -> Result<Self, regex::Error> {
        let set = RegexSet::new(RULES.iter().map(|r| r.pattern))?;
        let regexes = RULES
            .iter()
            .map(|r| Regex::new(r.pattern))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { set, regexes })
    }
}
