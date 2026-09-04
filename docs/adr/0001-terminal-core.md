# ADR-0001 — Terminal core: `alacritty_terminal` behind a `TerminalCore` trait

**Status:** Accepted (revisit at M0) · 2026-09-04

## Context
We need a VT state machine: parser, grid, scrollback, modes, damage. Writing one from scratch is a year of work before a daily driver. Three candidates exist.

## Options
1. **`alacritty_terminal` 0.26.0** — on crates.io, complete `Term` + `Grid` + PTY + event loop in one crate, small dependency tree, 6-year history. Apache-2.0 only. **No API stability guarantee** — breaking changes in consecutive minors (0.25.0 `Options::hold` → `drain_on_exit`; 0.26.0 `ChildEvent::Exited` `i32` → `ExitStatus`).
2. **`wezterm-term`** — more feature-complete (bidi, images, terminfo) but **not published on crates.io**. Means a git pin plus ~15 workspace-internal crates on edition 2018. `termwiz` alone is a TUI toolkit, not a state model.
3. **`libghostty-vt` 0.2.1** — safe Rust over Ghostty's core, optional kitty graphics. Upstream C header self-describes as "incomplete, work-in-progress… breaking changes are expected."

## Decision
`alacritty_terminal`, **behind our own `TerminalCore` trait** in `vt-core`. Nothing outside `vt-core` names an alacritty type.

## Consequences
- Breaking upstream changes are contained to one file.
- `libghostty-vt` is benchmarked against it in M0 on the same corpus; the trait makes a swap a crate change, not a rewrite.
- Apache-2.0-only licence is fine for a personal/internal tool; noted for any future distribution question.
- Kitty graphics and Sixel decoding are deferred (A1.9) — no good standalone Rust decoder exists; `libghostty-vt`'s kitty feature is the likeliest future source.

## Strategic note
libghostty is a **C** library, so Swift could call it directly with zero FFI tooling. If the Rust preference ever softens, that collapses ADR-0002 entirely. Worth remembering.
