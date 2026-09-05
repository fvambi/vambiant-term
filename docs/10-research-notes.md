# 10 — Research Notes

> Everything below was verified against primary sources in **September 2026**. Versions and APIs move; re-verify before relying on any of it. Items marked ⚠️ are **unverified** and must be confirmed in M0.

## 1. Rust terminal core

| Fact | Detail |
|---|---|
| `alacritty_terminal` | **0.26.0** (2026-04-06), master at `0.26.1-dev`. Licence **Apache-2.0 only** (not the usual dual licence) |
| API | Re-exports `Grid`, `Term`, and the whole `vte` crate. Modules: `event, event_loop, grid, index, selection, sync, term, thread, tty, vi_mode` |
| PTY | `tty` module still bundled and actively developed. Unix uses `rustix-openpty 0.2` + `rustix 1.0` + `signal-hook 0.4`; `polling 3.8` for the event loop |
| macOS details worth copying | Passes `-q` to `login` when `~/.hushlogin` exists; `drain_on_exit` handling |
| Stability | **No stability guarantee.** No stated semver or MSRV policy for library consumers. Recent minors shipped breaking changes: 0.25.0 replaced `Options::hold` with `Options::drain_on_exit`; 0.26.0 changed `ChildEvent::Exited`/`Event::ChildExit` from `i32` to `ExitStatus`. Expect a breaking bump roughly every 6 months |
| Toolchain | Workspace is `edition = "2024"`, `rust-version = "1.85.0"` |
| `vte` | **0.15.0** (2025-02-02), Apache-2.0 OR MIT. `alacritty_terminal` pins it with `default-features = false, features = ["std", "ansi"]` |
| ✅ Verified | **As verified on 2026-09-04, `alacritty_terminal` 0.26.0** (source read + PTY spike in `spikes/term-core-spike`): `Term::damage(&mut self) -> TermDamage<'_>`; `enum TermDamage<'a> { Full, Partial(TermDamageIterator<'a>) }`; the iterator yields `LineDamageBounds { line: usize, left: usize, right: usize }` (viewport-relative, inclusive columns, already offset by `display_offset`). Damage is cumulative until **`Term::reset_damage(&mut self)`**, which the caller must invoke after consuming — nothing resets it implicitly. `damage()` also folds the previous and current cursor cells into the partial set, and any scroll while `display_offset != 0`, insert mode, or resize produces `Full`. Construction: `Term::new<D: Dimensions>(Config, &D, T: EventListener)` with `term::test::TermSize::new(cols, lines)` as the stock `Dimensions`; `Term::resize<S: Dimensions>(&mut self, S)` reflows and returns `Full` damage. `Config { scrolling_history, default_cursor_style, vi_mode_cursor_style, semantic_escape_chars, kitty_keyboard, osc52 }`. Parsing is `vte::ansi::Processor<StdSyncHandler>::advance(&mut term, &[u8])`. `Event::ChildExit(ExitStatus)` confirmed (the 0.26 breaking change). PTY: `tty::new(&Options { shell, working_directory, drain_on_exit, env }, WindowSize { num_lines, num_cols, cell_width, cell_height }, window_id)`; on macOS with `shell: None` it execs `/usr/bin/login -flp <user> /bin/zsh -fc <exec>`, adding `-q` when `~/.hushlogin` exists (`tty/unix.rs:170–190`). Gap found: `Pty::child()` returns `&Child` only, so exit detection needs `waitpid` or the `EventLoop` — that is the surface `vt-pty` owns. Full dependency list: `base64 0.22, bitflags 2.4, home 0.5, libc, log, parking_lot 0.12, polling 3.8, regex-automata 0.4, unicode-width 0.2, vte 0.15` plus unix `rustix 1.0, rustix-openpty 0.2, signal-hook 0.4` — as documented. Reflow verified: 80→40→80 columns round-trips five 67-char lines byte-identically |

**`wezterm-term` is NOT on crates.io.** Neither are `wezterm-cell`, `wezterm-surface`, `wezterm-escape-parser`. Using it means a git pin plus ~15 workspace-internal crates on edition 2018. `termwiz` 0.23.3 is published but is a **client-side TUI toolkit**, not a terminal state model — the actual state machine lives in the unpublished `wezterm-term`. A third-party fork exists (`tattoy-wezterm-term`). Not recommended.

