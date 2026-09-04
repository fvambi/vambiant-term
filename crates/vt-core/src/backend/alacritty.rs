//! `alacritty_terminal` backend (ADR-0001).
//!
//! Implementation lands in M1. The M0 spike established (verified on
//! 2026-09-04, `alacritty_terminal` 0.26.0):
//! * `Term::damage(&mut self) -> TermDamage<'_>` with
//!   `TermDamage::{Full, Partial(TermDamageIterator)}`, iterator item
//!   `LineDamageBounds { line, left, right }`;
//! * `Term::reset_damage(&mut self)` must be called after consuming it;
//! * `Term::new(Config, &dyn Dimensions, EventListener)`; resize via
//!   `Term::resize(TermSize)`.
//!
//! See docs/10-research-notes.md §1 for the full record.
