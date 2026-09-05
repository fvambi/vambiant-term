# 07 — Implementation Plan

## 0. Sequencing philosophy

You chose "full spec, no compromise". That does not mean building everything before using anything — it means not cutting features to reach a ship date. The ordering below still front-loads the parts that give you daily value early, because a tool you use is a tool you can correct.

The dependency spine: **PTY + VTE → shell integration → blocks → daemon → adapters → inbox → providers → intelligence → autonomy.**

Two tracks run in parallel from day one because they touch nothing else and are headless-testable: the **provider layer** (M-AI) and the **redaction pipeline** (M-SEC).

> **M2 is the first milestone that changes your daily life.** The daemon + CLI work inside Ghostty. You get the session manager and the approval inbox months before the GUI terminal exists. Everything after that is upgrading the surface, not discovering the product.

## M0 — Ground truth (1 week)

Nothing is built on assumptions. Before writing product code, verify the three surfaces that this entire design rests on and that are documented as unstable.

- [x] Scaffold the workspace: `Cargo.toml`, crate skeletons, `mise.toml`, CI, `cargo deny`, `rust-toolchain.toml` pinned ≥ 1.98.1. *(done 2026-09-04; licence still undecided)*
- [x] Verify `alacritty_terminal` 0.26.x against a real PTY: spawn, parse, damage, resize, reflow. Confirm the `Term::damage()` / `TermDamage` API shapes (flagged unverified in research). *(done; `docs/10` §1)*
- [x] **Spike `libghostty-vt` 0.2.1 in parallel** and benchmark both on the same input corpus. ADR-0001 is decided in favour of `alacritty_terminal` on maturity grounds but is explicitly revisitable — this is the moment to revisit it, not later. *(done; benchmark and a proposed ADR-0001 amendment await a decision)*
- [x] Install Claude Code hooks against the real binary. Confirm every event name in `03-agent-integration.md` §4.2 actually fires, capture real payloads to fixtures, and confirm the `http` handler type works with a loopback receiver. *(26/33 events captured; 7 not reached — `docs/10` §6)*
- [x] Install a status line command and capture real stdin JSON to a fixture.
- [x] Install Codex hooks and connect to `codex app-server` over a Unix socket. Capture real JSON-RPC traffic. *(6 hook events captured; `PermissionRequest`/compaction/subagent/`Interrupt` not reached)*
- [x] Record every finding in `10-research-notes.md` with an "as verified on <date>, version <x>" line.

**Exit:** fixtures on disk for both agents' real event streams; a written go/no-go on the terminal core; no undocumented assumption remaining in the design.

**Risk if skipped:** every subsequent milestone is built on a guess about a schema the vendors say will change.

## M1 — Terminal core (3–4 weeks)

