//! `policy.toml` (docs/09) and rule evaluation on the hot path of a
//! permission request. Autonomy ships off and in dry-run; the floor in
//! [`crate::floor`] is applied after every rule and cannot be configured.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::classify::{Context, SafetyClass, Verdict, classify};
use crate::floor::{self, FloorReason, NEVER_ALLOW};

/// What the terminal does with a class when a human types it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Decision {
    /// No prompt.
    Allow,
    /// Runs, with a visible warning.
    Warn,
    /// A confirm dialog first.
    Confirm,
    /// Refused.
    Block,
}

/// `[safety]`: per-class decisions for typed commands.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[allow(missing_docs)] // the keys are the classes, documented on SafetyClass
pub struct Safety {
    pub destructive: Decision,
    pub irreversible_remote: Decision,
    pub credential_read: Decision,
    pub network_egress: Decision,
    pub privilege: Decision,
    pub obfuscated: Decision,
    pub unparseable: Decision,
    /// Branches a force-push may never target (globs).
    pub protected_branches: Vec<String>,
}

impl Default for Safety {
    fn default() -> Self {
        Self {
            destructive: Decision::Confirm,
            irreversible_remote: Decision::Confirm,
            credential_read: Decision::Confirm,
            network_egress: Decision::Warn,
            privilege: Decision::Confirm,
            obfuscated: Decision::Block,
            unparseable: Decision::Confirm,
            protected_branches: ["main", "master", "develop", "release/*"]
                .map(String::from)
                .to_vec(),
        }
    }
}

impl Safety {
    /// The decision for a class; benign is always allow.
    pub fn decision(&self, class: SafetyClass) -> Decision {
        match class {
            SafetyClass::Benign => Decision::Allow,
            SafetyClass::NetworkEgress => self.network_egress,
            SafetyClass::Privilege => self.privilege,
            SafetyClass::IrreversibleRemote => self.irreversible_remote,
            SafetyClass::Destructive => self.destructive,
            SafetyClass::CredentialRead => self.credential_read,
            SafetyClass::Obfuscated => self.obfuscated,
            SafetyClass::Unparseable => self.unparseable,
        }
    }
}

/// What a rule decides for an agent's request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Decide {
    /// Approve without asking (only when autonomy is on and not dry-run).
    Allow,
    /// Route to the inbox: the manual path.
    Ask,
    /// Refuse on the agent's behalf.
    Deny,
}

/// One or many values in TOML (`tool = "Bash"` or `tool = ["Read", "Grep"]`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OneOrMany<T> {
    /// A single value.
    One(T),
    /// Several.
    Many(Vec<T>),
}

impl<T: PartialEq> OneOrMany<T> {
    fn contains(&self, v: &T) -> bool {
        match self {
            Self::One(x) => x == v,
            Self::Many(xs) => xs.contains(v),
        }
    }
}

/// `[[autonomy.rule]].match`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Match {
    /// Repository name (the worktree's directory name).
    pub repo: Option<String>,
    /// Tool name(s) as the agent reports them.
    pub tool: Option<OneOrMany<String>>,
    /// Regex over the command line (Bash tool).
    pub command: Option<String>,
    /// The session runs inside a known worktree.
    pub in_worktree: Option<bool>,
    /// The request touches a path outside the worktree.
    pub outside_worktree: Option<bool>,
}

/// One autonomy rule; first match wins.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    /// Shown in the audit log.
    pub name: String,
    /// When it applies.
    #[serde(rename = "match", default)]
    pub matcher: Match,
    /// What it decides.
    pub decide: Decide,
}

/// `[autonomy]`: every switch defaults off; `dry_run` defaults on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Autonomy {
    /// Master switch.
    pub enabled: bool,
    /// Log what would have been decided without deciding.
    pub dry_run: bool,
    /// Kill a runaway agent (M8).
    pub watchdog: bool,
    /// Detect repeated tool calls (M8).
    pub loop_detect: bool,
    /// Background task queue (M8).
    pub task_queue: bool,
    /// Rules in order.
    #[serde(rename = "rule")]
    pub rules: Vec<Rule>,
}

impl Default for Autonomy {
    fn default() -> Self {
        Self {
            enabled: false,
            dry_run: true,
            watchdog: false,
            loop_detect: false,
            task_queue: false,
            rules: Vec::new(),
        }
    }
}

/// `[egress]` (docs/05 §4.1): what may leave for a provider.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EgressMode {
    /// Nothing leaves.
    None,
    /// Redacted context only.
    Redacted,
    /// Redacted, with the full block text allowed.
    Full,
}

/// `[egress.never_include]`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct NeverInclude {
    /// Path globs whose contents never leave.
    pub paths: Vec<String>,
    /// Command prefixes whose output never leaves.
    pub commands: Vec<String>,
}

/// `[egress]`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Egress {
    /// The mode.
    pub mode: EgressMode,
    /// Profiles allowed; empty means all configured.
    pub allow_providers: Vec<String>,
    /// Cap on context bytes.
    pub max_context_bytes: Option<u64>,
    /// Exclusions.
    pub never_include: NeverInclude,
}

impl Default for Egress {
    fn default() -> Self {
        Self {
            mode: EgressMode::Redacted,
            allow_providers: Vec::new(),
            max_context_bytes: None,
            never_include: NeverInclude::default(),
        }
    }
}

