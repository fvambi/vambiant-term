//! git worktree registry: create/adopt/destroy, ownership guard so two
//! agents never share a tree, stale sweep, cooperation with Claude Code's
//! `WorktreeCreate`/`WorktreeRemove` hooks.

pub mod guard;
pub mod lifecycle;
pub mod registry;
