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

## M2 amendment — proposed 2026-09-05, awaiting decision

**Status of this section: Proposed.**

The recovery paragraph above says a restarted daemon "re-adopts by scanning its own session table + `/dev/ptmx` records". That is not implementable: a PTY master is a file descriptor, it dies with the process that holds it, and macOS offers no way to reopen an existing master from its slave path. M2 therefore ships the honest half of the rule — on start, every session that was live under the previous daemon is **marked `orphaned` in `state.db` and stays listed** (verified by `crates/vtermd/tests/end_to_end.rs`) — and none of the survive-a-daemon-crash promise.

Proposed mechanism to deliver that promise: a **per-session fd holder**. `vtermd` spawns each PTY through a tiny helper process (`vtermd-hold`) that owns the master fd, keeps a bounded ring of recent output while no daemon is attached, and listens on its own Unix socket under the runtime directory. The daemon receives the master fd over `SCM_RIGHTS` on first attach and again on re-adoption after a restart; the grid is rebuilt by replaying the ring, and the session is labelled *"re-adopted — grid rebuilt from buffer"* until the next full-screen redraw, per the never-lie-about-state rule. A helper that is gone (reboot, kill) is what `orphaned` then means.

Costs: one more process per session (a few hundred KB), one fd hop at attach only (not on the byte path — the daemon reads the master directly once it holds the fd), and a `launchd`-independent lifetime that must be reasoned about (helpers outlive the daemon by design and are reaped by `vterm prune`).

Alternative rejected: moving the terminal core into the helper (tmux-style server per session). It survives daemon crashes too, but puts every viewer hop and the whole adapter/inbox logic behind a second socket, which is the latency budget ADR-0004 already spends once.