/// A whole policy file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Policy {
    /// `[safety]`.
    pub safety: Safety,
    /// `[autonomy]`.
    pub autonomy: Autonomy,
    /// `[egress]`.
    pub egress: Egress,
}

/// A policy file that cannot be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PolicyError {
    /// Not valid TOML for this schema.
    #[error("policy.toml: {0}")]
    Syntax(String),
    /// A `[safety]` class the floor forbids is set to `allow`.
    #[error(
        "policy.toml: [safety] {class} = \"allow\" is not permitted: the never-auto floor applies to it"
    )]
    FloorViolation {
        /// The class.
        class: SafetyClass,
    },
    /// A rule's `command` regex does not compile.
    #[error("policy.toml: rule `{rule}`: command pattern does not compile: {detail}")]
    BadRegex {
        /// The rule's name.
        rule: String,
        /// The regex error.
        detail: String,
    },
}

impl Policy {
    /// Parses and validates a policy file.
    pub fn parse(text: &str) -> Result<Self, PolicyError> {
        let policy: Self = toml::from_str(text).map_err(|e| PolicyError::Syntax(e.to_string()))?;
        policy.validate()?;
        Ok(policy)
    }

    /// The invariants a file cannot break: floor classes stay unallowed,
    /// regexes compile.
    pub fn validate(&self) -> Result<(), PolicyError> {
        for class in NEVER_ALLOW {
            if self.safety.decision(*class) == Decision::Allow {
                return Err(PolicyError::FloorViolation { class: *class });
            }
        }
        for rule in &self.autonomy.rules {
            if let Some(re) = &rule.matcher.command {
                regex::Regex::new(re).map_err(|e| PolicyError::BadRegex {
                    rule: rule.name.clone(),
                    detail: e.to_string(),
                })?;
            }
        }
        Ok(())
    }
}

/// One permission request as the engine sees it.
#[derive(Clone, Debug)]
pub struct ToolRequest<'a> {
    /// Tool name as the agent reports it (`Bash`, `Edit`, …).
    pub tool: &'a str,
    /// The command line for shell tools.
    pub command: Option<&'a str>,
    /// The file for edit/write tools.
    pub path: Option<&'a Path>,
    /// Repository name (worktree directory name).
    pub repo: Option<&'a str>,
    /// Where the command runs and what it may touch.
    pub context: &'a Context,
    /// The session is observed by the generic adapter.
    pub generic_adapter: bool,
}

/// What the engine decided, and whether that decision was applied.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Outcome {
    /// The decision.
    pub decision: Decide,
    /// True only when autonomy is enabled, not in dry-run, and the
    /// decision is not `ask`; otherwise this is what *would* have happened.
    pub applied: bool,
    /// The rule that decided, if one did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
    /// The command's classification, when there was a command.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verdict: Option<Verdict>,
    /// The floor reason, when the request can never be auto-approved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub floor: Option<FloorReason>,
    /// The `[safety]` decision for the verdict's class.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safety: Option<Decision>,
}

/// Evaluates a request: classify, apply the floor, then the first
/// matching rule. Without a matching rule the answer is `ask`.
pub fn evaluate(policy: &Policy, req: &ToolRequest<'_>) -> Outcome {
    let verdict = req.command.map(|c| classify(c, req.context));
    let safety = verdict.as_ref().map(|v| policy.safety.decision(v.class));
    let floor = match &verdict {
        Some(v) => floor::check(v, req.generic_adapter),
        None if req.generic_adapter => Some(FloorReason::GenericAdapter),
        None => None,
    };
    let outside = req.path.is_some_and(|p| {
        !crate::classify::inside_worktree(
            &crate::classify::resolve(&p.to_string_lossy(), req.context),
            req.context,
        )
    }) || verdict.as_ref().is_some_and(|v| {
        v.findings
            .iter()
            .any(|f| f.outside_worktree && f.class == SafetyClass::Destructive)
    });
    let mut outcome = Outcome {
        decision: Decide::Ask,
        applied: false,
        rule: None,
        verdict,
        floor,
        safety,
    };
    if outcome.floor.is_some() {
        if safety == Some(Decision::Block) {
            outcome.decision = Decide::Deny;
        }
        outcome.applied =
            outcome.decision == Decide::Deny && policy.autonomy.enabled && !policy.autonomy.dry_run;
        return outcome;
    }
    let in_worktree = req
        .context
        .worktree
        .as_ref()
        .is_some_and(|w| req.context.cwd.starts_with(w));
    for rule in &policy.autonomy.rules {
        let m = &rule.matcher;
        if m.repo.as_deref().is_some_and(|r| Some(r) != req.repo) {
            continue;
        }
        if m.tool
            .as_ref()
            .is_some_and(|t| !t.contains(&req.tool.to_owned()))
        {
            continue;
        }
        if let Some(re) = &m.command {
            let Some(cmd) = req.command else { continue };
            if !regex::Regex::new(re).is_ok_and(|re| re.is_match(cmd)) {
                continue;
            }
        }
        if m.in_worktree.is_some_and(|want| want != in_worktree)
            || m.outside_worktree.is_some_and(|want| want != outside)
        {
            continue;
        }
        outcome.rule = Some(rule.name.clone());
        outcome.decision = match rule.decide {
            Decide::Allow if safety == Some(Decision::Block) => Decide::Deny,
            d => d,
        };
        break;
    }
    outcome.applied =
        policy.autonomy.enabled && !policy.autonomy.dry_run && outcome.decision != Decide::Ask;
    outcome
}
