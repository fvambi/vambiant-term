# ADR-0004 — The daemon owns every PTY

**Status:** Accepted · 2026-09-04

## Context
If the GUI owns the PTY, closing a window kills the agent. That is precisely the failure this product exists to remove.

## Decision
`vtermd` (launchd LaunchAgent, `KeepAlive`) spawns and owns all PTYs. The app and the `vterm` CLI are peer *viewers* that attach over JSON-RPC 2.0 on a Unix socket (mode 0700, `LOCAL_PEERCRED` check), plus a token-authenticated loopback HTTP/WebSocket API on 127.0.0.1.

## Consequences
- The GUI can crash, be force-quit or be updated mid-flight and nothing running is lost.
- `vterm` inside Ghostty is a first-class client — M2 is useful months before M4 exists.
- Other tools (a menu bar app, Raycast, a phone over Tailscale) can list sessions and answer approvals through the loopback API.
- One extra hop on the hot path. Mitigated by shared memory for grid snapshots, a local socket, and damage batched at display cadence.

## Fallback, pre-agreed
If M4 measures the hop at **> 1 ms p99**, move the *foreground* pane's PTY in-process and keep background sessions in the daemon. This is written down now so the decision isn't relitigated under deadline pressure later.

## Recovery
On restart the daemon re-adopts from its own session table. Anything unadoptable is marked `orphaned` and shown — **never silently dropped.**