**As verified on 2026-09-04, `libghostty-vt` 0.2.1 + `libghostty-vt-sys` 0.2.1** (MIT OR Apache-2.0, `rust-version = 1.90`, repo `uzaaft/libghostty-rs`): the sys crate's `build.rs` **git-clones Ghostty at pinned commit `a887df42` (1.3.2-dev) into `OUT_DIR` and runs `zig build -Demit-lib-vt=true`** — so a plain `cargo build` needs `git`, network access and **zig 0.15.2 exactly** (Ghostty's `src/build/zig.zig` rejects 0.16.0: *"does not meet the required build version of v0.15.2"*). Escape hatches: `GHOSTTY_SOURCE_DIR`, `GHOSTTY_ZIG_SYSTEM_DIR`, and a `pkg-config` feature that links a pre-built `libghostty-vt-static`. 🔴 **On this Mac the vendored build fails**: zig 0.15.2 cannot link against the macOS 26.5 Command Line Tools SDK (dated 2026-08-31) at all — even `zig cc hello.c` fails with `undefined symbol: _printf`. Cause: `MacOSX26.5.sdk/usr/lib/libSystem.tbd` lists targets `[x86_64-macos, x86_64-maccatalyst, arm64e-macos, arm64e-maccatalyst]` — **plain `arm64-macos` is gone** — while `MacOSX15.4.sdk` (still shipped in the same CLT) lists it. zig 0.16.0 copes; 0.15.2 does not, and it locates the SDK through `xcrun --sdk macosx --show-sdk-path`, which ignores `SDKROOT`. Workaround used for the spike: build out-of-band with `--sysroot …/MacOSX15.4.sdk` (see `scripts/bench/build-ghostty.sh`). Rust API shape: `Terminal::new(Options { cols, rows, max_scrollback })`, `vt_write(&[u8])` (no return, malformed input is contained), `resize(cols, rows, cell_w_px, cell_h_px)`, read-side via `RenderState::update(&term) -> Snapshot` with `Snapshot::dirty() ∈ {Clean, Partial, Full}`, `RowIterator`/`CellIterator` lending iterators (`row.dirty()`, `cell.raw_cell()?.codepoint()`, `graphemes()`), `title()`, `pwd()`, `scrollback_rows()`. Every type is `!Send + !Sync` (documented; the C API may use thread-local state). **No PTY layer** — the spike borrows `alacritty_terminal::tty`. Default feature `kitty-graphics` on. Fixture-grade record of the original research below.

**`libghostty-vt` 0.2.1** (2026-07-18, MIT/Apache-2.0) is the genuine alternative and must be spiked in M0. Safe Rust API over Ghostty's terminal core, optional `kitty` graphics feature. The upstream C header self-describes: *"This is an incomplete, work-in-progress API… Breaking changes are expected."* `ghostling` is the reference consumer (a minimal terminal in one C file). Strategic note: because libghostty is a C library, **Swift could call it directly with zero FFI tooling** — if the Rust requirement ever softens, that collapses the whole FFI problem.

