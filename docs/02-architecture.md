# 02 — Architecture

## 1. Shape of the system

Three processes. Two languages. One socket.

```
┌──────────────────────────────────────────────────────────────────┐
│  Vambiant Term.app        (Swift 6.3 · SwiftUI + AppKit)         │
│  ─────────────────────────────────────────────────────────────   │
│  Windows · tabs · splits · sidebar · inbox · palette · settings  │
│  MetalGridView (CAMetalLayer)   CoreText shaper + glyph atlas    │
│           │  C ABI (cbindgen header, main-thread marshalled)     │
│  ┌────────▼─────────────────────────────────────────────────┐    │
│  │ libvambiant_term.a    (Rust · staticlib)                 │    │
│  │  vt-core   VTE state, grid, damage      (libghostty-vt)  │    │
│  │  vt-pty    PTY spawn, resize, signals                    │    │
│  │  vt-blocks OSC 133/633 semantic segmentation             │    │
│  │  vt-ai     provider layer                                │    │
│  │  vt-redact streaming secret redaction                    │    │
│  │  vt-policy safety classifier + autonomy rules            │    │
│  │  vt-ipc    client for vtermd                             │    │
│  └──────────────────────────────────────────────────────────┘    │
└───────────────────────────────┬──────────────────────────────────┘
                                │ JSON-RPC 2.0 over Unix socket
                                │ $TMPDIR/vambiant-term-<uid>/vtermd.sock, 0700
┌───────────────────────────────▼──────────────────────────────────┐
│  vtermd                    (Rust · launchd LaunchAgent)          │
│  ────────────────────────────────────────────────────────────    │
│  Session registry · PTY ownership · agent adapters ·             │
│  approval queue · worktree registry · event log (SQLite) ·       │
│  hook receiver (HTTP on the same socket) ·                       │
│  loopback API (127.0.0.1, token auth) · notifications            │
└───────────────────────────────┬──────────────────────────────────┘
                                │ spawns + owns PTYs
                   ┌────────────┴────────────┐
                   ▼                         ▼
            claude (hooks →)          codex (app-server ↔)
```

`vterm` (the CLI) is a fourth, ephemeral process: a thin JSON-RPC client of `vtermd`. It shares `vt-ipc` with the app.

## 2. Why the daemon owns the PTYs

The single most important structural decision. If the GUI owns the PTY, closing the window kills the agent, and that is the failure this product exists to eliminate.

Consequence: **the app is a viewer.** It attaches to a session, receives a snapshot plus a damage stream, and sends input. It can crash, be force-quit, or be updated mid-flight and nothing running is lost. It also means `vterm` in Ghostty and the GUI are peers, not master and slave — which is what makes M2 (daemon-first) genuinely useful before M4 (the GUI) exists.

Cost: one extra hop on the hot path. Mitigated by (a) shared memory for the grid snapshot, (b) the daemon and app being on the same machine with a Unix socket, (c) batching damage at display cadence rather than per byte. Measured in M1 against a direct-PTY baseline; if the hop costs more than 1 ms p99 we move the *foreground* pane's PTY into the app and keep background sessions in the daemon (documented fallback, ADR-0004).

## 3. Crate layout

```
vambiant-term/
├── Cargo.toml                  # workspace, resolver = "3", edition 2024
├── crates/
│   ├── vt-core/                # terminal state: Term, Grid, damage, modes
│   ├── vt-pty/                 # spawn, winsize, SIGWINCH/SIGCHLD, drain-on-exit
│   ├── vt-blocks/              # OSC 133/633 → Block stream; heuristic fallback
│   ├── vt-proto/              # wire types shared by daemon, app, CLI (serde)
│   ├── vt-ipc/                 # JSON-RPC client + server plumbing
│   ├── vt-agent/               # AgentAdapter trait + claude/codex/generic impls
│   ├── vt-ai/                  # provider abstraction (Messages-shaped) + clients
│   ├── vt-redact/              # streaming secret detection & redaction
│   ├── vt-policy/              # shell parser, safety classes, autonomy rules
│   ├── vt-store/               # SQLite event log, session metadata, block index
│   ├── vt-worktree/            # git worktree registry and lifecycle
│   ├── vt-config/              # TOML schema, hot reload, theme import
│   ├── vt-ffi/                 # #[unsafe(no_mangle)] extern "C" surface + cbindgen
│   ├── vtermd/                 # the daemon binary
│   └── vterm/                  # the CLI binary
└── app/
    ├── Package.swift           # SwiftPM, no .xcodeproj
    ├── Sources/VambiantTerm/   # SwiftUI app, windows, panes, inbox, palette
    ├── Sources/TermRender/     # MetalGridView, glyph atlas, CoreText shaping
    ├── Sources/VTBridge/       # generated header + safe Swift wrappers
    └── Scripts/bundle.sh       # .app assembly, Info.plist, codesign
```

