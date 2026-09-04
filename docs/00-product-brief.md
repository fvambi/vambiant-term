# 00 — Product Brief: Vambiant Term

> Status: draft v1 · Owner: Florian Wartner (Vambiant) · Date: 2026-09-04

## 1. One sentence

**Vambiant Term** is a native macOS terminal emulator whose first-class citizen is not a shell session but an **AI agent session** — it supervises Claude Code, Codex and friends the way a window manager supervises windows: named, persistent, observable, interruptible, and answerable from one place.

## 2. The problem, stated precisely

Every terminal shipped today assumes the thing on the other end of the PTY is a human-driven program. Agents break that assumption in four specific ways:

1. **They block on questions.** A blocked agent is invisible. With six panes open across four repos you find the blocked one by tabbing through them. The terminal knows nothing about "waiting for input" as a state.
2. **They emit structured events as unstructured text.** A file diff, a tool call, a token count and a permission request all arrive as the same undifferentiated byte stream. The terminal renders them identically because it has no idea they are different things.
3. **They are long-lived and detachable in principle, but not in practice.** Close the window, lose the session. `claude --resume <uuid>` requires you to know the uuid. tmux solves detach but knows nothing about agents.
4. **They run in parallel and collide.** Two agents in one working tree fight. Worktrees fix that but the bookkeeping (create, name, track, compare, merge, destroy) is manual.

Prior art confirms the gap. Warp's Universal Agent Support is the closest product and it is proprietary, cloud-coupled, and undocumented in mechanism. Zed's ACP is the only open standard but it is editor-hosted, not terminal-hosted. Ghostty explicitly declines to add agent features — and Mitchell Hashimoto left to build **Superlogical**, a durable multiplexer for human+agent workflows, which is the clearest possible signal that the substrate is missing (see `10-research-notes.md` §6).

## 3. What we are building

A single macOS application with three layers that are useful independently:

| Layer | Ships as | Useful alone? |
|---|---|---|
| **Terminal** | `Vambiant Term.app` — GPU-rendered, VT-correct, tabs/splits/panes | Yes — a fast terminal |
| **Supervisor** | `vtermd` daemon + `vterm` CLI | Yes — works inside Ghostty today |
| **Intelligence** | provider layer, inline suggest, ⌘K, failure explain, safety classifier | Yes — attaches to the other two |

The supervisor is the differentiator. The terminal is the vehicle. The intelligence is the polish.

## 4. Non-goals (v1)

- **Not cross-platform.** macOS only, native-first. Portability is preserved in the Rust core but never at the cost of a macOS-native affordance.
- **Not a cloud product.** No account, no sync service, no telemetry egress. The only network traffic is to model providers you configure.
- **Not an IDE.** No editor, no LSP, no file tree. Diffs are rendered for review, not edited.
- **Not a Warp clone.** No blocks-as-a-social-feature, no shared workflows, no cloud agents.
- **Not an agent.** Vambiant Term supervises agents; it does not implement one. The models it calls answer questions about *your terminal*, they do not autonomously edit your repo.

## 5. Success criteria

Vambiant Term v1 is done when, for one week, Florian:

1. Has not opened Ghostty.
2. Has not lost an agent session to a closed window.
3. Has never hunted through panes to find which agent is blocked.
4. Has run at least three agents in parallel on one repo without a worktree collision.
5. Reports input latency as indistinguishable from Ghostty (target: p99 keystroke→glyph under one display frame at 120 Hz, i.e. **< 8.3 ms**).
6. Has had zero credentials leave the machine unredacted.

## 6. Confirmed decisions

Locked with Florian on 2026-09-04:

| Axis | Decision |
|---|---|
| Terminal core | Rust, `alacritty_terminal` (see ADR-0001 — `libghostty-vt` is a live alternative behind a trait) |
| Platform | macOS only, native-first |
| UI shell | SwiftUI/AppKit app over a Rust core via a C ABI (ADR-0002) |
| Providers | Local (Ollama/llama.cpp/MLX) + Anthropic + OpenAI + any OpenAI-compatible endpoint |
| Agent features | Session manager, approval inbox, structured blocks, worktree orchestration — all four |
| Agent hooks | Claude Code hooks + transcripts, OSC 133 shell integration, supervisor process, PTY heuristics — all four, layered |
| AI UX | Inline ghost text, ⌘K NL→command, failure explain, command safety classifier |
| Data policy | Cloud-first for quality, **redaction always on**, per-workspace override |
| Daemon reach | Local Unix socket + token-authenticated loopback HTTP/WebSocket API |
| Config | TOML, hot-reloaded; import existing themes; tmux-style multiplexer keybinds |
| Autonomy | Full policy engine built, **every autonomous behaviour ships defaulted off** (see §7) |
| Scope | Full spec, no compromise |
| Build | Rust stable ≥ 1.98.1, Swift 6.3+, full Xcode, mise-managed toolchains, SwiftPM (no .xcodeproj) |

## 7. The one contradiction, resolved

You selected all three autonomy features *and* "keep it strictly manual". These are not compatible as defaults, so the resolution written into this spec is:

> **Build the whole autonomy engine. Ship every rule disabled. Autonomy is opt-in per workspace, per rule, with a visible audit log of every decision the terminal made on your behalf and a one-key undo.**

The engine is the hard part and it must exist from the architecture up; the defaults are a one-line config change you can make when you trust it. Manual is the default posture, not a permanent constraint.

## 8. Naming

- Product: **Vambiant Term**
- Bundle id: `com.vambiant.term` (stable — TCC grants anchor to it, never change)
- App CLI: `vterm`
- Daemon: `vtermd`
- Config dir: `~/.config/vambiant-term/`
- State dir: `~/.local/state/vambiant-term/`

Note: Emacs' `vterm` is a package, not a `$PATH` binary — there is no real collision. `vt` is left free as a user-defined alias.

## 9. Document map

| Doc | What it settles |
|---|---|
| `01-feature-catalogue.md` | Every feature considered, tiered Must/Should/Could/Won't |
| `02-architecture.md` | Processes, crates, threads, data flow, the FFI seam |
| `03-agent-integration.md` | Exactly how we observe and control Claude Code and Codex |
| `04-ai-provider-layer.md` | Provider abstraction, routing, latency budgets |
| `05-security-privacy.md` | Threat model, redaction pipeline, sandboxing, secrets |
| `06-ux-interaction.md` | Windows, panes, palette, inbox, keymap |
| `07-implementation-plan.md` | M0–M9, exit criteria, risks |
| `08-test-benchmark-plan.md` | Conformance, snapshot, fuzz, perf budgets |
| `09-config-reference.md` | The complete TOML schema |
| `10-research-notes.md` | Every verified fact with a URL, and everything still unverified |
| `adr/*` | Ten decision records with the alternatives we rejected |