- [x] `vt-pty`: spawn under `login`, `-q` with `~/.hushlogin`, winsize, `SIGWINCH`/`SIGCHLD`, drain-on-exit. *(2026-09-05; libc only, five tests)*
- [x] `vt-core`: `TerminalCore` trait; **`libghostty-vt` implementation** behind it (ADR-0001 amendment, gates met); grid, scrollback, damage, events. `alacritty_terminal` stays the documented fallback, unimplemented.
- [x] Headless renderer harness: feed a byte stream, dump the grid as text. This is the test substrate for everything after. *(`vt_core::harness`, `tests/fixtures/vt/`)*
- [x] Kitty keyboard protocol push/pop; OSC 7/8/52-write/133/633/777/1337 handling. *(kitty + DECCKM + modifyOtherKeys via the backend encoder; OSC 7/1337 cwd, OSC 52 write and OSC 1337 Copy as `TermEvent`s; OSC 133/633 and OSC 8 exposed per row/cell for vt-blocks; ⚠️ OSC 777 notifications not surfaced by libghostty-vt 0.2.1 — docs/10 §1)*
- [x] `terminfo` entry. *(`terminfo/vambiant-term.terminfo`, derived from Ghostty's since the core is libghostty; `scripts/terminfo-install.sh`)*
- [x] esctest2 run and triaged: **568 run, 487 pass, 70 triaged failures, 11 esctest-known-bug** on 2026-09-05 (`mise run conformance`, list with reasons in `tests/conformance/esctest-known-fail.txt`; every failure is an unanswered query — DECRQM, DECRQSS, extended DECDSR, xterm window ops, OSC 5 — not a rendering error). ⚠️ vttest is a manual per-release run (docs/08 §2); not yet done.
- [x] vtebench-style throughput harness following Ghostty's methodology: pre-generated inputs, hyperfine, medians, serial runs. *(`scripts/bench/run.sh`; `vtcore-spike` measures the trait overhead over bare libghostty)*

**Exit:** headless terminal passes the triaged conformance set; throughput within 2× of Ghostty on the same corpus.

> **M1 exit met on 2026-09-05.** `mise run conformance`: 487/568 esctest2 tests pass with every failure triaged (all unanswered queries, none rendering). Throughput through `TerminalCore` is 1.00–1.24× raw libghostty-vt on the six M0 corpora (hyperfine medians, serial: ascii 40→40 ms, japanese 39→48, sgr 106→112, cursor 89→96, long-lines 28→35, scroll-region 38→38), i.e. at parity with Ghostty's own parser. Open: vttest is a manual per-release run; OSC 777 notifications wait on a libghostty callback (docs/10 §1).

## M2 — Daemon + CLI (3–4 weeks) ← *first daily-driver milestone*

- [x] `vtermd` as a launchd LaunchAgent with `KeepAlive`; socket 0700 with peer credential checks. *(`vterm daemon install|start|stop|uninstall` writes and bootstraps `~/Library/LaunchAgents/com.vambiant.term.vtermd.plist`; socket + `getpeereid` in `vt-ipc`)*
- [x] Session registry, PTY ownership, attach/detach, snapshot + damage delta protocol. *(`vtermd`: one thread per session, deltas ≥ 8 ms apart, `session.*` methods in `vt-proto`)*
- [x] `vt-store`: SQLite schema for sessions, blocks, events, decisions, egress, worktrees. WAL. Migrations from commit one.
- [x] `vterm` CLI: `ls`, `new`, `attach`, `kill`, `rename`, `logs`, all with `--json`. *(plus `send`, `daemon status|start`; `attach` is a raw-mode viewer with Ctrl-\ d to detach)*
- [x] Crash recovery: daemon restart re-adopts sessions; unadoptable sessions marked `orphaned`, never dropped silently. *(2026-09-05: per-session `vtermd-hold` fd holder — ADR-0004 amendment accepted; re-adoption, recorded-exit and orphan paths all covered by end-to-end tests)*
- [ ] Loopback HTTP/WS API with a Keychain-stored bearer token. *(not started)*

**Exit:** you can start an agent with `vterm new --agent claude`, close the Ghostty window, and reattach. Sessions survive a daemon restart.

> **M2 exit met 2026-09-05** except the loopback HTTP/WS API: `vterm new -- claude` / `vterm attach` / detach with Ctrl-\ d / reattach work against a launchd-managed daemon, sessions survive closing the window **and** a daemon crash or restart (re-adopted from their fd holders, labelled `readopted`). `--agent claude` provisioning is M3. The loopback HTTP/WS API with a Keychain token is deferred behind M3's adapters.

## M3 — Agent adapters + the inbox (4–5 weeks)

- [x] `AgentAdapter` trait and the normalised `AgentEvent` model. *(`vt-proto::agent`, `vt-agent::adapter`; unknown events/fields degrade to `Unknown` + warning, contract-tested)*
- [~] Claude adapter: session-scoped `--settings` file, HTTP hook handlers, status line receiver, `--include-hook-events` ingestion, `claude agents --json` cooperation. *(settings file + http hooks + status line relay + inbox done 2026-09-05; `stream-json` lines on the session's stdout are ingested (init, assistant text/tool_use/thinking, tool_result, result usage+cost, rate limits, permission_denied); `claude agents --json` cooperation pending)*
- [x] The deferred-decision protocol (§4.6 of `03`) with watchdogs on every deferred request. *(hold-then-answer, reminders every 120 s with a desktop notification and `reminded ×N` in the inbox, withdrawal when the agent's own prompt was answered; tested end to end)*
- [~] Codex adapter: `app-server` JSON-RPC client (`thread/*`, `turn/*`), hooks, `codex exec --json`. *(app-server observer with approvals in the inbox done and verified live 2026-09-05; hooks deliberately unused (trust cannot be granted programmatically, see `10` §7); `codex exec --json` ingestion pending)*
- [x] Generic adapter: heuristics with per-agent regex packs as data files. *(OSC 133 marks, alt-screen/bracketed-paste/kitty transitions, 700 ms idle detection, prompt-shape packs: built-in `crates/vt-agent/packs/generic.json` plus `<state>/packs/*.json`; every verdict is recorded as a `guess` with its evidence and the session is labelled. No other agent CLI was installed to verify vendor-specific packs against, so only Codex/Claude TUI forms are verified; aider/gemini/opencode packs are still to be captured)*
- [x] Approval queue; `vterm inbox list|allow|deny|edit`. *(`edit` opens `$VISUAL`/`$EDITOR` on the tool input or takes `--input`, then allows with `updatedInput`; refused for Codex, whose approvals are accept/decline only)*
- [~] Notification Center integration with actionable notifications. *(plain notifications via `osascript` from the daemon; actionable ones need the app bundle, M4)*
- [~] Degraded-mode detection and honest labelling. *(a Claude session with no hook event within 30 s and a Codex session whose app-server is unreachable or closed carry a `degraded` reason; `vterm ls` marks the state with `!` and prints the reason. Managed-settings detection (`allowManagedHooksOnly`) is not yet distinguished from other causes)*

**Exit:** three agents running across two repos; every blocked one appears in `vterm inbox` within 500 ms; answering from the CLI unblocks the agent.

> **M3 status 2026-09-05:** verified against the real `claude` 2.1.261 binary under `vtermd`: `vterm new --agent claude -- claude -p --permission-mode default --permission-prompts none …` → the `Write` permission request was held by the daemon's hook receiver, listed by `vterm inbox`, and `vterm inbox allow` unblocked the agent (file written, `done`, session ended) with the full sequence in `vterm events`. Latency to the inbox was well under 500 ms (the 7 s in the log is the model's own thinking time before the tool call). Still to do in M3: `claude agents --json` cooperation, `codex exec --json` ingestion (the adapter accepts the lines; nothing feeds them yet), vendor-specific prompt packs, and the three-agents-two-repos exit run.
>
> **Codex, 2026-09-05:** `vterm new --agent codex -- codex --sandbox workspace-write -c sandbox_workspace_write.exclude_slash_tmp=true …` against the real `codex-cli 0.153.2`: the daemon's per-session app-server observer resumed the TUI's thread, the sandbox-blocked `touch` appeared in `vterm inbox` with the exact command and cwd while the TUI showed its own prompt, `vterm inbox allow` answered it over JSON-RPC, the TUI continued ("blocked initially, then succeeded on the single retry with escalated permissions"), and the app-server was terminated with the session. Mechanism details in `10` §7.

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
