# ADR-0010 — SwiftPM + a bundle script, mise-managed toolchains, full Xcode

**Status:** Accepted · 2026-09-04

## Decision
- **No `.xcodeproj`, no XcodeGen, no Tuist.** `app/Package.swift` plus `Scripts/bundle.sh` assembles the `.app` (Info.plist, entitlements, resources, codesign) — the same pattern as `claude-sessions`.
- **Full Xcode is a prerequisite** (you're installing it). It gives us `xcrun metal` for precompiled `.metallib`, the macOS SDK, and a real signing/notarization path. Runtime shader compilation from source stays as a documented fallback so a CLT-only machine can still build a debug binary.
- **mise** owns toolchain versions: `mise.toml` pins rust, node (for any tooling), swiftlint, swiftformat, cbindgen. `mise run <task>` is the only entry point — `build`, `test`, `ci`, `bench`, `release`.
- **Rust ≥ 1.98.1** pinned in `rust-toolchain.toml`. 1.98.1 fixed a vtable miscompilation; do not pin below it. Edition 2024.
- `cargo deny` in CI for licences and advisories. `cbindgen` output checked in and diffed.

## Signing and entitlements
Ad-hoc signed for local development. The bundle id `com.vambiant.term` is **stable forever** — TCC grants (Automation, Notifications, Accessibility if ever needed) anchor to it.

The app is **unsandboxed by necessity** — it spawns arbitrary user processes. Stated plainly in the README rather than buried. Hardened runtime on; no `allow-unsigned-executable-memory`. Every entitlement gets a one-line justification in `Scripts/entitlements.plist` comments.

## CI
`fast` on every push, `full`/`perf`/`conformance` on every PR, `nightly` for fuzzing, sanitizers and real-agent round trips, `release` on a tag. All of it runs locally via `mise run ci` — **a CI you cannot run locally is a CI that gets ignored.**
