//! `alacritty_terminal` backend — the documented fallback (ADR-0001).
//!
//! Not implemented: the M1 gates in the ADR-0001 amendment were met and
//! `libghostty-vt` is the primary backend. The verified API shapes stay here so
//! a fallback implementation is a mechanical task. The M0 spike established (verified on
//! 2026-09-04, `alacritty_terminal` 0.26.0):
//! * `Term::damage(&mut self) -> TermDamage<'_>` with
//!   `TermDamage::{Full, Partial(TermDamageIterator)}`, iterator item
//!   `LineDamageBounds { line, left, right }`;
//! * `Term::reset_damage(&mut self)` must be called after consuming it;
//! * `Term::new(Config, &dyn Dimensions, EventListener)`; resize via
//!   `Term::resize(TermSize)`.
//!
//! See docs/10-research-notes.md §1 for the full record.
