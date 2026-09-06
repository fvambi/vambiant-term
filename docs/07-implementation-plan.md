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
- [x] Notifications (2026-09-06): mailbox (`⌘⇧M`), toasts, Notification Center for long commands, agent stop/crash and Agent Mode when the app is in the background; policy from `[notifications]`, coalesced per session (docs/06 §8 as-built).
- [x] The app's inbox (2026-09-06): approval card in the pane, `✋ n` sidebar badge, the `⌘⇧A` sheet with `j/k/a/d/e/⎋`, edit & allow, verdict and floor inline (docs/06 §3 as-built).
- [x] Approval queue; `vterm inbox list|allow|deny|edit`. *(`edit` opens `$VISUAL`/`$EDITOR` on the tool input or takes `--input`, then allows with `updatedInput`; refused for Codex, whose approvals are accept/decline only)*
- [~] Notification Center integration with actionable notifications. *(plain notifications via `osascript` from the daemon; actionable ones need the app bundle, M4)*
- [~] Degraded-mode detection and honest labelling. *(a Claude session with no hook event within 30 s and a Codex session whose app-server is unreachable or closed carry a `degraded` reason; `vterm ls` marks the state with `!` and prints the reason. Managed-settings detection (`allowManagedHooksOnly`) is not yet distinguished from other causes)*

**Exit:** three agents running across two repos; every blocked one appears in `vterm inbox` within 500 ms; answering from the CLI unblocks the agent.

> **M3 exit met 2026-09-05** against the real binaries (`claude` 2.1.261, `codex-cli` 0.153.2) under a debug `vtermd`: two Claude sessions in two repos (`/tmp/vt-m0/claude/work`, `/tmp/vt-m3/repo2`) and one Codex session, each blocked on a permission (`Write` ×2 via the hook receiver, a sandbox-escalating `shell` via app-server). All three appeared in the inbox; measured over the IPC socket with a 5 ms poll against millisecond `requested_at`, the Claude items were visible **2 ms and 4 ms** after the daemon queued them (the CLI adds its own process start-up, ~100–500 ms in a debug build). Allowing each from the inbox unblocked the agent: both files were written, Codex's blocked `touch` succeeded on its escalated retry. One caveat observed on the way: a Claude item left unanswered for the 90 s hold falls back to the agent's own prompt, which under `--permission-prompts none` auto-denies — the session ends with the model saying so, and the event log shows it.

> **M3 status 2026-09-05:** verified against the real `claude` 2.1.261 binary under `vtermd`: `vterm new --agent claude -- claude -p --permission-mode default --permission-prompts none …` → the `Write` permission request was held by the daemon's hook receiver, listed by `vterm inbox`, and `vterm inbox allow` unblocked the agent (file written, `done`, session ended) with the full sequence in `vterm events`. Latency to the inbox was well under 500 ms (the 7 s in the log is the model's own thinking time before the tool call). Left open in M3: `claude agents --json` cooperation (the fixture and shape are captured; what "cooperate" should mean for `vterm ls` is a product decision — see `03` §4), vendor-specific prompt packs (no other agent CLI installed to capture from), and `vterm prune` for holders whose daemon is gone.
>
> **Codex, 2026-09-05:** `vterm new --agent codex -- codex --sandbox workspace-write -c sandbox_workspace_write.exclude_slash_tmp=true …` against the real `codex-cli 0.153.2`: the daemon's per-session app-server observer resumed the TUI's thread, the sandbox-blocked `touch` appeared in `vterm inbox` with the exact command and cwd while the TUI showed its own prompt, `vterm inbox allow` answered it over JSON-RPC, the TUI continued ("blocked initially, then succeeded on the single retry with escalated permissions"), and the app-server was terminated with the session. Mechanism details in `10` §7.

## M4 — The GUI terminal (5–6 weeks)

