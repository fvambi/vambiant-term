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

## M2 amendment — accepted 2026-09-05 (implemented; Florian: "move on")

**Status of this section: Accepted and implemented** in `crates/vtermd/src/bin/hold.rs` (`vtermd-hold`) and `crates/vtermd/src/holder.rs`; verified by `crates/vtermd/tests/end_to_end.rs::sessions_survive_a_daemon_restart`.

The recovery paragraph above says a restarted daemon "re-adopts by scanning its own session table + `/dev/ptmx` records". That is not implementable: a PTY master is a file descriptor, it dies with the process that holds it, and macOS offers no way to reopen an existing master from its slave path. M2 therefore ships the honest half of the rule — on start, every session that was live under the previous daemon is **marked `orphaned` in `state.db` and stays listed** (verified by `crates/vtermd/tests/end_to_end.rs`) — and none of the survive-a-daemon-crash promise.

Mechanism as built: a **per-session fd holder**. `vtermd` spawns each PTY through `vtermd-hold`, a detached (`setsid`) helper that owns the master fd, is the child's parent, and is the *only reader* of the master: it keeps a rolling 1 MiB ring of output at all times and relays live output to the attached daemon as length-prefixed frames over its Unix socket (`<runtime>/hold-<id>.sock`). The daemon receives the master fd over `SCM_RIGHTS` on attach and re-adoption and uses it directly for input, resize and signals, so the extra hop is on the output path only. On re-adoption the ring is replayed into a fresh core (query responses from the replay are discarded) and the session carries `readopted = true` so the UI can say *"re-adopted — grid rebuilt from buffer"*. A holder that recorded the child's exit while no daemon was attached leaves `<socket>.exit`, and the next daemon closes the session with that code; a holder that cannot be reached (reboot, killed) is what `orphaned` means — listed, never dropped. An earlier variant that buffered only while unattached was rejected because the dead daemon had already consumed everything the holder had not seen.

Costs: one more process per session (a few hundred KB), one extra copy on the output path (holder → daemon over a Unix socket; input stays direct), and a `launchd`-independent lifetime (helpers outlive the daemon by design; the daemon reaps exited holders while it lives, and `vterm prune` will sweep leftovers).

Alternative rejected: moving the terminal core into the helper (tmux-style server per session). It survives daemon crashes too, but puts every viewer hop and the whole adapter/inbox logic behind a second socket, which is the latency budget ADR-0004 already spends once.
