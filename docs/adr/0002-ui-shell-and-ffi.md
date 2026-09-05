# ADR-0002 — Swift shell over a Rust core, two-tier FFI

**Status:** Accepted · 2026-09-04 · Amended 2026-09-05

## Context
A Warp-class UI (sidebar, approval inbox, diff blocks, palette) needs a real UI toolkit. The core must be headless-testable Rust. Something has to bridge them at 120 Hz.

## Options
- Pure Rust + winit/wgpu with hand-built widgets — one language, but we'd be writing a UI toolkit.
- Pure Rust + egui/iced — rich panels cheap, never feels macOS-native (menus, text fields, accessibility are approximations).
- Rust core + webview (Tauri) — fastest to polish, costs memory, a JS toolchain, and input-latency care on the hot path.
- **Rust core + SwiftUI/AppKit** — native everything, one FFI seam.

## Decision
SwiftUI/AppKit shell over a Rust staticlib. **Two FFI tiers:**

- **Hot path — plain C ABI + `cbindgen` 0.29.4.** Grid snapshots as `*const VtCell` + length; flat key/mouse structs. Zero marshalling — Swift's renderer reads the cell array directly. `swift-bridge`'s `RustVec<T>` and UniFFI both copy; at 120 Hz that copy is the budget.
- **Cold path — `swift-bridge` 0.1.59.** Config, session lifecycle, async provider calls. Real bidirectional async (async Swift fns returning `Result` need typed `throws(E)`).

## Rejected
- **UniFFI 0.32** — serializes across the boundary, MPL-2.0, Swift 6 support explicitly partial ("async code will not conform" to `Sendable`). Built for cross-language breadth, wrong for damage streaming.
- **cxx** — Rust→C++→Swift is two bridges and inherits every Swift C++-interop limitation (no C++→Swift generics, actors, async, or throwing functions).

## Consequences
- Edition 2024 requires `#[unsafe(no_mangle)]`. Use `extern "C-unwind"` anywhere Swift could unwind through Rust.
- Swift 6.3's `@c` attribute replaces `@_cdecl` hacks for Swift→Rust callbacks. Use it.
- `AppKitWindowHandle` is `!Send`/`!Sync`: PTY and parser on their own threads, double-buffered damage region, only a dirty signal marshalled to main.
- `cbindgen` output is checked in and **diffed in CI** — an ABI drift is a memory-safety bug, not a compile error.
- `swift-bridge` is 0.1.x with intermittent maintenance (15-month release gap, ~92 open issues). Acceptable for the cold path only; the C ABI cannot rot.

## Amendment 2026-09-05 — the cold path is a JSON passthrough for now

**What changed.** M4 ships the cold tier as one C function, `vt_daemon_call(socket, method, params_json) → json`, not as a `swift-bridge` surface. Swift sends the same JSON-RPC the `vterm` CLI sends and decodes the reply with `Codable`. `swift-bridge` is not in the tree.

**Why.** By M4 every cold-path operation (session lifecycle, inbox, events, config) already exists as a `vtermd` JSON-RPC method with a Rust-side contract test, because the CLI needed it in M2/M3. A second typed surface would duplicate that contract in generated code from a 0.1.x crate with intermittent maintenance, for no latency benefit — the cold path is not latency-sensitive by definition. The passthrough has no build-time code generation, adds nothing to the ABI header beyond three functions, and cannot rot.

**What stays.** The hot path is unchanged: `vt_viewer_attach` runs a reader thread that applies daemon output deltas into a `#[repr(C)]` cell array; Swift reads it by pointer between `vt_viewer_acquire` and `vt_viewer_release` and sends keys as a flat `VtKeyEvent`. The dirty callback fires on the viewer's thread and the shell marshals it to main.

**When to revisit.** If a cold-path call needs streaming or cancellation from Swift (provider calls in M-AI are the likely first case), decide then between `swift-bridge` async and a second notification-carrying connection through the same passthrough. Record it here.
