//! The daemon's side of `vt-policy`: where the policy files live, what
//! the classifier knows about a session, and the verdict attached to
//! every approval request and to `policy.classify`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use vt_policy::rules::Decision;
use vt_policy::workspace::{self, Effective};
use vt_policy::{Context, FloorReason, Verdict};
use vt_proto::jsonrpc::RpcError;

use crate::registry::Registry;

/// The global `policy.toml`: next to `config.toml`.
pub fn policy_path() -> PathBuf {
    vt_config::load::Paths::default_paths()
        .config
        .with_file_name("policy.toml")
}

/// The nearest ancestor of `cwd` (inclusive) that holds a `.git` entry:
/// the session's worktree for the outside-worktree test. Lexical walk,
/// one `exists` per level.
pub fn git_root(cwd: &Path) -> Option<PathBuf> {
    let mut dir = Some(cwd);
    while let Some(d) = dir {
        if d.join(".git").exists() {
            return Some(d.to_path_buf());
        }
        dir = d.parent();
    }
    None
}

/// The effective policy for a worktree. A file that fails to load is
/// reported and the default policy applies; the default is the manual
/// path, so a broken file never loosens anything.
pub fn effective(worktree: Option<&Path>) -> Effective {
    match workspace::load(&policy_path(), worktree) {
        Ok(e) => {
            for w in &e.warnings {
                eprintln!("vtermd: policy: {w}");
            }
            e
        }
        Err(e) => {
            eprintln!("vtermd: {e}; using the default policy");
            Effective {
                policy: vt_policy::Policy::default(),
                warnings: vec![e.to_string()],
            }
        }
    }
}

/// What the classifier knows about a session's shell.
pub fn context(cwd: &Path, effective: &Effective) -> Context {
    Context {
        cwd: cwd.to_path_buf(),
        worktree: git_root(cwd),
        home: std::env::var_os("HOME").map(PathBuf::from),
        known_hosts: std::collections::BTreeSet::new(),
        protected_branches: effective.policy.safety.protected_branches.clone(),
    }
}

/// A command line's verdict, floor and `[safety]` decision.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Classified {
    /// The classification.
    pub verdict: Verdict,
    /// Why it can never be auto-approved, when it cannot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub floor: Option<FloorReason>,
    /// What `[safety]` says to do when a human runs it.
    pub decision: Decision,
}

/// Classifies `command` as run from `cwd` under the session's policy.
pub fn classify(command: &str, cwd: &Path, generic_adapter: bool) -> Classified {
    let effective = effective(git_root(cwd).as_deref());
    let ctx = context(cwd, &effective);
    let verdict = vt_policy::classify(command, &ctx);
    let floor = vt_policy::floor::check(&verdict, generic_adapter);
    let decision = effective.policy.safety.decision(verdict.class);
    Classified {
        verdict,
        floor,
        decision,
    }
}

/// `policy.classify { command, session?, cwd? }`.
pub fn classify_rpc(
    registry: &Arc<Registry>,
    command: &str,
    session: Option<&str>,
    cwd: Option<&str>,
) -> Result<serde_json::Value, RpcError> {
    let handle = session.and_then(|s| registry.find(s));
    let (session_cwd, generic) = handle.map_or((None, false), |h| {
        let info = h
            .info
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        (
            Some(info.cwd.clone()),
            info.agent == vt_proto::agent::AgentKind::Generic,
        )
    });
    let cwd = cwd
        .map(PathBuf::from)
        .or(session_cwd)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("/"));
    serde_json::to_value(classify(command, &cwd, generic))
        .map_err(|e| RpcError::new(RpcError::INTERNAL, e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_root_walks_up() {
        let dir = std::env::temp_dir().join(format!("vt-git-root-{}", std::process::id()));
        let nested = dir.join("a").join("b");
        std::fs::create_dir_all(&nested).unwrap();
        assert_eq!(git_root(&nested), None);
        std::fs::write(dir.join(".git"), "gitdir: elsewhere").unwrap();
        assert_eq!(git_root(&nested), Some(dir.clone()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
