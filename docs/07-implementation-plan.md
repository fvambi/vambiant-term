# 07 — Implementation Plan

## 0. Sequencing philosophy

You chose "full spec, no compromise". That does not mean building everything before using anything — it means not cutting features to reach a ship date. The ordering below still front-loads the parts that give you daily value early, because a tool you use is a tool you can correct.

The dependency spine: **PTY + VTE → shell integration → blocks → daemon → adapters → inbox → providers → intelligence → autonomy.**

Two tracks run in parallel from day one because they touch nothing else and are headless-testable: the **provider layer** (M-AI) and the **redaction pipeline** (M-SEC).

> **M2 is the first milestone that changes your daily life.** The daemon + CLI work inside Ghostty. You get the session manager and the approval inbox months before the GUI terminal exists. Everything after that is upgrading the surface, not discovering the product.

## M0 — Ground truth (1 week)

Nothing is built on assumptions. Before writing product code, verify the three surfaces that this entire design rests on and that are documented as unstable.

- [ ] Scaffold the workspace: `Cargo.toml`, crate skeletons, `mise.toml`, CI, `cargo deny`, `rust-toolchain.toml` pinned ≥ 1.98.1.
- [ ] Verify `alacritty_terminal` 0.26.x against a real PTY: spawn, parse, damage, resize, reflow. Confirm the `Term::damage()` / `TermDamage` API shapes (flagged unverified in research).
- [ ] **Spike `libghostty-vt` 0.2.1 in parallel** and benchmark both on the same input corpus. ADR-0001 is decided in favour of `alacritty_terminal` on maturity grounds but is explicitly revisitable — this is the moment to revisit it, not later.
- [ ] Install Claude Code hooks against the real binary. Confirm every event name in `03-agent-integration.md` §4.2 actually fires, capture real payloads to fixtures, and confirm the `http` handler type works with a loopback receiver.
- [ ] Install a status line command and capture real stdin JSON to a fixture.
- [ ] Install Codex hooks and connect to `codex app-server` over a Unix socket. Capture real JSON-RPC traffic.
- [ ] Record every finding in `10-research-notes.md` with an "as verified on <date>, version <x>" line.

**Exit:** fixtures on disk for both agents' real event streams; a written go/no-go on the terminal core; no undocumented assumption remaining in the design.

**Risk if skipped:** every subsequent milestone is built on a guess about a schema the vendors say will change.

## M1 — Terminal core (3–4 weeks)

- [ ] `vt-pty`: spawn under `login`, `-q` with `~/.hushlogin`, winsize, `SIGWINCH`/`SIGCHLD`, drain-on-exit.
- [ ] `vt-core`: `TerminalCore` trait; `alacritty_terminal` implementation behind it; grid, scrollback, damage, modes.
- [ ] Headless renderer harness: feed a byte stream, dump the grid as text. This is the test substrate for everything after.
- [ ] Kitty keyboard protocol push/pop; OSC 7/8/52-write/133/633/777/1337 handling.
- [ ] `terminfo` entry.
- [ ] esctest2 + vttest run and triaged (100% is not the bar — no shipping terminal clears it; a triaged known-fail list is).
- [ ] vtebench-style throughput harness following Ghostty's methodology: pre-generated inputs, hyperfine, medians, serial runs.

**Exit:** headless terminal passes the triaged conformance set; throughput within 2× of Ghostty on the same corpus.

## M2 — Daemon + CLI (3–4 weeks) ← *first daily-driver milestone*

- [ ] `vtermd` as a launchd LaunchAgent with `KeepAlive`; socket 0700 with peer credential checks.
- [ ] Session registry, PTY ownership, attach/detach, snapshot + damage delta protocol.
- [ ] `vt-store`: SQLite schema for sessions, blocks, events, decisions, egress, worktrees. WAL. Migrations from commit one.
- [ ] `vterm` CLI: `ls`, `new`, `attach`, `kill`, `rename`, `logs`, all with `--json`.
- [ ] Crash recovery: daemon restart re-adopts sessions; unadoptable sessions marked `orphaned`, never dropped silently.
- [ ] Loopback HTTP/WS API with a Keychain-stored bearer token.

