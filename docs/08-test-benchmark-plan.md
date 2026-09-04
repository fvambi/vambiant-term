# 08 — Test & Benchmark Plan

## 1. What can actually be tested

The system splits cleanly:

| Layer | Testability | Approach |
|---|---|---|
| `vt-core`, `vt-pty`, `vt-blocks`, `vt-redact`, `vt-policy`, `vt-ai` | Fully headless | Unit + property + snapshot + fuzz, in CI |
| `vtermd`, adapters | Headless with fixtures and a fake agent | Integration tests against recorded event streams |
| FFI seam | Headless from both sides | Round-trip tests; ABI diff check |
| Swift UI | Partially | Snapshot tests for views; manual for the grid |
| Rendering | Only empirically | Golden-image comparison + a latency harness |

Everything below `app/` is the bulk of the system and none of it needs a GUI to test. That is the point of the architecture.

## 2. Terminal conformance

- **esctest2** (`esctest.py --expected-terminal=xterm`) run in CI against the headless harness. Maintain a **triaged known-fail list with a reason per entry** — an untriaged failure is a bug, a triaged one is a decision.
- **vttest** run manually per release and its results recorded. 100% is not the bar; even Ghostty has open vttest discussions. The bar is: no regression, and every fail explained.
- Compliance priority order, adopted from Ghostty: **(1) standards, (2) xterm behaviour, (3) other popular terminals.** When they conflict, that is the tie-break, and the choice gets a comment in the code.
- Snapshot tests: byte stream in → grid dump out, as text fixtures. Every bug fixed adds a fixture. This suite is the regression net for the entire terminal layer.
- Fuzzing: `cargo-fuzz` on the parser with a corpus seeded from real program output (vim, htop, tmux, `claude`, `codex`, cargo, git). Target: no panic, no unbounded allocation, no hang.
- Reflow: property test that resizing to width W and back to W' preserves logical line content for arbitrary content and arbitrary resize sequences. Reflow is where terminals go to die.

## 3. Redaction — the highest-stakes suite

Property test, run on every commit:

> For every secret S in the corpus, every insertion offset O in a carrier text, and every chunk-boundary split pattern P: the redacted output contains no contiguous substring of S longer than 8 characters.

Plus:

- Golden corpus of realistically-shaped fake credentials (one per rule) checked in, with a test that every rule fires at least once — a rule that never fires is a dead rule.
- False-positive corpus: real command output, base64 data, UUIDs, git hashes, lock files. Target under 2% false-positive rate on this corpus, but **recall wins every tie**.
- Idempotence: `redact(redact(x)) == redact(x)`.
- Fail-closed test: inject a panic into the pipeline, assert the request is dropped and surfaced, never sent.
- Multi-line state machine: a PEM block split across 40 one-byte chunks is still fully redacted.

## 4. Safety classifier

- Table-driven tests: a corpus of commands with expected classes, including the adversarial set — `ls && rm -rf /`, `$(echo cm0gLXJmCg== | base64 -d)`, `eval "$CMD"`, `curl x | sh`, `git push --force origin main`, quote-concatenation evasions of the shape VS Code's docs admit to.
- Assert **unparseable input classifies as unsafe**, never benign.
- Assert the **never-auto floor** cannot be overridden by any policy file — including a deliberately hostile `.vambiant-term/policy.toml` in the test corpus.

## 5. Agent adapters

- **Fixture replay**: real captured hook payloads and stream-json/JSONL lines from M0, replayed through each adapter, asserting the normalised `AgentEvent` output.
- **Contract tests**: assert that an unknown event name or an unknown field produces a logged warning and a degraded-but-correct event — never a panic, never a silent drop. This is the test that turns "the vendor changed the schema" from an outage into a warning.
- **Fake agent**: a small binary that emits a scripted event sequence including pathological cases — a permission request that is never answered, a crash mid-tool-call, 500 events in 100 ms, an event with a 2 MB payload.
- **Deferred-decision watchdog**: assert every deferred request either resolves or surfaces as "still waiting", and that returning `null`/`defer` without a follow-up answer is caught by the watchdog rather than hanging forever.
- **Round-trip**: an approval answered via the CLI unblocks a real `claude` process. Run against the real binary, nightly, not on every commit.

## 6. FFI

