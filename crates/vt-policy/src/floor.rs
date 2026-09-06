//! The never-auto floor (ADR-0009, docs/05 §5.3). **Hard-coded. Not
//! configurable.** Applied after every rule: a verdict that hits it is
//! never auto-approved by any policy, in any workspace. Tests feed a
//! deliberately hostile `policy.toml` and assert it cannot move this.

use serde::{Deserialize, Serialize};

use crate::classify::{SafetyClass, Verdict};

/// Why a command can never be auto-approved.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "reason")]
pub enum FloorReason {
    /// A destructive command targets a path outside the session's worktree
    /// (or a target that cannot be resolved).
    DestructiveOutsideWorktree {
        /// The target.
        token: String,
    },
    /// Anything that reads credentials.
    CredentialRead {
        /// The token that read them.
        token: String,
    },
    /// Anything obfuscated.
    Obfuscated {
        /// The token.
        token: String,
    },
    /// A force-push to a protected (or unknown) branch.
    ForcePushProtected {
        /// The branch, or what could be read of it.
        token: String,
    },
    /// The session is observed by the generic adapter: heuristic
    /// observability is not a basis for autonomous consent.
    GenericAdapter,
    /// The shell reader could not follow the command line.
    ParseFailed {
        /// The reader's reason.
        detail: String,
    },
}

impl std::fmt::Display for FloorReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DestructiveOutsideWorktree { token } => {
                write!(
                    f,
                    "destructive command targets `{token}` outside the worktree"
                )
            }
            Self::CredentialRead { token } => write!(f, "`{token}` reads credentials"),
            Self::Obfuscated { token } => write!(f, "`{token}` is obfuscated"),
            Self::ForcePushProtected { token } => {
                write!(f, "force-push to protected branch `{token}`")
            }
            Self::GenericAdapter => {
                f.write_str("session is observed heuristically (generic adapter)")
            }
            Self::ParseFailed { detail } => write!(f, "command line could not be parsed: {detail}"),
        }
    }
}

/// The floor for a verdict. `generic_adapter` is the session's adapter
/// kind; it alone puts every command on the floor.
pub fn check(verdict: &Verdict, generic_adapter: bool) -> Option<FloorReason> {
    if generic_adapter {
        return Some(FloorReason::GenericAdapter);
    }
    if let Some(detail) = &verdict.parse_error {
        return Some(FloorReason::ParseFailed {
            detail: detail.clone(),
        });
    }
    // Worst first, so the reason shown is the strongest one.
    for f in &verdict.findings {
        match f.class {
            SafetyClass::Unparseable => {
                return Some(FloorReason::ParseFailed {
                    detail: f.detail.clone(),
                });
            }
            SafetyClass::Obfuscated => {
                return Some(FloorReason::Obfuscated {
                    token: f.token.clone(),
                });
            }
            SafetyClass::CredentialRead => {
                return Some(FloorReason::CredentialRead {
                    token: f.token.clone(),
                });
            }
            SafetyClass::Destructive if f.outside_worktree => {
                return Some(FloorReason::DestructiveOutsideWorktree {
                    token: f.token.clone(),
                });
            }
            SafetyClass::IrreversibleRemote if f.protected_branch => {
                return Some(FloorReason::ForcePushProtected {
                    token: f.token.clone(),
                });
            }
            _ => {}
        }
    }
    None
}

/// Classes a `[safety]` table may never set to `allow`: they are on the
/// floor whatever the file says. Reported as a load error (docs/09).
pub const NEVER_ALLOW: &[SafetyClass] = &[
    SafetyClass::CredentialRead,
    SafetyClass::Obfuscated,
    SafetyClass::Unparseable,
];
