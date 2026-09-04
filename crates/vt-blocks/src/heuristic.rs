//! Fallback segmentation when marks are absent or corrupted.
//!
//! Signals in descending reliability (docs/03 §6): terminal mode
//! transitions, idle detection, prompt-shape regex packs (data files, so
//! they update without a release). Output is always `Confidence::Heuristic`.
