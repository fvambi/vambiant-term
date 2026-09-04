# ADR-0007 — Build the redaction pipeline; fail closed

**Status:** Accepted · 2026-09-04

## Context
"Cloud-first for quality, redaction always on" puts the entire privacy story on one component. Research found **no well-maintained Rust crate that does gitleaks-grade secret detection**, and nothing at all that handles streaming chunk boundaries.

## Decision
Build `vt-redact`:
- **Layer 1**: vendor gitleaks' MIT-licensed regexes into a single `RegexSet` (Go RE2 ≈ Rust `regex`, so rules port unmodified).
- **Layer 2**: ripsecrets-style entropy + character-class scoring for unprefixed secrets, gated on the surrounding identifier so `DEBUG=true` survives.
- **Streaming**: sliding window of `max_pattern_len + margin` across chunk boundaries; a stateful mode for multi-line PEM blocks.
- Redaction runs **when context is built**, before anything enters a prompt — not as an egress filter.
- Redact the **typed input region** (OSC 133 B→C) as well as output. `export API_KEY=…` is the most common leak and never appears in "output".
- Wrap detected values in `zeroize`.

## Fail closed
A panic, timeout or internal error means **the request is not sent**. Never fail open on a redaction bug.

## Verification
Property test: for every secret S, every insertion offset, and every chunk-split pattern, the output contains no contiguous substring of S longer than 8 characters. Plus a golden corpus (every rule must fire at least once) and a false-positive corpus of real command output, hashes and lock files. **Recall wins every tie** — over-redacting costs a worse answer, under-redacting costs a rotated credential.

## Rejected
`secrecy` (wraps values you hold, does not detect), `ripsecrets` as a dependency (binary-first, 11.6K downloads), `redact-core` (PII-focused, pre-1.0), `secretscan` (3K downloads).
