//! Shared hook-handling abstraction for both vendors: common stdin fields,
//! `hookSpecificOutput`, `permissionDecision`, exit-2 blocking. Response
//! bodies stay small (10,000-char caps) and fast (per-event timeouts).