- [x] `vt-ffi` C ABI + `cbindgen`; header checked in and diffed in CI. `#[unsafe(no_mangle)]`, `extern "C-unwind"` where Swift can unwind through. *(`vt_viewer_*` hot path, `vt_daemon_call` cold path; `crates/vtermd/tests/ffi_viewer.rs` drives it against the real daemon)*
- [~] `swift-bridge` for the cold path. *(deferred — the cold path is a JSON-RPC passthrough over the contract the CLI already exercises; ADR-0002 amendment 2026-09-05)*
- [x] SwiftPM app: window, tabs, splits, `MetalGridView` on a `CAMetalLayer`. *(native window tabbing, nested `NSSplitView` splits, geometric focus, zoom; both keymap profiles from `06` §7 with unimplemented actions named, not swallowed)*
- [~] CoreText shaping, run segmentation by grapheme → style → font, glyph atlas, colour emoji. *(per-cell `CTLine` shaping with CoreText's own cascade for fallback, slot-grid atlas, colour emoji, bold/italic faces, underline/strike; cross-cell ligatures and combining marks are not shaped — the wire carries one scalar per cell)*
- [x] `CADisplayLink` presentation, ProMotion-adaptive, idle when clean. *(renders on change; the link only coalesces bursts and pauses when clean — see the status note for why it does not also hold the panel at 120 Hz)*
- [x] Attach to `vtermd`; measure the daemon hop against a direct-PTY baseline. **If it costs > 1 ms p99, execute the ADR-0004 fallback** (foreground pane's PTY in-process, background sessions in the daemon). *(measured, within budget — no fallback)*
- [x] `Scripts/bundle.sh`: `.app` assembly, `Info.plist`, entitlements, codesign. *(ad-hoc signature until M9; no entitlements file yet because a terminal cannot be sandboxed and hardened-runtime flags come with the Developer ID)*
- [x] Configuration UI (added 2026-09-05 at the owner's request): a Settings window (⌘,) that edits every `config.toml` and `keymap.toml` key from `09`, driven by the Rust config schema so it cannot drift from the file format. *(`vt-config` schema with defaults, validation, comment-preserving edits, resolved keymaps, themes, hot reload; daemon RPC `config.*`; `vterm config`/`vterm keys`; SwiftUI Settings window generated from the daemon's field metadata — every key gets a control from its kind, range and options, a reset-to-default button, and an orange "applies from Mx" note when the build stores but does not yet honour it; keymap editor with conflicts; theme picker/editor with duplicate-and-edit for user themes. Fonts, theme (following the system appearance), padding, cursor style/blink, keymap and close policy apply live to open panes; edits made outside the app arrive through the `config.changed` broadcast. Verified from offscreen captures of the form and of a grid rendered under a custom config; the vibrancy sidebar does not composite into such captures and was not visually verified.)*

**Exit:** p99 keystroke→glyph < 8.3 ms measured with a typometer-class harness on this Mac; visually indistinguishable from Ghostty at rest.

> **M4 status 2026-09-05 — exit criterion not met as written; measured, not estimated.** The shell runs under the Command Line Tools alone (`mise run app:build`, `app/Scripts/bundle.sh`), attaches to `vtermd`, and renders the login shell. Numbers from this Mac (M-series, built-in 120 Hz ProMotion panel, release builds, `VAMBIANT_TERM_LATENCY_PROBE=200`, 100×30 cells, Menlo 13):
>
> | Stage | p50 | p99 |
> |---|---|---|
> | key event in the view → grid dirty (daemon round trip incl. `login`/zsh echo) | 0.88 ms | 3.2 ms |
> | dirty → frame committed (instance build + encode) | 0.46 ms | 1.3 ms |
> | commit → GPU done | 0.78 ms | — |
> | commit → frame presented (`CAMetalDrawable.presentedTime`) | **20.6 ms** | 26.2 ms |
> | **key → presented, total** | **22.0 ms** | **30.9 ms** |
>
> Everything this codebase controls costs about 1.3 ms; the remaining 20 ms is the window server presenting a windowed `CAMetalLayer`. Presenting a frame every tick to hold the panel at 120 Hz made it worse (32 ms — frames queued), `presentsWithTransaction` was worse still (28 ms), and disabling display sync changed nothing, so the view renders on change only. The harness is in-process (`app/Sources/VambiantTerm/Latency/LatencyProbe.swift`) and stops at the compositor; a camera-based typometer would add up to one refresh on top. The 8.3 ms figure is therefore not reachable in windowed mode on macOS by this design, and it is left as the exit line so the gap stays visible: the next things to try are fullscreen direct-to-display and a comparison run of Ghostty through the same harness method.
>
> **Daemon hop (docs/08 §7, ADR-0004):** `cargo run --release -p vtermd --example daemon_hop -- <sock> 300` — direct PTY `cat` echo p50 25 µs / p99 77 µs; through `vtermd` (IPC in, output delta out) p50 424 µs / p99 873 µs; **added p99 0.80 ms, within the 1 ms budget.** No fallback.
>
> **Visual check:** `VAMBIANT_TERM_SCREENSHOT=<png>` writes the rendered drawable; bold, italic, underline, ANSI/256/RGB colours, backgrounds, CJK wide cells and colour emoji were verified from that capture. "Indistinguishable from Ghostty" was not compared side by side (Ghostty is not installed on this machine; only its source is vendored).
>
> Left open in M4: selection and copy (M5), bracketed paste through a `session.paste` method (M5), IME/dead-key composition via `NSTextInputClient`, cursor styles and blink from config, hollow-cursor focus behaviour across windows, and the configuration UI above.

## M5 — Blocks and the sidebar (3 weeks)

- [x] `vt-blocks`: OSC 133/633 segmentation; heuristic fallback; corrupted-marks detection with a one-time warning. *(A stream scanner in `vt-core` (`osc.rs`) pulls 133/633 marks out of the byte stream — the exit code in `D;<exit>` and the `633;E` command line that no backend exposes — and emits them as `TermEvent::ShellMark` with the absolute scrollback row. `vt-blocks::Segmenter` turns marks into `Block`s, counts out-of-order marks and declares the session corrupted past a threshold (Powerlevel10k/starship). The daemon runs the segmenter per session, persists closed blocks (`vt-store::blocks`), broadcasts `session.block`, serves `session.blocks`, and emits a `blocks_degraded` event when marks are corrupt. `vterm blocks <session>` prints the timeline. Verified end to end against a real shell emitting 133 marks.)*
- [x] Shell integration snippets for zsh/fish/bash with auto-injection; the bash `\[ \]` prompt-width fix; Powerlevel10k/starship detection. *(`vt-shell` ships original MIT snippets — deliberately not derived from Ghostty's GPL scripts — that emit OSC 133 A/B/C/D and OSC 7, self-install on source, stay silent when not interactive, and add to the user's prompt/hooks rather than replacing them. The bash B mark is wrapped in `\[ \]`. Injection is per shell and survives `/usr/bin/login`: zsh via `ZDOTDIR` chaining to the user's `.zshrc`, fish via an `XDG_DATA_DIRS` `vendor_conf.d` entry; bash needs `--rcfile`, so it is integrated only when the daemon launches bash directly and says so otherwise. The daemon writes the files and env per session (`[shell_integration]` gates it). Corrupted-mark detection lives in the segmenter (previous commit). Verified end to end: a real zsh with no marks in the command produced an exit-1 command block purely from the injected integration.)*
- [x] Block rendering: headers, collapse, exit chips, copy/rerun/explain. *(Partly. Scrollback became addressable first: `TerminalCore` gained `viewport()`, `scroll()` and `text_range()` (absolute rows, the numbering `ShellMark` already used); `OutputDelta` carries `top`/`total`; the daemon owns the viewport (`session.scroll`, typing snaps to the live end) and serves `session.text`; `vt_viewer_scroll` and `top`/`total` on `VtGridView` (ABI 2). The app draws block chrome from the daemon's rows — gutter stripe by exit status, hairline above each command line, `exit N` chip, `≈` for guesses, selection tint — and offers copy command / copy output / re-run from the Blocks menu, right-click and ⌘C, with ⌘↑/⌘↓ prompt jumps and ⌘⇧↑/↓ block selection. `blocks_degraded` shows as a persistent label on the pane, not a hidden state. Warp parity (12 §A, 2026-09-06): multi-selection with ⌘/⇧-click and ranges, bookmarks stored by the daemon with a right-edge tick and ⌥↑/⌥↓ jumps, top/bottom-of-block scrolling, re-input (plain and sudo), copy command/output/both, copy as HTML through libghostty's formatter, keyboard block menu, `session.clear`, a find bar over `session.find` (⌘F, regex/case/in-block, ⌘G/⌘⇧G, highlights in the grid, bottom-up start), a sticky command header for a scrolled-off command line, a failed-row tint and `[blocks]` config keys. Background output at an idle prompt becomes a heuristic `background` block (segmenter `on_output`, daemon quiet-time rule, `≈ background` chip). **Warp mode (ADR-0011, 2026-09-06):** `[input] mode = "warp"` is the default: the injected zsh/bash/fish integration hides the shell's prompt and prints one blank row per prompt, the app pins an editor with cwd/branch chips and a hint line at the bottom of the pane, submits on ↩ (⇧↩ newline, ⌃C clears), hands the keyboard to the grid while a command runs (daemon `prompt`/`command_started` events), draws Warp's context line (cwd, `git:(branch)`, duration from the block's C..D timing) into the blank prompt row and sets the command row in bold; `warp-dark` is the default theme and the window adopts the dark appearance. The editor has Warp's basics: ↑/↓ walk `history.search` (distinct command lines across sessions from the block store, prefix-filtered), ghost-text suggestions from the same history accepted with →/⌃F (⌃→ one word), and token colouring (command bold, flags, strings, variables, operators, comments) from a shallow shell tokenizer. `mode = "classic"` keeps the shell's editor. Still missing from docs/06 §4: collapse, duration and cwd in the header, share-as-text beyond the clipboard, the per-block filter panel, text selection, and "explain", which waits for M-AI. Known limitation, recorded in `vt_core::cell::Viewport`: when libghostty prunes old scrollback the absolute numbering shifts and blocks older than the budget drift; they are treated as unmapped, not redrawn wrong.)*
- [ ] Agent event blocks: tool calls, diffs, thinking, assistant markdown.
- [ ] Cost and context meters from the status line feed.
- [x] Sidebar: repo → worktree → session tree with state glyphs. *(Warp's vertical tabs (12 §L), 2026-09-06: `⌘\` toggles a left panel with a search field and rows grouped by repository (worktrees resolve to their main repo through `commondir`; sessions outside a repo sit under "scratch"). Each row: a terminal or agent icon with the docs/06 state glyph as a badge (○ ◐ ⚙ ✋ ✕ ■), the last command as the title, the cwd, the branch, and a `+n -m` diff pill from `git diff --numstat` probed off the main thread at most every three seconds. The window title is the focused pane's cwd. Clicking a row focuses that pane. Not yet: hover ⋮/✕ controls, drag to reorder, tab groups and pins, rows for other windows in the tab group.)*
- [ ] Inbox as a sheet, full keyboard triage.

**Exit:** an agent session reads as a structured timeline, not a wall of text.

> **M5 status 2026-09-06:** the block engine and shell integration are built and tested (`vt-core` OSC scanner, `vt-blocks` segmenter, `vt-store` persistence, daemon `session.block`/`session.blocks`, `vterm blocks`, `vt-shell` snippets + injection wired into the daemon), and the app renders command blocks Warp-style over a daemon-owned scrollback viewport (see the item above). Still to build for the exit criterion: block collapse and header details, agent-event rendering in the app timeline, the cost/context meters, the sidebar tree, and the inbox sheet. A full Warp feature analysis and the parity plan that extends M5–M9 with two new packages (M5.5 input/shell parity, M10 agent composer) live in `11-warp-feature-inventory.md` and `12-warp-parity-plan.md` (2026-09-06; direction accepted the same day in ADR-0011: look like Warp, ship the same features, cloud features as local equivalents).

## M-AI (parallel from M1) — Provider layer (3 weeks of effort, spread)

- [x] `Provider` trait; capability tables including `SamplingSupport`. *(2026-09-06: `vt-ai::provider` — Messages-shaped `Request`/`Content`/`Chunk`, a synchronous `Provider` trait (the daemon runs a request per thread; docs/04's async sketch is the same contract without a runtime), `Assembler` for streams, `SamplingSupport` per model.)*
- [x] Anthropic Messages client: streaming, ~~prompt caching with breakpoint budgeting, `count_tokens`~~. *(Streaming SSE with every documented event kind and tolerance for unknown ones; `DefaultOnly` sampling drops `temperature`/`top_p` per model; caching and `count_tokens` not yet.)*
- [x] ~~OpenAI Responses +~~ Chat clients; generic OpenAI-compatible adapter. *(Chat Completions with the safe subset from docs/04 §2, tool calls mapped by index; Responses API not yet.)*
- [x] Local adapters: Ollama, `llama-server` (Messages ~~+ `/infill`~~), ~~MLX,~~ LM Studio. *(Ollama and LM Studio through the compatible adapter, llama.cpp through the Messages adapter; `/infill` and MLX not yet.)*
- [x] Route table, ~~fallback chains, circuit breakers, cancellation~~. *(`providers.toml` profiles, models, pricing and routes; `config.toml [ai.routes]` wins. Fallbacks, breakers and cancellation not yet.)*
- [x] Cost accounting, ~~budgets with hard stop, `vterm ai spend`~~. *(List-price estimates per request, recorded in the egress log; budgets and `spend` not yet.)*
- [x] Keychain integration; `vterm ai doctor`. *(`security-framework` generic passwords under `com.vambiant.term`, env var per profile first; `vterm ai key set|remove`, `vterm ai doctor`, `vterm ask`. Every outbound text part goes through `vt-redact` and a failure refuses the request; the e2e test proves a key in the prompt never reaches the mock server.)*

- [x] Agent Mode, first slice (ADR-0011 D2, 2026-09-06): the conversation panel in the pane (`AgentPanel`, `AgentConversation`), `⌘↩`/`⌘K`/Agent menu → `ai.ask` off the main thread with the session as context, `⌥E` explains the last failed block, `history` turns on `ai.ask` (redacted, fail-closed, `user`/`assistant` only), footer and header cost/token meters, proposed commands staged into the editor and never run. Streaming through `ai.ask { stream = true }` → `ai.chunk`/`ai.done`/`ai.error` notifications (the daemon runs the request on its own thread; `vterm ask` still uses the plain reply). Verified end to end against a mock OpenAI-compatible server through a fresh daemon (`VAMBIANT_TERM_SCREENSHOT` writes `.agent.png`, captured mid-stream). **Tool loop (2026-09-06):** `agent = true` offers `run_command`; every call is classified, evaluated by `vt-policy` and waits in the inbox (`Agents::ask`, shared with the vendor path) with its verdict; an allowed command is typed at the prompt and its block's output, clipped to the context budget and redacted fail-closed, returns as the tool result; `ai.tool_request`/`ai.tool_result` drive the panel's embedded command blocks. e2e in a real zsh: allow and deny. Not yet: the approval card's Edit (the sheet has it),  under `vt-policy`, approval card, thinking rows, task ticks.

**Exit:** `vterm ask` works against all four provider families; `vterm ai doctor` prints resolved capabilities and flags missing models.

## M-SEC (parallel from M1) — Redaction & policy (3 weeks of effort, spread)

- [ ] `vt-redact`: gitleaks rule import into a `RegexSet`, entropy layer, streaming sliding window, stateful multi-line PEM mode. *(2026-09-06: the rule set (14 credential shapes written from their public formats, in a `RegexSet` plus per-rule regexes), whole-payload PEM, a time budget and panic containment that fail closed, with a golden test per rule. Entropy, the streaming window and streaming PEM are still placeholders.)*
- [ ] Property tests across all chunk boundaries; golden corpus in CI.
- [x] `vt-policy`: POSIX shell parser, classification, the never-auto floor. *(2026-09-06: an own POSIX reader (lists, pipelines, subshells, groups, `if`/`while`/`for`/`case`/functions, redirections and here-docs, quoting, `$VAR`/`${…}`/`$(…)`/backticks/process substitution) — yash-syntax is GPL and conch-parser unmaintained. Every simple command in the tree is classified, through `sudo`/`env`/`xargs`/`timeout`/`sh -c`/literal `eval`; the docs/05 classes with rule ids and tokens in each finding; targets resolved lexically against cwd/worktree/home for the outside-worktree test; protected-branch globs for force pushes; known hosts for egress. The floor is a function of the verdict, not of any file, and `policy.toml` refuses to load with a floor class set to `allow`. Test corpus: the §5.2 table, twenty ways to reach `rm -rf /` through the tree, six quoting evasions, unparseable inputs, a permissive policy that cannot cross the floor.)*
- [x] Per-workspace policy files with intersection semantics. *(`workspace::narrow`: stricter `[safety]` wins, repo `allow` rules are dropped with a warning, `[egress]` mode/providers/never-include intersect, `dry_run` is sticky. Wired the same day: every inbox item whose request carries a `command` gets `verdict` and `floor` from the session's cwd (worktree = nearest `.git`, generic adapter ⇒ floor); `policy.classify` and `vterm classify`; the app's Warp-mode editor classifies every submitted line and shows the confirm sheet (rule, token, worktree note) for `confirm`, refuses `block`, and warns in the hint line for `warn`; Agent Mode's stage buttons carry the class. Not yet: rule evaluation on the approval path (M8), known hosts from the egress log, the audit log.)*
- [~] Egress log and `vterm egress` commands. *(2026-09-06: the redacted body is stored with each record; `vterm egress tail|last`, `ai.payload.last` and ⌥⌘E in the app. Stats, hash and per-rule counts not yet.)*

**Exit:** the property test suite passes; a synthetic secret cannot survive any chunking; the classifier correctly refuses `ls && rm -rf /`.

## M6 — Intelligence in the terminal (3–4 weeks)

- [ ] Inline ghost text with history-first ranking, debounce, cancellation, late-discard.
- [ ] ⌘K palette with staging, explanation and verdict. *(The command palette itself exists since 2026-09-06: ⌘⇧P / `<prefix> :` opens a panel over actions with their chords, every session, the daemon's history and the repo's tracked files, with Warp's `actions:` `sessions:` `history:` `files:` scopes and fuzzy ranking; picking an action performs it, a session focuses it, history or a file lands in the editor. ⌘K's natural-language mode waits for the provider layer.)*
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