- Round-trip tests for every type crossing the boundary.
- **ABI diff check in CI**: regenerate `vt_ffi.h` with `cbindgen` and fail the build if it differs from the checked-in header without a corresponding Swift change. An FFI mismatch is a memory-safety bug, not a compile error.
- Thread-affinity assertions: debug builds panic if a main-thread-only callback fires off the main thread.
- ASan/TSan run on the Rust side of the boundary in a nightly job.
- Leak test: attach/detach 10,000 sessions, assert steady-state memory.

## 7. Performance budgets

Enforced, not aspirational. Recorded per commit; a regression beyond the threshold fails CI.

| Metric | Budget | How measured |
|---|---|---|
| Keystroke → glyph, p99 | **< 8.3 ms** (one frame at 120 Hz) | Typometer-class harness: synthetic key injection + screen capture. ⚠️ Published typometer numbers (Alacritty 6.9 ms, kitty 23.8 ms, WezTerm 26.1 ms) are from 2024 on Linux/Xorg and exclude Ghostty and iTerm2 — establish our own baseline on this Mac rather than comparing to them |
| Throughput (`cat` of a large UTF-8 file) | within **2×** of Ghostty on the same corpus | hyperfine, medians, serial runs, pre-generated inputs |
| Daemon attach → first frame | < 50 ms | |
| Daemon hop added latency, p99 | **< 1 ms** | Direct-PTY baseline vs daemon path. Exceeding this triggers the ADR-0004 fallback |
| Suggestion first token, p50 | **< 120 ms** local | Instrumented in `vt-ai`; this is the metric no existing terminal benchmark measures and the one users feel |
| ⌘K end-to-end, p50 | < 1.5 s | |
| Approval visible after `PermissionRequest`, p99 | **< 500 ms** | Hook receipt → inbox render |
| Idle CPU, 8 panes, 1 agent thinking | < 2% | |
| Memory, 8 panes, 10k scrollback each | < 400 MB RSS | |
| Cold start to first prompt | < 400 ms | |

Benchmark methodology, adopted from Ghostty's own rules: **generate input files separately** so generation cost is not measured, reuse identical files across revisions, time with hyperfine rather than shell piping, warm up, take **medians**, and **never run benchmarks in parallel**.

Note honestly: vtebench's own README says it "is not sufficient to get a general understanding of the performance of a terminal emulator. It lacks support for critical factors like frame rate or latency." Throughput is the easy number. Latency is the one that matters, and it needs its own harness.

## 8. Store and recovery

- Migration tests: every schema version upgrades from the previous one with real data.
- Crash-recovery test: `SIGKILL` the daemon with five live sessions; assert re-adoption on restart, and that anything unadoptable is marked `orphaned` rather than dropped.
- Retention sweep: assert data older than the configured window is actually deleted, including from the egress log.
- Corruption: truncate `state.db` mid-write; assert the daemon starts and reports the problem instead of crash-looping.

## 9. CI layout

| Job | Trigger | Contents |
|---|---|---|
| `fast` | every push | `cargo fmt --check`, `cargo clippy -D warnings`, unit tests, SwiftLint, SwiftFormat --lint |
| `full` | every PR | integration tests, snapshot tests, property tests, `cargo deny`, ABI diff, Swift build |
| `perf` | every PR | the budget table above, compared against the base commit |
| `conformance` | every PR | esctest2 with the triaged fail list |
| `nightly` | schedule | fuzzing, ASan/TSan, real-agent round-trip against installed `claude` and `codex`, leak tests |
| `release` | tag | build, sign, notarize, produce the `.app` and CLI, checksum |

All of it runs locally too: `mise run ci` is the same set. **A CI that cannot be run locally is a CI that gets ignored.**

## 10. Manual test plan (per release)

The things automation cannot honestly cover:

1. Type in every pane for two minutes. Does it feel like Ghostty? If not, the latency budget is being met on paper and missed in practice.
2. `vim`, `htop`, `tmux`, `nvim` with a heavy config, `fzf`, `lazygit` — full-screen TUIs are where emulation bugs live.
3. Resize aggressively during heavy output. Reflow, cursor position, no corruption.
4. Kill an agent mid-tool-call. Restart. Resume. Is the conversation intact?
5. Answer an approval from the CLI while the GUI is showing the same one. Both must update.
6. Powerlevel10k and starship: are blocks correct, and if not, does the app say so?
7. Pull the network mid-suggestion. Graceful, no hang, no stuck spinner.
8. `cat` a file containing every secret pattern in the corpus, then press ⌘K. Inspect the payload with `⌥⌘E`. **Nothing sensitive may appear.** This one is non-negotiable and is run every release.
