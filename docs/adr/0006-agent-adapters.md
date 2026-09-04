# ADR-0006 — Agents are adapters over supported surfaces; never over transcript files

**Status:** Accepted · 2026-09-04

## Context
Claude Code and Codex both persist rich session transcripts on disk, and both document those formats as internal and version-unstable. Claude Code's docs, verbatim: *"The entry format is internal to Claude Code and changes between versions, so scripts that parse these files directly can break on any release."*

## Decision
Live observability comes only from supported surfaces: **hooks** (both vendors now have convergent hook systems), **structured stream events** (`--include-hook-events`, `codex exec --json`), the **Claude Code status line** feed, and **`codex app-server`** JSON-RPC. Our own SQLite event log is the durable record. Transcript files may be read *opportunistically* for backfill only; that path is allowed to break.

## Decision, part 2
One `AgentAdapter` trait with an explicit `Capabilities` struct, and the UI **shows** capability level. A generic-adapter session is visibly labelled "heuristic mode — observation only". We never let a fragile guess look like a real integration.

## Notable specifics
- `PermissionRequest` is the inbox's source event, not `Notification`.
- `canUseTool` fires **only** when the flow falls through to a prompt — never for auto-approved calls. To see every tool call, use `PreToolUse`. We do both.
- Permission prompts **never time out**, so every deferred decision gets a watchdog and a visible "still waiting" state. A `defer`/`null` without a follow-up hangs the agent forever.
- `system/init` carries a `capabilities` string array — use it for feature detection instead of version comparison.
- Prefer `modelUsage` over `usage` for cost accounting; `total_cost_usd` is a client-side list-price estimate and is labelled as such in the UI.
- Always resume by explicit id. `--continue`'s skip rules are asymmetric between interactive, `-p`, background and `/loop` sessions.
- Codex: use `app-server`, not the deprecated `codex mcp-server`. `turn/steer` has no Claude equivalent and deserves UI.

## Consequences
- Contract tests assert that unknown events and unknown fields produce a warning and a degraded-but-correct event, never a panic or a silent drop. A vendor schema change becomes a warning, not an outage.
- **ACP** (`terminal/create`, `terminal/output`, `terminal/wait_for_exit`) is nearly a specification of our daemon. Shape the internal session API so speaking ACP is an adapter, not a refactor — that is the v2 unlock.