**Conformance testing**: `esctest2` (Thomas Dickey, Python, `--expected-terminal=xterm`; cannot test colour-setting sequences because per-cell colour readback isn't feasible), `vttest` (interactive/visual). Ghostty's own compliance priority: standards → xterm → other terminals. Even Ghostty has open vttest-failure discussions — 100% is not a bar anyone clears.

## 2. Rust toolchain (Sept 2026)

- Latest stable **1.98.1** (2026-09-03) — a point release fixing a **vtable miscompilation** that emitted null pointers into trait-object vtables. **Pin ≥ 1.98.1.**
- ✅ As verified on 2026-09-04: `rustc 1.98.1 (48a229cea 2026-09-01)`, `cargo 1.98.1`, `clippy 0.1.98`, `rustfmt 1.9.0` installed through mise (`rust = "1.98.1"` in `mise.toml`, mirrored by `rust-toolchain.toml`). The workspace (edition 2024, resolver 3, `#[unsafe(no_mangle)]`) builds and passes `clippy -D warnings` with `pedantic` on.
- ⚠️ **macOS 26.5 SDK / arm64 note** (see §1): the 2026-08-31 Command Line Tools ship an SDK whose `libSystem.tbd` drops the `arm64-macos` target. Rust/`cc`/Apple `ld` are unaffected; **zig ≤ 0.15.2 is broken** against it. Anything that builds Ghostty from source on this machine must pin the 15.4 SDK until Ghostty moves to zig 0.16. Full Xcode is **not yet installed** (`xcode-select -p` → CommandLineTools); M0 needs none of it, M4 (Metal, signing) does.
- Tooling versions pinned in `mise.toml` and verified installed: hyperfine 1.20.0, cargo-deny 0.20.2, cbindgen 0.29.4, zig 0.15.2 (spike only).
- 1.98.0 (2026-08-20) added FFI-relevant lints: warn-by-default `c_void_returns`, deny-by-default `invalid_runtime_symbol_definitions`, warn-by-default `suspicious_runtime_symbol_definitions`, and stricter `repr(transparent)` validation.
- Current edition is **2024**. ⚠️ No Rust 2027 edition has been announced — only an internals discussion thread. Do not plan around it.
- **Edition 2024 gotcha**: `#[no_mangle]` must be written `#[unsafe(no_mangle)]`. Hard error otherwise.
- `extern "C-unwind"` (stable since 1.81) is correct wherever a Swift `fatalError` could unwind through a Rust frame; plain `extern "C"` aborts.

## 3. Rust ↔ Swift interop

| Tool | Version | Date | Licence |
|---|---|---|---|
| `swift-bridge` | 0.1.59 | 2026-01-06 | Apache-2.0/MIT |
| `uniffi` | 0.32.0 | 2026-06-30 | MPL-2.0 |
| `cxx` | 1.0.199 | 2026-08-08 | MIT/Apache-2.0 |
| `cbindgen` | 0.29.4 | 2026-06-09 | MPL-2.0 |

- **swift-bridge** requires Swift 6.0+. Supports primitives, `String`/`&str`, `Vec<T>` (as `RustVec<T>`), `Option`, `Result`, tuples, opaque types, and **bidirectional async** (Swift async fns returning `Result` must use typed `throws(E)`). Not implemented: `SwiftArray<T>`, slices, `Arc<T>`, fixed-size arrays, Swift→Rust `Box<dyn FnOnce>` callbacks. ⚠️ Maintenance is intermittent — 15-month gap between 0.1.57 (2024-08) and 0.1.58 (2025-12); ~92 open issues.
- **UniFFI** describes Swift bindings as production-quality but Swift 6 support is **explicitly partial**: "it is known that async code will not conform" to `Sendable`. It also serializes across the boundary and is MPL-2.0. Wrong tool for 120 Hz damage streaming.
- **Swift 6.3** (2026-03-24) added the **`@c` attribute** — a Swift function or enum annotated `@c` appears in the generated C header. This is the closest thing to a new official interop story and it cleans up the Swift→Rust callback direction (no more `@_cdecl` hacks). There is still **no official Swift↔Rust story**.
- Swift C++ interop remains "actively evolving"; C++→Swift excludes generic classes, actors, async and throwing functions. Rust→C++→Swift is two bridges. Rejected.
- `AppKitWindowHandle` is `!Send`/`!Sync` because NSView is main-thread-only. Keep PTY/parser on their own threads, publish damage into a double-buffered region, marshal only a dirty signal to the main thread.

## 4. Text rendering

- **Alacritty** (0.17.0, 2026-04-06) is still OpenGL; glyphs via its own `crossfont` 0.9.0, which uses **CoreText on macOS**. No full ligature shaping.
- **Ghostty** (1.3.1, 2026-03-13) on macOS: **Metal + CoreText**. Maintains both a CoreText and a HarfBuzz shaper; the HarfBuzz path disables `fl`/`fi`/`st` by default. Font fallback breaks runs whenever a different font index is required for a codepoint; grapheme clusters segmented manually. Open issues (#1645, #3128) show the **CoreText path is not at parity with HarfBuzz** — evidence that CoreText-only shaping is harder than it looks.
- **WezTerm** default front-end reverted to OpenGL; `WebGpu` is opt-in.
- `cosmic-text` 0.19.0 migrated shaping from `rustybuzz` to HarfRust; its font fallback uses **hardcoded lists borrowed from Chromium and Firefox** rather than the system cascade. `swash` 0.2.9 (0.2.8 was yanked). `fontdue` 0.9.3 is dormant and does no shaping at all.
- **Conclusion**: CoreText + Metal on the Swift side. `CTFontCreateForString` gives the real system fallback cascade — the part cosmic-text approximates. Colour emoji via `CTFontDrawGlyphs`. Rust never touches fonts.

## 5. wgpu / winit

- `wgpu` **30.0.1** (2026-08-22); roughly quarterly breaking majors (28.0.0 Dec 2025, 29.0.0 Mar 2026, 30.0.0 Jul 2026).
- `winit` stable is **0.30.13**; 0.31 has been in beta ~10 months. **If Swift owns the NSView, drop winit entirely.**
- wgpu **can** render into an externally-owned `CAMetalLayer` via `SurfaceTargetUnsafe::CoreAnimationLayer(*mut c_void)`. Trap: it is `#[cfg(metal)]`-gated so it does **not appear on docs.rs** (which builds on Linux) — don't conclude it was removed.
- ⚠️ Unverified whether wgpu picks up `drawableSize`/`contentsScale` changes automatically; assume you must reconfigure on resize.
- **Counterpoint we accepted**: if Swift already owns the layer and we are drawing a glyph atlas plus coloured quads, Metal directly from Swift is simpler than a 29k-LOC dependency with quarterly breaking releases. Hence ADR-0003.

## 6. Claude Code integration

Docs moved: `docs.claude.com/en/docs/claude-code/*` now redirects to **`code.claude.com/docs/en/*`**. Every page is available as raw markdown by appending `.md`. Index at `code.claude.com/docs/llms.txt`. Verified against **v2.1.260**.

**Hook events (32, not the 9 from 2025):** `SessionStart`, `Setup`, `SessionEnd`; `UserPromptSubmit`, `UserPromptExpansion`, `Stop`, `StopFailure`; `PreToolUse`, `PermissionRequest`, `PermissionDenied`, `PostToolUse`, `PostToolUseFailure`, `PostToolBatch`; `SubagentStart`, `SubagentStop`, `TaskCreated`, `TaskCompleted`, `TeammateIdle`; `InstructionsLoaded`, `ConfigChange`, `CwdChanged`, `DirectoryAdded`, `FileChanged`, `WorktreeCreate`, `WorktreeRemove`; `Notification`, `MessageDisplay`; `PreCompact`, `PostCompact`; `PreModelSwitch`, `PostModelSwitch`; `Elicitation`, `ElicitationResult`.

**`PermissionRequest` is the event for a permission UI** — not `Notification`.

Common stdin fields: `session_id`, `prompt_id`, `transcript_path`, `cwd`, `permission_mode`, `effort`, `hook_event_name`, plus `agent_id`/`agent_type` in subagents. `UserPromptSubmit` uses `prompt` (not `user_input`). `transcript_path` "is written asynchronously and may lag the in-memory conversation" — use `last_assistant_message` on `Stop`.

Return values: top-level `continue`, `stopReason`, `systemMessage`, `terminalSequence`; `suppressOutput` is **accepted but has no effect**. `PreToolUse` → `hookSpecificOutput.permissionDecision` ∈ `allow|deny|ask|**defer**` + `updatedInput`. `PermissionRequest` → `hookSpecificOutput.decision.behavior` ∈ `allow|deny`. Exit code 2 blocks **even if the JSON said allow**. Strings capped at 10,000 chars.

`terminalSequence` allowlist: OSC 0/1/2 (titles), 9 (incl. 9;4 progress; iTerm2/ConEmu/WT/WezTerm), 99 (Kitty), 777 (urxvt, **Ghostty**, **Warp**), bare BEL. CSI, OSC 8, OSC 52, OSC 1337 rejected. **Only emitted in interactive sessions.**

Handler types: `command`, `http` (POST, same JSON), `mcp_tool`, `prompt`, `agent`. Timeouts 600 s default; 30 s for `UserPromptSubmit`/model switches; **10 s for `MessageDisplay`**; `SessionEnd` hooks share a **1.5 s** budget. Matchers: `*`/empty = all; simple charset = exact or `|`/`,` list; anything else is an **unanchored JS regex** (`Edit.*` also matches `NotebookEdit`).

Settings precedence: subagent > skill > local > project > plugin > managed > user, but **hooks merge rather than replace** — everything runs. `disableAllHooks` and enterprise `allowManagedHooksOnly` (which also narrows `statusLine`, `fileSuggestion`, `subagentStatusLine`) are the degraded-mode triggers.

**Sessions**: `~/.claude/projects/<project>/<session-id>.jsonl`, project name = cwd with non-alphanumerics replaced by `-`, truncated+hashed over 200 chars. Docs state verbatim: *"The entry format is internal to Claude Code and changes between versions, so scripts that parse these files directly can break on any release."* `CLAUDE_CONFIG_DIR` relocates the tree. `cleanupPeriodDays` default 30.

⚠️ Empirically observed on v2.1.260 (**not a contract**): line `type` values `user`, `assistant`, `attachment`, `queue-operation`, `last-prompt`, `atis-latch`; records carry `uuid`, `parentUuid`, `sessionId`, `promptId`, `timestamp`, `cwd`, `gitBranch`, `version`, `isSidechain`, `message`. Subagent transcripts at `<session-id>/subagents/agent-<id>.jsonl`.

**Resume**: `--resume <id>` searches the current project first, then every other project (v2.1.223+), refusing on ambiguity. `--continue` resumes the most recent conversation in the cwd and **skips** `-p`/SDK, background and `/loop` sessions — except `claude -p --continue`, which includes those but still skips background. `--fork-session` assigns a new id. `--name`/`/rename` allow resume by name.

**Headless flags**: `-p`, `--output-format text|json|stream-json`, `--input-format stream-json`, `--include-partial-messages`, **`--include-hook-events`**, `--forward-subagent-text`, `--replay-user-messages`, `--permission-mode {default|manual|acceptEdits|auto|plan|dontAsk|bypassPermissions}`, **`--permission-prompts host|none`** (v2.1.259+), `--mcp-config`, `--strict-mcp-config`, `--json-schema`, `--max-budget-usd`, `--fallback-model`, `--session-id`, `--agents`, `--settings`, `--setting-sources`, `--add-dir`, `--bare` (skips hooks/LSP/CLAUDE.md; docs say it "will become the default for `-p` in a future release"), `--restricted`, `--safe-mode`.

**Background sessions**: `claude --bg`, `claude agents --json` (`--json --all` includes completed), `claude attach|logs|stop|rm <id>`, `claude respawn`, `claude daemon status`. Cooperate with this rather than duplicating it.

**Stream events**: `system/init` carries a **`capabilities` array of strings** for feature detection — use it instead of version comparison. Also `system/plugin_install`, `hook_started`/`hook_progress`/`hook_response`, `system/api_retry`, `permission_denied`. Subagent messages carry `parent_tool_use_id`.

**Result object**: `subtype` ∈ `success|error_max_turns|error_during_execution|error_max_budget_usd|error_max_structured_output_retries`; fields include `total_cost_usd`, `usage`, **`modelUsage`** (prefer for accounting — covers subagents and compaction), `permission_denials`, `num_turns`, `duration_api_ms`, `ttft_ms`, `structured_output`. **`total_cost_usd` is a client-side estimate at list price** and can differ from the bill.

**Agent SDK**: TS `@anthropic-ai/claude-agent-sdk` 0.3.260; Python `claude-agent-sdk` 0.2.152. `canUseTool(toolName, input, {signal, suggestions, toolUseID, requestId, ...})` → `{behavior:"allow"|...}`. **Critical**: it fires *only* when the flow falls through to a prompt — never for auto-approved calls. To gate everything, use `PreToolUse`. Returning `null` (v2.1.199+) lets another process answer the `control_response` out-of-band; returning `null` without a follow-up **hangs forever — permission prompts never time out**. `reinitialize()` redelivers pending requests, so make the callback idempotent per `requestId`.

**Status line**: the richest external-state surface. stdin JSON includes `model.*`, `workspace.*`, `cost.*`, `context_window.*` (incl. `used_percentage` and cache token breakdown), `rate_limits.{five_hour,seven_day,spend_limit}`, `prompt_cache.*`, `session_id`, `session_name`, `transcript_path`, `output_style.name`, `agent.name`, `pr.*`, `worktree.*`. Output renders ANSI colours and OSC 8 links. **`subagentStatusLine`** gives a per-subagent row feed with token counts. Both disabled/narrowed under `allowManagedHooksOnly`.

**Output styles**: Default, Proactive, Concise (v2.1.237+), Explanatory, Learning. `/output-style` was **deprecated in v2.1.73 and removed in v2.1.91** — do not call it.

**OTEL**: `CLAUDE_CODE_ENABLE_TELEMETRY=1` plus standard `OTEL_*` vars (`OTEL_EXPORTER_OTLP_PROTOCOL` has **no default and must be set**). Metrics include `claude_code.cost.usage`, `.token.usage`, `.active_time.total`; 26 event types. Correlation via `prompt.id`. ⚠️ **Claude Code strips `OTEL_*` from every subprocess it spawns, including hooks**, and managed settings can lock the destination. Do not build on it.

## 7. Codex integration

⚠️ Not installed in the research container; all from docs and registries. Docs moved: `developers.openai.com/codex/*` → **`learn.chatgpt.com/docs/*`**.

- npm `@openai/codex` latest **0.153.2** (2026-09-03); alpha 0.154.0-alpha.3.
- **Config**: TOML. Precedence: CLI/`--config` → project `.codex/config.toml` (trusted projects only) → profile `~/.codex/<profile>.config.toml` → user `~/.codex/config.toml` → system `/etc/codex/config.toml`. `CODEX_HOME` relocates.
- `sandbox_mode` ∈ `read-only|workspace-write|danger-full-access` (+ Windows `elevated`/`unelevated`). `approval_policy` ∈ `untrusted|on-request|never` or a `granular` table. `[sandbox_workspace_write] writable_roots, network_access` (default false).
- **Codex now has hooks**: `hooks.json` or `[hooks]` in config, gated by `[features] hooks`. Events: `SessionStart`, `SessionEnd`, `UserPromptSubmit`, `PreToolUse`, `PermissionRequest`, `PostToolUse`, `Stop`, `PreCompact`, `PostCompact`, `SubagentStart`, `SubagentStop`, `Interrupt`. Same `hookSpecificOutput`/`permissionDecision`/exit-2 shape as Claude Code — **one abstraction serves both**. No `terminalSequence`, no `MessageDisplay`, no `PostToolBatch`. `SessionEnd`/`Interrupt` timeouts are 1 s (max 3 s). Managed hooks via `requirements.toml` + `allow_managed_hooks_only`.
- **`codex exec --json`** emits JSONL: `thread.started`, `turn.started`, `turn.completed`, `turn.failed`, `item.*`, `error`. Progress → stderr, final message → stdout. Flags: `-o/--output-last-message`, `--output-schema`, `--ephemeral`, `--sandbox`, `--ask-for-approval`, `--skip-git-repo-check`. Resume: `codex exec resume --last|<SESSION_ID>`.
- **`codex app-server`** is the real embedding API: JSON-RPC 2.0 over stdio, WebSocket, or Unix socket (`--listen ws://127.0.0.1:4500`). Methods: `thread/start|resume|fork|list|archive`, `turn/start|steer|interrupt`, `model/list`, `config/read|value/write`, `command/exec`, `process/spawn`, `fs/readFile|writeFile|watch`, `mcpServer/tool/call|oauth/login`. **This is a better embedding surface than Claude Code's** — `turn/steer` in particular has no Claude equivalent.
- `codex mcp-server` (tools `codex`, `codex-reply`) is **deprecated**. Do not use.
- ⚠️ **Unverified**: rollout storage at `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` — third-party source only; the first-party page 404'd. Do not depend on it.

## 8. Prior art

- **Warp**: Agent Mode vs Terminal mode (`⌘↩` to switch; `⌘I` or leading `!` forces shell). **Universal Agent Support** auto-detects ten agent CLIs — Claude Code, Codex, OpenCode, Cursor CLI, Gemini CLI, Amp, Auggie, Copilot CLI, Droid, Pi — with an "agent toolbelt": rich input editor, **notifications for Claude Code / Codex / OpenCode only, requiring a one-time setup step**, code review comments, vertical tabs with metadata, Remote Control. ⚠️ The detection and notification mechanism is **not publicly documented**.
- **Ghostty**: shell integration for bash/elvish/fish/nushell/zsh, auto-injected; prompt marks, cwd inheritance, `jump_to_prompt`, click-to-select-output, optional ssh/sudo wrapping. **No AI/agent features, deliberately.** Integration is lost when you exec a sub-shell.
- **Superlogical** — announced **2026-07-31** by Mitchell Hashimoto (with Jack Parks, Alasdair Monk, Hector Simpson; backed by Notable Capital and Amplify): a terminal multiplexer with durable, long-lived sessions spanning multiple terminals, resumable across web and native, **explicitly framed around workflows mixing humans and AI agents**. Ghostty stays nonprofit-owned with an unchanged roadmap. This is the most direct competitive signal available and it validates the thesis.
- **Zed / ACP**: external agents run as separate processes over the **Agent Client Protocol** (JSON-RPC, stdio; HTTP/WS for remote is WIP). ACP Registry lists Claude, Codex, OpenCode, Copilot, Cursor, Pi, Poolside, Gemini CLI. **ACP's terminal capability is nearly a spec for our daemon**: `terminal/create` (with `outputByteLimit`), `terminal/output` (with `truncated` + `exitStatus`), `terminal/wait_for_exit`, `terminal/kill`, `terminal/release`; terminals embed in tool calls as `{"type":"terminal", terminalId}` and the client renders live output that persists after release. `@zed-industries/claude-code-acp` is the Claude adapter.
- **VS Code**: OSC 633 (`A`/`B`/`C`/`D [;exit]`/`E ; commandline [; nonce]`/`P ; key=value`). Extension API: `onDidChangeTerminalShellIntegration`, `onDidStartTerminalShellExecution`, `onDidEndTerminalShellExecution`. Agent approvals via `chat.tools.terminal.autoApprove` (regex by wrapping in `/…/`, `matchCommandLine`), with docs that explicitly warn it is "**best effort**" and evadable by quote concatenation. ⚠️ Member-level API signatures not extracted verbatim.

**Gap we are targeting**: nobody offers a cross-agent, terminal-native supervision layer with a uniform permission UI. Warp is closest, proprietary and undocumented. ACP is the only open standard and it is editor-hosted.

## 9. Provider APIs

**Anthropic** — current model IDs: `claude-fable-5-1` (1M ctx, $10/$50), `claude-opus-5` (1M, $5/$25), `claude-sonnet-5` (1M, $2/$10), `claude-haiku-4-5-20251001` (200K, $1/$5).

- 🔴 **`temperature`/`top_p`/`top_k` are deprecated for Opus 4.7+ — non-default values return HTTP 400.** This is why `ProviderCaps` exists.
- SSE order: `message_start` → (`content_block_start` → `content_block_delta`* → `content_block_stop`)* → `message_delta`+ → `message_stop`, with `ping` interspersed. Deltas: `text_delta`, `input_json_delta` (**partial JSON — accumulate, don't parse per event**), `thinking_delta`, `signature_delta`. `message_delta` usage is cumulative. Tolerate unknown event types.
- `stop_reason` ∈ `end_turn|max_tokens|stop_sequence|tool_use|pause_turn|refusal|model_context_window_exceeded`.
- Prompt caching: `cache_control` ephemeral 5 min (1.25× write) or `ttl:"1h"` (2× write); reads 0.1×. **Max 4 breakpoints.** Minimum cacheable length is model-dependent — 512 (Fable 5.1/Opus 5), 1024 (Sonnet 5/4.6/4.5, Opus 4.8), 2048 (Opus 4.7, Haiku 3.5), **4096 (Haiku 4.5)**. Automatic caching via a top-level `cache_control` consumes one of the 4 slots.
- `POST /v1/messages/count_tokens` → `{"input_tokens": N}`. ⚠️ Rate limits and billing for this endpoint are not stated.
- **Fast mode** (`speed:"fast"` + beta header `fast-mode-2026-02-01`, Opus 5 / 4.8 only): up to 2.5× output tokens/sec, **explicitly does not improve TTFT**, premium pricing, **invalidates the prompt cache** on switch (separate prefix pools), separate rate limits. Wrong lever for a terminal.
- `output_config.effort` ∈ `low|medium|high|xhigh|max`; `thinking: {type:"disabled"|"adaptive"}`.
- OpenAI-compatible endpoint exists at `api.anthropic.com/v1/` but Anthropic says it is **"not considered a long-term or production-ready solution"**; it ignores prompt caching, `response_format`, `logprobs`, `seed`, penalties, `reasoning_effort`, `metadata`, tool `strict`.
- ⚠️ **Haiku 4.5 retirement "not sooner than 2026-10-15" with no announced successor.** Re-check before shipping.

**OpenAI** — Chat Completions is **not deprecated** but Responses is recommended for new projects. Assistants API shut down 2026-08-26. Models: `gpt-6-astra` (1.05M, $10/$50), `gpt-5.6-sol` ($4/$20), `gpt-5.6-terra` ($2/$12), `gpt-5.6-luna` ($0.20/$1.20 — the natural cheap cloud tier), `gpt-5.3-codex` ($1.75/$14). Responses streams typed events with no `[DONE]`; Chat streams `choices[].delta` with `[DONE]`.

**Compatible-endpoint matrix** (verified): `/v1/chat/completions` and `/v1/models` are universal across Ollama, llama.cpp, LM Studio, vLLM, OpenRouter, Groq. `/v1/responses` is now on Ollama (0.13.3+), llama.cpp, LM Studio and vLLM. Avoid `logprobs` (absent on Ollama, unsupported on Groq), `logit_bias` (Groq), `n>1` (Groq requires 1), `seed`, strict `response_format`, `stream_options.include_usage`. Groq silently rewrites `temperature: 0` → `1e-8`.

**Local**: Ollama native `/api/generate` (with `suffix` → **FIM**), `/api/chat`, `/api/embed`; `format` for JSON-schema structured output; `keep_alive` default **5m**, set `-1` to pin resident; durations in nanoseconds. `llama-server` has `/infill`, GBNF/JSON-schema grammars, `--cache-reuse`, `--spec-draft-model` — and 🔴 **since 2026-01-19 also implements `POST /v1/messages` and `/v1/messages/count_tokens` with correct Anthropic SSE, tool blocks, vision and thinking.** MLX `mlx_lm.server` on :8080 with `draft_model` support, but its docs warn it "is not recommended for production as it only implements basic security checks." LM Studio on :1234 plus a native REST v0 exposing tokens/sec and **TTFT**.

**Small models (Sept 2026)**: current sweet spot is the **Qwen3.5 dense family** — `Qwen3.5-0.8B/2B/4B/9B`, each with a `-Base` variant (use Base for completion). `Qwen3-Coder-Next` is 80B total / 3B active MoE, 262k ctx. **Gemma 4** `E2B-it`/`E4B-it` with official QAT q4_0 GGUF are strong on-device candidates. 🔴 **Llama and Phi are not current choices** — Meta has published nothing since before 2025-11 and Microsoft's recent releases are not Phi language models. ⚠️ FIM tokens are **not documented** for Qwen3.5-Base or Qwen3-Coder-Next; `Qwen2.5-Coder-1.5B` remains the fallback because its FIM tokens *are* documented. Test empirically.

## 10. Shell integration sequences

- **OSC 133**: `A` prompt start, `B` command start (terminal records the column across the *logical* line so reflow is safe), `C` command executed, `D [;exit]` finished. ⚠️ The canonical freedesktop spec is robots-blocked; only Contour's `click_events=1` and `cmdline_url=` params were confirmable, though Ghostty demonstrably emits `cl=line`. **Parse params permissively, ignore unknown keys.**
- **OSC 7**: `ESC ] 7 ; file://HOST/percent-encoded-path ESC \`. 🔴 **Not in the xterm spec** — grepping the 3,840-line ctlseqs shows OSC 7 and OSC 8 are both absent. It is a VTE/iTerm2 convention. Hostnames are inconsistent in the wild (empty, `file:///path`, unknown hosts). **Accept blank/unknown hostnames, percent-decode leniently.**
- **OSC 9;9**: ConEmu cwd, Windows-style path, not a URL.
- **iTerm2 OSC 1337**: `SetMark`, `CurrentDir=`, `RemoteHost=`, `ShellIntegrationVersion=`, `SetUserVar=` (base64), `CopyToClipboard`/`EndCopy`, `File=`.
- **VS Code OSC 633**: adds `E ; commandline [; nonce]` — the nonce is the only spoofing defence in this family, since command text otherwise comes from untrusted program output. Escaping inside `E` uses `\xAB` for `;`, `\` and bytes ≤ 0x20.
- 🔴 Two real pitfalls: in bash, prompt marks **must** be wrapped in `\[…\]` or line editing miscounts width; and Ghostty's own source warns its zsh hook "will work incorrectly in the presence of a preexec hook that prints" — expect breakage with Powerlevel10k, starship and oh-my-zsh, and **detect it rather than assuming marks are well-formed**. ⚠️ The snippets in `01`/`07` are composed from the Ghostty/kitty/VS Code scripts, not copied verbatim from one normative source — test them.
- **kitty keyboard protocol**: push `CSI > flags u`, pop `CSI < number u`, set `CSI = flags ; mode u`, query `CSI ? u`. Flags: 1 disambiguate, 2 report event types, 4 alternate keys, 8 all keys as escapes, 16 associated text. Encoding `CSI code:alternates ; modifiers:event-type ; text u`; modifiers are **1 + bitfield** (shift 1, alt 2, ctrl 4, super 8, hyper 16, meta 32, capslock 64, numlock 128); event type 1 press, 2 repeat, 3 release. **Always pop on exit** or the next program's input is corrupted.
- **OSC 52**: `OSC 52 ; Pc ; Pd ST`, gated by `allowWindowOps`. `Pd = ?` makes the terminal **reply with the selection** — 🔴 supported by only iTerm2, VS Code and Cursor precisely because it is an exfiltration primitive over ssh. **Default read off.**
- **OSC 8** hyperlinks: Alacritty 0.11+, foot 1.7+, kitty 0.19+, iTerm2 3.1+, WezTerm, Windows Terminal, Ghostty, VTE 0.50+; tmux 3.4+ and Zellij 0.21+ pass through.

## 11. Secret detection

- **gitleaks** (MIT, Go): TOML rules with `regex`, `entropy` (Shannon, ~2.0–4.5), `keywords` pre-filter, `secretGroup`, layered allowlists with stopwords. Default config ~3,200 lines. Maintainers say the format is "primarily designed for Gitleaks rather than as a standardized format" — but the regexes are MIT and Go's RE2 ≈ Rust's `regex`, so **vendoring the patterns into a `RegexSet` is the pragmatic path**.
- 🔴 **There is no well-maintained, widely-adopted Rust crate that does gitleaks-grade secret detection.** `secrecy` 0.10.3 (2024) wraps values you already hold, it does not detect. `ripsecrets` 0.1.11 has the right *algorithm* (prefixed regex + a randomness test against `token`/`secret`/`password`-ish identifiers, P(random) < 1e-4) but is binary-first with 11.6K downloads. `redact-core` 0.8.2 is PII-focused, pre-1.0, and does not use gitleaks rules. `secretscan` 0.2.3 has 3K downloads. **Build it.**
- `zeroize` 1.9.0 (2026-06-12) is actively maintained — use it directly for buffer wiping.
- **Nothing off the shelf handles sliding-window matching across streaming chunk boundaries.** That is our code and it is the part that actually matters for a terminal.

## 12. Keychain

- `security-framework` 3.7.0 (2026-02-20): `PasswordOptions::use_protected_keychain()` (✅ modern data protection keychain), `set_access_control` / `set_access_control_options` (✅ `USER_PRESENCE`, `BIOMETRY_ANY`, `BIOMETRY_CURRENT_SET`, `DEVICE_PASSCODE`, `WATCH`, `OR`, `AND`, …), `set_access_group`, `set_access_synchronized`.
- 🔴 **Synchronized and non-synchronized items live in completely different stores.** Items are identified by service + account **plus** the sync flag. `None` on delete removes from both; `None` on get returns from either with no way to tell which; `None` on set updates both but tries the non-synced store first. Be explicit.
- `keyring` 4.2.0 (2026-08-29) is an architectural break: `keyring-core` 1.0.0 plus pluggable stores. macOS lives in `apple-native-keyring-store` 1.0.2 with two features: `keychain` (file-based, for **non-code-signed** CLI tools) and `protected` (Apple's Protected Data store, for **code-signed** apps; iCloud sync; biometric items cannot sync). Repos moved to `open-source-cooperative`, not `hwchen/keyring-rs`. ⚠️ The "Protected Data store" ↔ `kSecUseDataProtectionKeychain` mapping is inferred, not documented.

## 13. Performance references

- **vtebench**'s own README: *"This benchmark is not sufficient to get a general understanding of the performance of a terminal emulator. It lacks support for critical factors like frame rate or latency."* It measures PTY read speed only.
- **Ghostty's methodology** (adopt it): generate input files separately, reuse identical files across revisions, time with **hyperfine**, warm up, take **medians**, **never run benchmarks in parallel**.
- Published Ghostty figures on an M3 Max: SIMD ASCII 7.3× over scalar, UTF-8→UTF-32 16.6×, codepoint width 2.8–5×, CSI parsing 1.4–2×. `cat` of a Japanese text file: **Ghostty 73 ms · kitty 0.32.1 392 ms · iTerm2 470 ms**.
- ⚠️ **Typometer** latency numbers (Alacritty 6.9 ms, kitty 23.8 ms / 10.7 ms tuned, WezTerm 26.1 ms) are from **March 2024 on Linux/Xorg**, exclude Ghostty and iTerm2, and use software capture that omits display-pipeline latency. **Establish our own baseline on this Mac.**
- On macOS you are bounded by refresh anyway: 8.3 ms at 120 Hz ProMotion, 16.7 ms at 60 Hz. Sub-frame input handling matters more than shaving microseconds off parsing.
- 🔴 None of these measure **keystroke → AI suggestion rendered**, which is the metric this product lives or dies on. It needs its own harness.

---

## Open items for M0

1. `Term::damage()` / `TermDamage` API shapes in `alacritty_terminal` 0.26.
2. `libghostty-vt` 0.2.1 benchmark vs `alacritty_terminal` on the same corpus; kitty-graphics coverage.
3. Every Claude Code hook event name and payload, captured to fixtures against the installed binary.
4. Claude Code status line stdin JSON, captured to a fixture.
5. Codex `app-server` JSON-RPC traffic, captured to a fixture; confirm the Unix socket transport.
6. Codex rollout storage path (first-party confirmation).
7. FIM token support for Qwen3.5-Base / Qwen3-Coder-Next — test, don't assume.
8. Local TTFT on this Mac for the candidate models, warm and cold.
9. Haiku 4.5 successor / retirement date.
10. Whether wgpu handles `drawableSize` changes on an externally-owned layer — **moot if ADR-0003 holds and we use Metal directly**.
11. Warp's actual agent-notification mechanism, if it ever becomes documented.
12. Live gitleaks TOML: re-verify the GitHub PAT prefix list.
