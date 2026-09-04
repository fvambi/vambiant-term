//! OSC 133 (FinalTerm / iTerm2 / Ghostty) prompt marks.
//!
//! `A` prompt start, `B` command start, `C` command executed, `D[;exit]`
//! finished. Parameters (`cl=line`, `click_events=1`, …) are parsed
//! **permissively** — unknown keys are ignored, never fatal (docs/10 §10).
//! Malformed sequences increment a counter that trips the corrupted-marks
//! warning rather than being silently dropped.
