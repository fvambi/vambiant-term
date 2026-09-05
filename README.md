# Vambiant Term

A native macOS terminal emulator whose first-class citizen is an **AI agent session**, not a shell session.

Claude Code, Codex and their peers are long-lived, block on questions, emit structured events as unstructured text, and collide when run in parallel. Every terminal shipped today treats them as anonymous byte streams. Vambiant Term supervises them: named persistent sessions, one global approval inbox, structured diffs and tool calls, and git-worktree orchestration for running several agents at once.

It is also a fast, correct, GPU-rendered terminal. In a plain shell pane it should be indistinguishable from Ghostty.

## Status

**M4 built, M5 next.** M0 verified the spec against the real binaries; M1–M3 delivered the terminal core (`libghostty-vt` behind a `TerminalCore` trait), the `vtermd` daemon with crash-surviving sessions, the `vterm` CLI, and the Claude Code / Codex adapters with the approval inbox (exit criteria measured, see `docs/07-implementation-plan.md`). M4 adds the native macOS shell: a Metal-rendered grid attached to the daemon, tabs and splits, both keymap profiles, and a Settings window generated from the config schema. Its latency exit criterion is **not** met and `docs/07` says why with numbers. `docs/10-research-notes.md` has every vendor finding with an *as verified on* line.

Build and run: `mise run app:bundle` assembles `app/build/Vambiant Term.app` (Command Line Tools suffice; no Xcode, no `.xcodeproj`); `mise run ci` runs everything CI runs.

## Design

| Doc | Contents |
|---|---|
| [`docs/00-product-brief.md`](docs/00-product-brief.md) | What and why, confirmed decisions |
| [`docs/01-feature-catalogue.md`](docs/01-feature-catalogue.md) | Every feature considered, tiered Must/Should/Could/Won't |
| [`docs/02-architecture.md`](docs/02-architecture.md) | Processes, crates, threads, the FFI seam |
| [`docs/03-agent-integration.md`](docs/03-agent-integration.md) | Exactly how Claude Code and Codex are observed and controlled |
| [`docs/04-ai-provider-layer.md`](docs/04-ai-provider-layer.md) | Provider abstraction, routing, latency budgets |
| [`docs/05-security-privacy.md`](docs/05-security-privacy.md) | Threat model, redaction pipeline, safety classifier |
| [`docs/06-ux-interaction.md`](docs/06-ux-interaction.md) | Windows, inbox, blocks, keymap |
| [`docs/07-implementation-plan.md`](docs/07-implementation-plan.md) | M0–M9, exit criteria, risk register |
| [`docs/08-test-benchmark-plan.md`](docs/08-test-benchmark-plan.md) | Conformance, property tests, perf budgets |
| [`docs/09-config-reference.md`](docs/09-config-reference.md) | Complete TOML schema |
| [`docs/10-research-notes.md`](docs/10-research-notes.md) | Every verified fact with a source, and everything still unverified |
| [`docs/adr/`](docs/adr/) | Ten decision records with the rejected alternatives |

## Stack

Rust core (`libghostty-vt` behind a trait, PTY, providers, redaction, policy, daemon) + SwiftUI/AppKit shell with a Metal renderer and CoreText shaping. Local Unix-socket daemon owns every PTY so closing a window never kills an agent.

Requires: Rust ≥ 1.98.1, zig 0.15.2 (for the vendored Ghostty core), Swift 6.3+, full Xcode (from M4), mise. macOS only. Clone with `--recurse-submodules`; `mise install` provides every toolchain.

## Security posture

The app is **unsandboxed by necessity** — it spawns arbitrary user processes. API keys live in the Keychain, never in config. Every payload sent to a cloud provider passes a redaction pipeline that **fails closed**. There is no telemetry and no update check by default. See `docs/05-security-privacy.md`.

## Kickoff

`PROMPT.md` is a paste-ready prompt for starting implementation with Claude Code.
