# ADR-0002 — Swift shell over a Rust core, two-tier FFI

**Status:** Accepted · 2026-09-04

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
