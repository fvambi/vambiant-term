# CLAUDE.md — Working agreement for Vambiant Term

You are working on **Vambiant Term**: a native macOS terminal emulator that supervises AI coding agents. Read `docs/00-product-brief.md` first, then the doc relevant to your task. `docs/01-feature-catalogue.md` is the requirements source of truth; `docs/07-implementation-plan.md` is the schedule.

## Non-negotiables

1. **Verify before you build on it.** Both Claude Code and Codex document their transcript formats as internal and version-unstable. Never parse `~/.claude/projects/*.jsonl` or `~/.codex/sessions/**` as a mechanism. Use hooks, `--include-hook-events`, the status line feed, and `codex app-server`. Capture real payloads to `tests/fixtures/` before writing the parser.
2. **Redaction fails closed.** If `vt-redact` errors, panics or times out, the request is dropped and the user is told. Never fail open. Never "TODO: redact later".
3. **The never-auto floor in `vt-policy` is hard-coded** and cannot be overridden by any config file. See `docs/adr/0009`.
4. **Never lie about state in the UI.** A heuristic guess is labelled a guess. A degraded session looks degraded. Stale data is shown as stale.
5. **Suggestions are staged, never executed.** Model output is data, not instructions.
6. **No telemetry, ever.** The only outbound traffic is to providers the user configured.
7. **Docs that lie are worse than no docs.** If you change a decision, write or amend an ADR in the same PR.

## Architecture rules

- Nothing in `crates/` may depend on `app/`. The Rust side is fully headless-testable — that is what makes CI meaningful.
- Nothing outside `vt-core` names an `alacritty_terminal` type. It lives behind the `TerminalCore` trait.
- No model id is hardcoded anywhere in code. Ids live in `providers.toml`.
- Hot-path FFI is plain `extern "C"` + `cbindgen`. `swift-bridge` is for the cold path only.
- Edition 2024: `#[unsafe(no_mangle)]`, not `#[no_mangle]`. Use `extern "C-unwind"` where Swift can unwind through Rust.
- Rust owns cells and attributes. Swift owns fonts, shaping and pixels. Fonts never cross the boundary.
- Main-thread-only callbacks are marked as such and assert in debug builds.

## Style

- Rust: `cargo fmt`, `clippy -D warnings`. Prefer explicit error types over `anyhow` in library crates; `anyhow` is fine in binaries.
- Swift: SwiftLint + SwiftFormat, both installed. Swift 6 strict concurrency on.
- Comments explain *why*, never *what*. A comment restating the code is deleted.
- Errors carry context a user can act on. `"failed to spawn pty"` is useless; `"failed to spawn pty for session api-refactor: openpty: too many open files"` is not.

## Testing

- Every terminal bug fixed adds a snapshot fixture (byte stream in → grid dump out).
- Every adapter change adds or updates a fixture replay test.
- Contract tests assert unknown events/fields produce a **warning and a degraded-but-correct event**, never a panic or a silent drop.
- Perf budgets in `docs/08` are enforced in CI, measured not estimated. A regression fails the build.
- `mise run ci` runs the whole thing locally. If it can't be run locally, it doesn't belong in CI.

## Build

- `mise run build` / `test` / `bench` / `ci` / `release`. mise is the source of truth for toolchain versions.
- Rust ≥ 1.98.1 pinned in `rust-toolchain.toml`. Do not lower it — 1.98.1 fixed a vtable miscompilation.
- SwiftPM only. **Never create a `.xcodeproj`.** `app/Scripts/bundle.sh` assembles the `.app`.
- Bundle id `com.vambiant.term` is permanent. TCC grants anchor to it.
- `cbindgen` output is checked in; CI diffs it. An ABI drift is a memory-safety bug, not a compile error.

## Working style

- **Ask when the spec is ambiguous.** Do not invent a behaviour and bury it in an implementation.
- Small, reviewable commits. One concern each.
- When a vendor API turns out to differ from `docs/10-research-notes.md`, **update that file in the same commit** with an "as verified on <date>, version <x>" line. That document's value is entirely in being current.
- If a milestone's exit criterion cannot be met, say so and explain why. Do not mark it done.