**Exit:** you can start an agent with `vterm new --agent claude`, close the Ghostty window, and reattach. Sessions survive a daemon restart.

## M3 — Agent adapters + the inbox (4–5 weeks)

- [ ] `AgentAdapter` trait and the normalised `AgentEvent` model.
- [ ] Claude adapter: session-scoped `--settings` file, HTTP hook handlers, status line receiver, `--include-hook-events` ingestion, `claude agents --json` cooperation.
- [ ] The deferred-decision protocol (§4.6 of `03`) with watchdogs on every deferred request.
- [ ] Codex adapter: `app-server` JSON-RPC client (`thread/*`, `turn/*`), hooks, `codex exec --json`.
- [ ] Generic adapter: heuristics with per-agent regex packs as data files.
- [ ] Approval queue; `vterm inbox list|allow|deny|edit`.
- [ ] Notification Center integration with actionable notifications.
- [ ] Degraded-mode detection and honest labelling.

**Exit:** three agents running across two repos; every blocked one appears in `vterm inbox` within 500 ms; answering from the CLI unblocks the agent.

## M4 — The GUI terminal (5–6 weeks)

- [ ] `vt-ffi` C ABI + `cbindgen`; header checked in and diffed in CI. `#[unsafe(no_mangle)]`, `extern "C-unwind"` where Swift can unwind through.
- [ ] `swift-bridge` for the cold path.
- [ ] SwiftPM app: window, tabs, splits, `MetalGridView` on a `CAMetalLayer`.
- [ ] CoreText shaping, run segmentation by grapheme → style → font, glyph atlas, colour emoji.
- [ ] `CADisplayLink` presentation, ProMotion-adaptive, idle when clean.
- [ ] Attach to `vtermd`; measure the daemon hop against a direct-PTY baseline. **If it costs > 1 ms p99, execute the ADR-0004 fallback** (foreground pane's PTY in-process, background sessions in the daemon).
- [ ] `Scripts/bundle.sh`: `.app` assembly, `Info.plist`, entitlements, codesign.

**Exit:** p99 keystroke→glyph < 8.3 ms measured with a typometer-class harness on this Mac; visually indistinguishable from Ghostty at rest.

## M5 — Blocks and the sidebar (3 weeks)

- [ ] `vt-blocks`: OSC 133/633 segmentation; heuristic fallback; corrupted-marks detection with a one-time warning.
- [ ] Shell integration snippets for zsh/fish/bash with auto-injection; the bash `\[ \]` prompt-width fix; Powerlevel10k/starship detection.
- [ ] Block rendering: headers, collapse, exit chips, copy/rerun/explain.
- [ ] Agent event blocks: tool calls, diffs, thinking, assistant markdown.
- [ ] Cost and context meters from the status line feed.
- [ ] Sidebar: repo → worktree → session tree with state glyphs.
- [ ] Inbox as a sheet, full keyboard triage.

**Exit:** an agent session reads as a structured timeline, not a wall of text.

## M-AI (parallel from M1) — Provider layer (3 weeks of effort, spread)

- [ ] `Provider` trait; capability tables including `SamplingSupport`.
- [ ] Anthropic Messages client: streaming, prompt caching with breakpoint budgeting, `count_tokens`.
- [ ] OpenAI Responses + Chat clients; generic OpenAI-compatible adapter.
- [ ] Local adapters: Ollama, `llama-server` (Messages + `/infill`), MLX, LM Studio.
- [ ] Route table, fallback chains, circuit breakers, cancellation.
- [ ] Cost accounting, budgets with hard stop, `vterm ai spend`.
- [ ] Keychain integration; `vterm ai doctor`.

**Exit:** `vterm ask` works against all four provider families; `vterm ai doctor` prints resolved capabilities and flags missing models.

## M-SEC (parallel from M1) — Redaction & policy (3 weeks of effort, spread)

- [ ] `vt-redact`: gitleaks rule import into a `RegexSet`, entropy layer, streaming sliding window, stateful multi-line PEM mode.
- [ ] Property tests across all chunk boundaries; golden corpus in CI.
- [ ] `vt-policy`: POSIX shell parser, classification, the never-auto floor.
- [ ] Per-workspace policy files with intersection semantics.
- [ ] Egress log and `vterm egress` commands.

**Exit:** the property test suite passes; a synthetic secret cannot survive any chunking; the classifier correctly refuses `ls && rm -rf /`.

## M6 — Intelligence in the terminal (3–4 weeks)

- [ ] Inline ghost text with history-first ranking, debounce, cancellation, late-discard.
- [ ] ⌘K palette with staging, explanation and verdict.
- [ ] `⌥E` explain-failure returning a patch, not prose.
- [ ] Context builder with inspectable payload (`⌥⌘E`).
- [ ] Safety verdicts surfaced inline on typed commands and in the inbox.

**Exit:** measured p50 suggestion latency under 120 ms with a warm local model; zero suggestions rendered after the user has typed past them.

## M7 — Worktrees & orchestration (3 weeks)

- [ ] `vt-worktree`: registry, create/adopt/destroy, ownership guard.
- [ ] `vterm task new/list/promote`; `⌘⇧N` in the GUI.
- [ ] Parallel-agent racing with side-by-side diff comparison.
- [ ] Stale worktree sweep.
- [ ] `WorktreeCreate`/`WorktreeRemove` hook cooperation.

**Exit:** three agents on one repo, three trees, zero collisions, one command to promote a winner.

## M8 — Autonomy (3 weeks, all defaults off)

- [ ] Policy rule evaluation on the hot path of `PermissionRequest`.
- [ ] Dry-run mode with a decision log.
- [ ] Watchdog and loop detection.
- [ ] Background task queue over `claude --bg` / `codex exec`.
- [ ] Audit log, `vterm audit`, undo handles.

**Exit:** a week of dry-run logs on real usage that you would have agreed with.

## M9 — Polish, packaging, docs (3 weeks)

- [ ] Config hot reload with an error overlay; JSON Schema.
- [ ] Theme import (Ghostty/iTerm2/Alacritty/base16).
- [ ] Session restore across reboot; layouts.
- [ ] Onboarding sheet.
- [ ] Accessibility pass.
- [ ] Hardened runtime, notarization path, `mise run release`.
- [ ] User docs and `vterm help` coverage.

---

## Risk register

| # | Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|---|
| R1 | Agent hook/event schemas change under us | **High** — both vendors say so | Medium | Fixtures from M0; adapters isolate; our own event log is the durable record; contract tests that fail loudly on unknown fields rather than silently dropping them |
| R2 | `alacritty_terminal` breaking change | Medium — it ships breaking changes in consecutive minors | Low | `TerminalCore` trait; `libghostty-vt` benchmarked in M0 as a live alternative |
| R3 | Daemon hop costs latency | Medium | High | Measured in M4 against a direct baseline; documented fallback in ADR-0004 |
| R4 | CoreText shaping gaps vs HarfBuzz | Medium — Ghostty has open issues on exactly this | Medium | Terminal-safe ligature defaults; accept the gap; revisit only if a real font breaks |
| R5 | Redaction misses a secret | Low if built as specified | **Severe** | Property tests at every boundary; recall over precision; fail closed; egress log makes a miss discoverable after the fact |
| R6 | `swift-bridge` maintenance is intermittent (0.1.x, 15-month gaps between releases) | Medium | Low | Cold path only; the hot path is plain C ABI which cannot rot |
| R7 | Enterprise policy blocks hooks | Low for you personally | Medium | Degraded mode designed in from M3, not bolted on |
| R8 | Scope. This is a large system | **High** | High | M2 delivers value early; every milestone has a standalone exit criterion; nothing after M5 is required for daily use |
| R9 | Codex `app-server` API churn (it is young) | Medium | Medium | Adapter isolation; `codex exec --json` as the fallback path |
| R10 | Model deprecation (e.g. Haiku 4.5 retirement, no announced successor) | High | Low | No hardcoded model ids; `vterm ai doctor` warns on missing models |

## Definition of done, per milestone

A milestone is done when: exit criteria are met, tests are green in CI, the perf budget is measured and recorded (not estimated), the docs in this folder reflect what was actually built, and any decision that changed is written up as a new ADR or an amendment to an existing one. **Docs that lie are worse than no docs** — this rule is in `CLAUDE.md` too.
