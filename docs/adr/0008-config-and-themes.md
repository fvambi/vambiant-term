# ADR-0008 — TOML config, hot-reloaded, validated loudly; themes imported

**Status:** Accepted · 2026-09-04

## Decision
- **TOML**, hot-reloaded on write, schema-validated. An invalid config shows an error overlay naming file, line and key. **Never a silent fallback to defaults** — that is how you lose an hour wondering why a setting does nothing.
- A JSON Schema is published for editor completion.
- Secrets never live in config. `providers.toml` holds `key_ref = "keychain:…"` and the value is resolved at call time.
- Per-repo `.vambiant-term/policy.toml` is **intersected** with the global policy: it can narrow, never widen. A repo may set `egress_mode = "none"` over a global `redacted`; it may not set `"full"`.
- **Themes**: native TOML format, with importers for Ghostty, iTerm2 `.itermcolors`, Alacritty and base16/tinted-theming, offered at first run. Imported themes are contrast-checked — a failure warns, it does not reject.
- **Keymaps**: two complete profiles, `tmux` (default for multiplexer actions) and `macos`, mixable. `vterm keys` prints the resolved map and flags conflicts.

## Rejected
KDL (Zellij's choice — nicer for nested layouts, worse for a config people hand-edit alongside every other TOML in their life), YAML (whitespace), JSON (no comments).

## Note
Ghostty config import was considered and **not** included per your selection. Theme import covers the part that matters day one.
