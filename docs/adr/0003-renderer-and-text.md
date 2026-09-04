# ADR-0003 — Metal + CoreText in Swift; no wgpu, no Rust text stack

**Status:** Accepted · 2026-09-04

## Decision
Text shaping, font fallback, glyph rasterization and drawing all live in Swift: CoreText + a Metal glyph atlas on a `CAMetalLayer`. Rust sends cell content and attributes and never touches fonts.

## Rationale
- `CTFontCreateForString` **is** the system fallback cascade. `cosmic-text` approximates it with hardcoded lists borrowed from Chromium and Firefox.
- Colour emoji (`sbix`/`COLR`) via `CTFontDrawGlyphs` is free; variable fonts and `.ttc` handling are free.
- Ghostty — the most recent, most macOS-native reference — does exactly this: Metal + CoreText on macOS.
- wgpu 30.x ships quarterly breaking majors and would add ~29k LOC to draw a glyph atlas and coloured quads. It *can* render into an externally-owned `CAMetalLayer` (`SurfaceTargetUnsafe::CoreAnimationLayer`, `#[cfg(metal)]`-gated so it doesn't appear on docs.rs) — we simply don't need it.
- `winit` is unnecessary once Swift owns the NSView. Stable is 0.30.13; 0.31 has been in beta ~10 months.

## Consequences
- Ligature support is CoreText-grade. Ghostty's own open issues (#1645, #3128) show CoreText is **not** at HarfBuzz parity for arbitrary OpenType features. Accepted; terminal-safe defaults disable `fl`/`fi`/`st`.
- Grapheme-cluster segmentation is ours: a ZWJ emoji sequence must map to one wide cell before shaping.
- Run segmentation: break on grapheme → style → font index change (Ghostty's rule).
- Cross-platform rendering is explicitly out. The Rust core stays portable; the renderer does not pretend to be.
