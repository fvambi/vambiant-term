//! Safety classes (docs/05 §5.2) and the verdict for one command line.
//! Every simple command in the parsed tree is checked; the worst class
//! wins; a parse failure is [`SafetyClass::Unparseable`] and never benign.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::checks;
use crate::shell::{self, Program};

/// Ordered from most to least benign: comparison is meaningful and the
/// worst finding decides the verdict.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafetyClass {
    /// Everything not matched below.
    Benign,
    /// `curl`, `wget`, `nc`, `scp`, `rsync`, `ssh` to a host not seen before.
    NetworkEgress,
    /// `sudo`, `chmod 777`, `chown root`, `launchctl load`, writes to system paths.
    Privilege,
    /// `git push --force`, `terraform apply`, `kubectl delete`, publishes.
    IrreversibleRemote,
    /// `rm -rf`, `truncate`, `dd of=`, `mkfs`, `DROP TABLE`, `git reset --hard`.
    Destructive,
    /// `cat .env`, `env`, `security find-generic-password`, `op read`.
    CredentialRead,
    /// `curl … | sh`, `eval "$X"`, `base64 -d | bash`, a verb from an expansion.
    Obfuscated,
    /// The shell reader could not follow the input.
    Unparseable,
}

impl SafetyClass {
    /// The `policy.toml [safety]` key for this class.
    pub fn key(self) -> &'static str {
        match self {
            Self::Benign => "benign",
            Self::NetworkEgress => "network_egress",
            Self::Privilege => "privilege",
            Self::IrreversibleRemote => "irreversible_remote",
            Self::Destructive => "destructive",
            Self::CredentialRead => "credential_read",
            Self::Obfuscated => "obfuscated",
            Self::Unparseable => "unparseable",
        }
    }
}

impl std::fmt::Display for SafetyClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.key())
    }
}

/// What the classifier knows about where the command runs.
#[derive(Clone, Debug, Default)]
pub struct Context {
    /// The session's working directory (relative paths resolve here).
    pub cwd: PathBuf,
    /// The session's worktree; destructive targets outside it hit the floor.
    /// `None` means no worktree is known and every destructive target is outside.
    pub worktree: Option<PathBuf>,
    /// `~` expands here.
    pub home: Option<PathBuf>,
    /// Hosts this repo has talked to before; egress to them is not new.
    pub known_hosts: BTreeSet<String>,
    /// Branches a force-push may never target (globs like `release/*`).
    pub protected_branches: Vec<String>,
}

/// One rule that fired, with the token it fired on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    /// The class this finding contributes.
    pub class: SafetyClass,
    /// Stable rule id, e.g. `rm-recursive`.
    pub rule: String,
    /// The command or argument that triggered it.
    pub token: String,
    /// What would happen, for the confirm dialog.
    pub detail: String,
    /// For destructive findings: the target is outside the worktree (or
    /// cannot be resolved, which counts as outside).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub outside_worktree: bool,
    /// For force pushes: the branch is protected (or unknown, which counts).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub protected_branch: bool,
    /// For egress: the host, when one could be read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
}

/// The classification of one command line.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verdict {
    /// The worst class among the findings.
    pub class: SafetyClass,
    /// Every rule that fired, worst first.
    pub findings: Vec<Finding>,
    /// Set when the input could not be parsed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parse_error: Option<String>,
    /// Simple commands seen in the tree.
    pub commands: usize,
}

impl Verdict {
    /// One line for a status row: `destructive: rm-recursive on /`.
    pub fn summary(&self) -> String {
        match self.findings.first() {
            Some(f) => format!("{}: {} on `{}`", self.class, f.rule, f.token),
            None if self.parse_error.is_some() => {
                format!("unparseable: {}", self.parse_error.as_deref().unwrap_or(""))
            }
            None => "benign".into(),
        }
    }
}

/// Classifies a command line.
pub fn classify(cmdline: &str, ctx: &Context) -> Verdict {
    let program = match shell::parse(cmdline) {
        Ok(p) => p,
        Err(e) => {
            return Verdict {
                class: SafetyClass::Unparseable,
                findings: vec![Finding {
                    class: SafetyClass::Unparseable,
                    rule: "parse-failed".into(),
                    token: cmdline.chars().take(80).collect(),
                    detail: e.to_string(),
                    outside_worktree: true,
                    protected_branch: false,
                    host: None,
                }],
                parse_error: Some(e.to_string()),
                commands: 0,
            };
        }
    };
    let mut findings = Vec::new();
    let commands = classify_program(&program, ctx, &mut findings, 0);
    findings.sort_by_key(|f| std::cmp::Reverse(f.class));
    let class = findings.first().map_or(SafetyClass::Benign, |f| f.class);
    Verdict {
        class,
        findings,
        parse_error: None,
        commands,
    }
}

/// Checks every pipeline and simple command of `program`, recursing into
/// the command lines that wrappers (`sh -c`, `eval`, `sudo`, `xargs`…)
/// hand to another shell.
pub(crate) fn classify_program(
    program: &Program,
    ctx: &Context,
    findings: &mut Vec<Finding>,
    depth: usize,
) -> usize {
    if depth > 16 {
        findings.push(Finding {
            class: SafetyClass::Obfuscated,
            rule: "nesting".into(),
            token: "…".into(),
            detail: "command lines nested more than 16 levels deep".into(),
            outside_worktree: true,
            protected_branch: false,
            host: None,
        });
        return 0;
    }
    for pipeline in program.pipelines_deep() {
        checks::pipeline(pipeline, findings);
    }
    let mut count = 0;
    program.walk(&mut |simple, _| {
        count += 1;
        checks::simple(simple, ctx, findings, depth);
    });
    count
}

/// Lexical path resolution against `cwd` and `home`; no filesystem access.
pub(crate) fn resolve(path: &str, ctx: &Context) -> PathBuf {
    let expanded = if path == "~" || path.starts_with("~/") {
        match &ctx.home {
            Some(h) => h.join(path.trim_start_matches('~').trim_start_matches('/')),
            None => PathBuf::from(path),
        }
    } else {
        PathBuf::from(path)
    };
    let joined = if expanded.is_absolute() {
        expanded
    } else {
        ctx.cwd.join(expanded)
    };
    let mut out = PathBuf::new();
    for comp in joined.components() {
        match comp {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            c => out.push(c),
        }
    }
    out
}

/// Whether `path` (already resolved) lies inside the worktree.
pub(crate) fn inside_worktree(path: &Path, ctx: &Context) -> bool {
    ctx.worktree
        .as_ref()
        .is_some_and(|w| path.starts_with(w) && path != w.as_path())
}
