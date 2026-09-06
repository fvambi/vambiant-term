//! Per-workspace policy files with intersection semantics: a repo's
//! `.vambiant-term/policy.toml` can narrow the global policy, never widen
//! it (docs/05 §4.1, docs/09).

use std::path::Path;

use crate::rules::{Decide, Policy, PolicyError};

/// The effective policy for a workspace, with what was dropped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Effective {
    /// The intersection.
    pub policy: Policy,
    /// Repo settings that would have widened the global policy; ignored
    /// and reported, never applied.
    pub warnings: Vec<String>,
}

/// `global ∩ repo`.
pub fn narrow(global: &Policy, repo: &Policy) -> Effective {
    let mut out = global.clone();
    let mut warnings = Vec::new();
    narrow_safety(&mut out, repo, &mut warnings);
    narrow_autonomy(&mut out, repo, &mut warnings);
    narrow_egress(&mut out, repo, &mut warnings);
    Effective {
        policy: out,
        warnings,
    }
}

fn narrow_safety(out: &mut Policy, repo: &Policy, warnings: &mut Vec<String>) {
    let safety = &mut out.safety;
    let theirs_safety = &repo.safety;
    for (name, mine, theirs) in [
        (
            "destructive",
            &mut safety.destructive,
            theirs_safety.destructive,
        ),
        (
            "irreversible_remote",
            &mut safety.irreversible_remote,
            theirs_safety.irreversible_remote,
        ),
        (
            "credential_read",
            &mut safety.credential_read,
            theirs_safety.credential_read,
        ),
        (
            "network_egress",
            &mut safety.network_egress,
            theirs_safety.network_egress,
        ),
        ("privilege", &mut safety.privilege, theirs_safety.privilege),
        (
            "obfuscated",
            &mut safety.obfuscated,
            theirs_safety.obfuscated,
        ),
        (
            "unparseable",
            &mut safety.unparseable,
            theirs_safety.unparseable,
        ),
    ] {
        if theirs > *mine {
            *mine = theirs;
        } else if theirs < *mine {
            warnings.push(format!(
                "[safety] {name} = {theirs:?} is looser than the global policy; kept {mine:?}"
            ));
        }
    }
    for b in &theirs_safety.protected_branches {
        if !safety.protected_branches.contains(b) {
            safety.protected_branches.push(b.clone());
        }
    }
}

fn narrow_autonomy(out: &mut Policy, repo: &Policy, warnings: &mut Vec<String>) {
    let auto = &mut out.autonomy;
    auto.enabled = auto.enabled && repo.autonomy.enabled;
    auto.dry_run = auto.dry_run || repo.autonomy.dry_run;
    auto.watchdog = auto.watchdog && repo.autonomy.watchdog;
    auto.loop_detect = auto.loop_detect && repo.autonomy.loop_detect;
    auto.task_queue = auto.task_queue && repo.autonomy.task_queue;
    // Repo rules run first so a repo can say "ask" or "deny" before a
    // global "allow"; a repo "allow" would widen and is dropped.
    let mut rules = Vec::new();
    for rule in &repo.autonomy.rules {
        if rule.decide == Decide::Allow {
            warnings.push(format!(
                "rule `{}`: a repo policy cannot allow; dropped",
                rule.name
            ));
        } else {
            rules.push(rule.clone());
        }
    }
    rules.extend(auto.rules.iter().cloned());
    auto.rules = rules;
}

fn narrow_egress(out: &mut Policy, repo: &Policy, warnings: &mut Vec<String>) {
    let egress = &mut out.egress;
    if repo.egress.mode < egress.mode {
        egress.mode = repo.egress.mode;
    } else if repo.egress.mode > egress.mode {
        warnings.push(format!(
            "[egress] mode = {:?} is wider than the global {:?}; kept the global",
            repo.egress.mode, egress.mode
        ));
    }
    if !repo.egress.allow_providers.is_empty() {
        if egress.allow_providers.is_empty() {
            egress
                .allow_providers
                .clone_from(&repo.egress.allow_providers);
        } else {
            egress
                .allow_providers
                .retain(|p| repo.egress.allow_providers.contains(p));
        }
    }
    egress.max_context_bytes = match (egress.max_context_bytes, repo.egress.max_context_bytes) {
        (Some(mine), Some(theirs)) => Some(mine.min(theirs)),
        (mine, theirs) => mine.or(theirs),
    };
    for path in &repo.egress.never_include.paths {
        if !egress.never_include.paths.contains(path) {
            egress.never_include.paths.push(path.clone());
        }
    }
    for command in &repo.egress.never_include.commands {
        if !egress.never_include.commands.contains(command) {
            egress.never_include.commands.push(command.clone());
        }
    }
}

/// Reads a policy file; a missing file is the default policy.
pub fn load_file(path: &Path) -> Result<Policy, PolicyError> {
    match std::fs::read_to_string(path) {
        Ok(text) => Policy::parse(&text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Policy::default()),
        Err(e) => Err(PolicyError::Syntax(format!("{}: {e}", path.display()))),
    }
}

/// The effective policy for a worktree: the global file narrowed by the
/// repo's `.vambiant-term/policy.toml`, if there is one.
pub fn load(global: &Path, worktree: Option<&Path>) -> Result<Effective, PolicyError> {
    let g = load_file(global)?;
    match worktree {
        Some(w) => {
            let r = load_file(&w.join(".vambiant-term").join("policy.toml"))?;
            Ok(narrow(&g, &r))
        }
        None => Ok(Effective {
            policy: g,
            warnings: Vec::new(),
        }),
    }
}