Rule: **nothing in `crates/` may depend on `app/`.** The Rust side is headless-testable in its entirety, which is what makes CI meaningful.

## 4. Threads and data flow

Per attached session, inside the daemon:

- **Reader thread** — blocking read from the PTY fd → feeds `vt-core`'s parser → mutates the grid → records damage. Never allocates per byte.
- **Writer thread** — input queue → PTY. Separate so a stalled write can't block reads.
- **Event thread** — hook receiver, adapter state machine, approval queue transitions.
- **Flush** — damage is coalesced and published at most once per display interval (8.3 ms at 120 Hz). Attached viewers get a delta; newly attached viewers get a full snapshot.

Inside the app:

- Rust callbacks arrive on a Rust thread and are marshalled to the main thread before touching AppKit. `AppKitWindowHandle` is `!Send`/`!Sync` for exactly this reason. The FFI contract (ADR-0002) states which callbacks are main-thread-only.
- The renderer reads a double-buffered cell array by pointer. No copy, no serialization on the hot path.

## 5. The FFI seam

Two tiers, deliberately (ADR-0002):

**Hot path — raw C ABI + `cbindgen`.** Grid snapshots as `*const VtCell` + length; key and mouse events as flat structs. Zero marshalling. Edition 2024 requires `#[unsafe(no_mangle)]`. Use `extern "C-unwind"` anywhere a Swift `fatalError` could unwind through a Rust frame.

**Cold path — `swift-bridge` 0.1.59.** Config, session lifecycle, provider calls, async operations. It has real bidirectional async (with the caveat that an async Swift fn returning `Result` needs typed `throws(E)`). It is a 0.1.x crate with intermittent maintenance — acceptable for the cold path, unacceptable for 120 Hz damage streaming.

> **As built (M4, 2026-09-05):** the cold path is `vt_daemon_call` — one JSON-RPC call per invocation, same wire contract as the `vterm` CLI. `swift-bridge` is deferred; see the ADR-0002 amendment. The hot path is `vt_viewer_*` in `crates/vt-ffi/src/viewer.rs`: an in-process viewer with two daemon connections (stream and input, the same split `vterm attach` uses) and a `#[repr(C)]` cell grid.

Explicitly rejected: **UniFFI** (serializes across the boundary, MPL-2.0, known Swift 6 async/`Sendable` gaps) and **cxx** (Rust→C++→Swift is two bridges and inherits every Swift C++ interop limitation).

New in Swift 6.3: the `@c` attribute exposes Swift functions as clean C symbols, which removes the `@_cdecl` hackery in the Swift→Rust callback direction. Use it.

## 6. Rendering pipeline

