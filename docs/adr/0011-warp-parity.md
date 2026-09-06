# ADR-0011 — Look like Warp, ship Warp's feature set

**Status:** Accepted · 2026-09-06

## Context
`00 §4` said "not a Warp clone" and "not an agent". On 2026-09-06 the owner reversed both: Vambiant Term must look like Warp and ship the same features. `11-warp-feature-inventory.md` lists those features; `12-warp-parity-plan.md` schedules them.

## Decision
1. **Visual parity.** The default appearance is Warp's: a dark block-based layout with a dim context line above a bold command, hover actions per block, a bottom input area with context chips, red sidebar and tint on failure, a vertical tab sidebar with agent badges and diff stats. `12 §L` is the spec, derived from Warp's published screenshots, not its code.
2. **Warp mode input.** The app owns the line editor by default (`[input] mode = "warp"`): a native editor pinned at the bottom submits commands to the shell, whose own prompt is made invisible by the injected integration. `mode = "classic"` keeps the shell's editor. This reverses `12 §0.1`.
3. **Agent Mode.** A first-party conversation view over the provider layer (`04`) that can run commands and edit files under `vt-policy`, plus the ACP client for third-party agents. "Not an agent" in `00 §4` is withdrawn; ADR-0009's floor and defaults are unchanged: every autonomous behaviour ships off.
4. **Code features.** The code review panel, the file tree and a basic editor are in scope; LSP is a later tier.
5. **What stays different.** No cloud, no account, no telemetry, redaction always on (`05`). Warp Drive, sharing, Remote Control, cloud agents and settings sync ship as local, file-based equivalents (`12 §0.3`), and the UI says so where Warp would show a cloud feature.

## Consequences
- `00 §4` is amended in the same commit; `01` gains rows for every adopted feature as they land.
- Warp is AGPL v3. Its documentation and screenshots are references; **its source is never read or copied** into this MIT codebase. Themes and completion specs are re-derived or taken only from MIT sources (Warp's screenshots for colours, withfig/autocomplete for specs).
- Schedule: `12 §S` is re-based; M5.5 becomes "Warp mode", M10 becomes "Agent Mode".
