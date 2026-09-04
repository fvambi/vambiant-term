# ADR-0009 — Build the whole autonomy engine; ship every rule disabled

**Status:** Accepted · 2026-09-04

## Context
You selected auto-approval, watchdog and background queue — **and** "keep it strictly manual". Those are incompatible as defaults but not as a design.

## Decision
Build the full policy engine as architecture. Ship with `autonomy.enabled = false` and `dry_run = true`. Autonomy is opt-in per workspace, per rule.

## The never-auto floor
Hard-coded in `vt-policy`, not configurable by any file, applied after every rule:
- `destructive` targeting a path outside the session's worktree
- anything `credential_read`
- anything `obfuscated` (`curl | sh`, `eval "$X"`, base64-decode-pipe-to-shell)
- force-push to a protected branch
- **any command from a generic-adapter session** — heuristic observability is not a basis for autonomous consent
- any command whose shell parse failed

A policy file attempting to allow one of these is a validation error at load, reported, not ignored.

## Parse, don't grep
Classification uses a real POSIX shell grammar over the whole command tree, not substring matching. `ls && rm -rf /` is destructive. **Unparseable input is unsafe by definition.** VS Code's docs admit their equivalent is "best effort" and evadable by quote concatenation — we do not repeat that.

## Trust is earned with evidence
`dry_run` logs every decision the engine *would* have made without making it. Run it for a week, read `vterm audit`, then enable. Every autonomous action logs the rule that caused it and carries an undo handle where the action is reversible.
