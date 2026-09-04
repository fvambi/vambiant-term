//! Watchdog for deferred decisions (docs/03 §4.6).
//!
//! A `defer`/`null` without a follow-up hangs the agent forever, so every
//! deferred request is tracked here and surfaced as "still waiting" until
//! it resolves.