1. `vt-core` produces a damage set: dirty line ranges plus cursor movement.
2. The daemon coalesces and publishes cells for those lines into shared memory.
3. Swift's `MetalGridView` receives a "dirty" signal on the main thread and reads the buffer.
4. Cell runs are segmented by grapheme cluster, then style, then font — a run breaks whenever a different font is required for a codepoint (Ghostty's rule).
5. CoreText shapes each run; glyphs are rasterized into an atlas on first use.
6. Metal draws background quads, then glyph quads, then decorations (underline, strikethrough, cursor, selection).
7. Present via `CADisplayLink`, adaptive to ProMotion; idle when nothing is dirty.

Fonts, shaping and glyphs never cross the FFI boundary. Rust sends cell content and attributes; Swift owns everything visual. This is the same split Ghostty uses on macOS, and it is why we do not need `wgpu`, `winit`, `cosmic-text` or `swash` at all.

## 7. Storage

Runtime (not persisted): the daemon socket is `$TMPDIR/vambiant-term-<uid>/vtermd.sock` — macOS gives every user a private `$TMPDIR` under `/var/folders`, the closest thing to `$XDG_RUNTIME_DIR` the platform has. `VAMBIANT_TERM_RUNTIME` overrides the directory (tests, multiple daemons). The directory is forced to `0700` and the socket to `0600` on bind, and `getpeereid` rejects any peer whose uid is not the daemon's (`vt-ipc::auth`, decided 2026-09-05). Framing is one JSON object per line. Each connection has its own writer thread behind a bounded outbox (512 messages): publishers never block on a slow viewer, and a viewer that falls that far behind is disconnected and must re-attach for a fresh snapshot — an explicit gap, never a silently dropped delta. Output deltas go only to connections that attached to that session; lifecycle notifications go to everyone.

`~/.local/state/vambiant-term/`

| File | Contents |
|---|---|
| `state.db` (SQLite, WAL) | sessions, blocks, agent events, approval decisions, worktree registry, egress log |
| `scrollback/<session>.bin` | compressed scrollback rings, memory-mapped |
| `logs/vtermd.log` | rotating daemon log |

`~/.config/vambiant-term/`

| File | Contents |
|---|---|
| `config.toml` | main config (see `09-config-reference.md`) |
| `themes/` | imported/custom themes |
| `policy.toml` | global safety + autonomy policy |
| `providers.toml` | provider profiles (**no secrets** — keys live in Keychain) |

Per-repo: `.vambiant-term/policy.toml` (committable, narrows but never widens the global policy) and `.vambiant-term/context.md`.

**Retention is a first-class setting.** Blocks and agent events default to 90 days; the egress log to 365; scrollback to a size cap. `vterm prune` and an automatic sweep on daemon start.

## 8. Failure posture

| Failure | Behaviour |
|---|---|
| Daemon dies | App shows a banner; every session's PTY lives on in its `vtermd-hold` process, which keeps buffering output. launchd `KeepAlive` restarts the daemon, which re-adopts each session from its holder (fd over `SCM_RIGHTS`, grid rebuilt from the holder's ring buffer, session labelled `readopted`). A holder that recorded an exit closes the session with that code; a holder that cannot be reached leaves the session `orphaned`, never silently dropped (ADR-0004 amendment) |
| App dies | Nothing is lost. Reopen and reattach |
| A provider is down | Circuit-breaker opens, feature degrades to history-only suggestions, banner in the pane, never a modal |
| Hook installation blocked by enterprise policy | Adapter falls back to PTY heuristics, UI labels the session "limited observability" |
| Redaction pipeline errors | **Fail closed** — the request is not sent. Never fail open on a redaction bug |
| An agent's transcript schema changes under us | We do not depend on it (§B1.10 in `01`); backfill degrades, live observability does not |
| Shell integration corrupted by a prompt framework | Detected via malformed mark sequences; blocks fall back to heuristic segmentation with a one-time warning |

## 9. Dependency policy

- Pin exact versions in `Cargo.lock`; `cargo deny` in CI for licences and advisories.
- The terminal core is `libghostty-vt` (MIT/Apache-2.0, pre-1.0 C API with expected breaking changes) behind our own `TerminalCore` trait; `alacritty_terminal` (Apache-2.0 only, no stability guarantee) is the documented fallback behind the same trait (ADR-0001 amendment, 2026-09-05). The Ghostty source is vendored as a submodule and built by zig 0.15.2 under mise.
- Prefer zero-dependency solutions on the hot path. `vt-core`, `vt-pty` and `vt-redact` should have a dependency tree you can read in one screen.
- No crate enters the tree without a note in `docs/10-research-notes.md` recording version, licence, maintenance status.

## 10. Toolchain

| Tool | Version | Notes |
|---|---|---|
| Rust | ≥ **1.98.1** stable, edition 2024 | 1.98.1 fixed a vtable miscompilation; do not pin below it |
| Swift | ≥ **6.3** | `@c` attribute. Builds under the Command Line Tools alone: Metal shaders are compiled at runtime (`makeLibrary(source:)`) because the CLT ship no `metal` compiler; signing is ad hoc until M9 |
| macOS SDK | 26.x | |
| mise | manages rust/node/swift-tool versions — `mise.toml` is the source of truth | |
| cbindgen | 0.29.x | generates `vt_ffi.h` in a build step, checked in and diffed in CI |
| swift-bridge | 0.1.59 | cold path only |
| SwiftLint / SwiftFormat | latest | already installed |
| gh | latest | releases, PRs |
