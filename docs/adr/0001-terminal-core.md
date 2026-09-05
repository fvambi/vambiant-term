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

## M0 amendment — proposed 2026-09-05, awaiting decision

**Status of this section: Proposed; both gates met on 2026-09-05.** M1 implements `libghostty-vt` as the primary `TerminalCore` backend on that basis; the decision above is superseded once Florian confirms.

M0 benchmarked both cores on the same 32 MiB corpora (`docs/10` §1): `libghostty-vt` 0.2.1 is 3.5–5.8× faster than `alacritty_terminal` 0.26.0 on five of six workloads and 1.2× on SGR-heavy input. The verified API shapes of both are recorded in `docs/10` §1.

Proposed change: make `libghostty-vt` the primary backend behind `TerminalCore`, keep `alacritty_terminal` as the compiled fallback through M1, and drop it at M1 exit if two gates hold:

1. **Grid parity** — ✅ met 2026-09-05: with CRLF input all six corpora are byte-identical; the only divergence is that alacritty_terminal keeps the pending-wrap state across a linefeed while libghostty-vt clears it (xterm behaviour). Fixture `tests/fixtures/vt/pending-wrap-lf/`.
2. **Hermetic build** — ✅ met 2026-09-05: Ghostty is vendored at the pinned commit as the submodule `third_party/ghostty`; `mise.toml` exports `GHOSTTY_SOURCE_DIR`, `LIBGHOSTTY_VT_SYS_OPTIMIZE=ReleaseFast` and puts the `xcrun` shim on `PATH`, so `cargo build` under mise never clones. Remaining network use: zig fetches Ghostty's own package dependencies into its global cache on first build (`GHOSTTY_ZIG_SYSTEM_DIR` can pre-seed that for a fully offline CI).

Consequences if accepted: `!Send`/`!Sync` terminal handles (one per reader thread — already the docs/02 §4 design), a second toolchain (zig) in `mise.toml`, a pre-1.0 C API with expected breaking changes, and kitty graphics available without a Rust decoder. The strategic note above (Swift calling libghostty directly) becomes a live option rather than a hypothetical.
