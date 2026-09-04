# 01 — Feature Catalogue

> The deep think. Everything considered, nothing filtered by politeness.
> Tiers: **M** = must ship in v1 · **S** = should, v1.x · **C** = could, speculative but designed-for · **W** = won't, and why.

Read this as the requirements source of truth. `07-implementation-plan.md` schedules it.

---

## A. Terminal fundamentals

If this layer is not excellent, nothing above it matters. A terminal that is clever but drops frames is a toy.

### A1. Emulation correctness

| # | Feature | Tier | Notes |
|---|---|---|---|
| A1.1 | VT100/VT220/xterm core: cursor movement, SGR, scroll regions, tabs, charsets | M | Delegated to `alacritty_terminal` |
| A1.2 | True colour (24-bit), 256-colour, colour palette OSC 4/10/11/12 | M | |
| A1.3 | Alternate screen buffer, bracketed paste, focus reporting, mouse modes (1000/1002/1003/1006) | M | |
| A1.4 | Scrollback with configurable limit, search, and reflow on resize | M | Reflow is the hard part; alacritty's grid does it |
| A1.5 | **Kitty keyboard protocol** (`CSI > flags u` push / `CSI < u` pop) | M | Agents' TUIs increasingly need disambiguated keys. Push on entry, **pop on exit** or you corrupt the next program |
| A1.6 | OSC 8 hyperlinks | M | Claude Code's status line emits them |
| A1.7 | OSC 52 clipboard **write**; **read defaulted OFF** | M | Read is an exfiltration primitive over ssh. See `05-security-privacy.md` |
| A1.8 | OSC 7 cwd, OSC 9;9, OSC 1337 `CurrentDir`/`SetUserVar` | M | Accept blank/unknown hostnames; percent-decode leniently |
| A1.9 | Kitty graphics protocol + Sixel (decode side) | S | No good standalone Rust decoder crate exists; `libghostty-vt` has a `kitty` feature. Deferred but the cell model must reserve space for image placements from day one |
| A1.10 | Unicode: grapheme cluster → cell mapping, ZWJ emoji, wide/ambiguous width, combining marks | M | Segment before shaping; a ZWJ emoji is one wide cell |
| A1.11 | Bidi | C | Ghostty punts to `wezterm-bidi`; we punt entirely in v1 |
| A1.12 | `terminfo` entry `vambiant-term` + `xterm-256color` compatibility fallback | M | Ship and install both; `TERM_PROGRAM=VambiantTerm`, `TERM_PROGRAM_VERSION` |
| A1.13 | OSC 133 A/B/C/D emission *and* consumption | M | Foundation for blocks. Parse params permissively; ignore unknown keys |
| A1.14 | OSC 633 (VS Code) consumption incl. `E` nonce | S | Free interop with anything already emitting it |
| A1.15 | OSC 777 / OSC 9 notifications | M | Claude Code's `terminalSequence` hook allowlist names Ghostty and Warp for OSC 777 — this is how an agent tells the terminal something happened |

### A2. Rendering & performance

| # | Feature | Tier | Notes |
|---|---|---|---|
| A2.1 | Metal renderer, glyph atlas, damage-driven redraw | M | Redraw only damaged cells; `Term::damage()` drives it |
| A2.2 | CoreText shaping + system font fallback cascade (`CTFontCreateForString`) | M | The reason to keep text on the Swift side. cosmic-text approximates fallback with hardcoded browser lists; CoreText *is* the cascade |
| A2.3 | Ligature support with terminal-safe defaults | S | Ghostty disables `fl`/`fi`/`st` by default to avoid unintended shaping — copy that |
| A2.4 | Colour emoji (`sbix`/`COLR`) via `CTFontDrawGlyphs` | M | |
| A2.5 | ProMotion / adaptive refresh, `CADisplayLink`-driven | M | Present at 120 Hz when active, drop to idle when the grid is static |
| A2.6 | Subpixel/greyscale AA options, gamma-correct blending, thin-strokes toggle | S | The detail that makes a terminal feel "right" |
| A2.7 | Cursor styles (block/bar/underline), blink with a cap, `DECSCUSR` | M | |
| A2.8 | Perf budgets enforced in CI (see `08`) | M | p99 keystroke→glyph < 8.3 ms; `cat` of a large UTF-8 file within 2× of Ghostty |
| A2.9 | Background transparency / blur | S | `NSVisualEffectView`; cheap on macOS, expensive to fake |
| A2.10 | Per-pane GPU budget so a `yes` flood in one pane cannot starve the others | S | Rate-limit damage flush per pane |

### A3. Windows, tabs, panes, sessions

| # | Feature | Tier | Notes |
|---|---|---|---|
| A3.1 | Native tabs and windows, macOS tab bar semantics | M | |
| A3.2 | Splits: horizontal/vertical, arbitrary nesting, drag-to-resize, zoom-to-fullscreen-pane | M | |
| A3.3 | **tmux-style prefix keybinds** for the multiplexer, remappable to ⌘-style | M | Your muscle memory carries over |
| A3.4 | Pane sync (broadcast input to N panes) | S | |
| A3.5 | Layouts: save, name, restore; per-project default layout | S | "open project X" = 4 panes, 2 agents, 1 server, 1 shell |
| A3.6 | Session restore across app restart and reboot | M | Shell panes restore cwd + scrollback; agent panes reattach to the daemon |
| A3.7 | Detach/reattach a pane from the GUI to the daemon and back | M | The tmux property, but for agents specifically |
| A3.8 | Quake-style dropdown window on a global hotkey | C | |
| A3.9 | Pane titles derived from OSC 2 + process introspection + agent state | M | "claude · vambiant-api · ✋ waiting" beats "zsh" |

### A4. Shell integration

| # | Feature | Tier | Notes |
|---|---|---|---|
| A4.1 | Auto-injected init for zsh, fish, bash; manual sourcing documented | M | Ghostty's model: inject on launch, `shell-integration = <shell>\|none` |
| A4.2 | Emit OSC 133 A/B/C/D + OSC 7 from the injected snippet | M | In bash, prompt marks **must** be wrapped in `\[ \]` or line-editing miscounts width |
| A4.3 | Detect a broken/absent integration and degrade honestly | M | Powerlevel10k / starship / oh-my-zsh preexec hooks that print will corrupt marks. Detect, warn once, fall back to heuristics — never silently render wrong blocks |
| A4.4 | cwd inheritance for new tabs/splits, and for restored sessions | M | |
| A4.5 | `jump_to_prompt`, click-to-select-command-output, semantic selection | S | |
| A4.6 | Re-establish integration across `ssh`, `sudo`, `nix-shell`, `docker exec` | S | Ghostty offers opt-in `ssh`/`sudo` wrapping; same approach, opt-in only |

---

## B. The supervisor — what makes it an agent terminal

This is the product. Everything in A is table stakes.

### B1. Agent session model

| # | Feature | Tier | Notes |
|---|---|---|---|
| B1.1 | An **AgentSession** is a first-class object: id, name, agent kind, cwd, repo, branch, worktree, PID, state, cost, token usage, start time, last activity | M | Not "a pane that happens to run claude" |
| B1.2 | States: `starting → idle → thinking → tool_running → **awaiting_input** → stopped/crashed/finished` | M | `awaiting_input` is the state the whole product exists to surface |
| B1.3 | Persistent across window close, app quit, and reboot (daemon-owned PTY) | M | |
| B1.4 | Named sessions; rename; pin; archive | M | `vterm attach api-refactor` beats a uuid |
| B1.5 | Grouped by git repo → worktree → branch in the sidebar | M | |
| B1.6 | Restart-in-place preserving conversation (`claude --resume <id>`, `codex exec resume <id>`) | M | |
| B1.7 | Fork a session (`--fork-session`, `thread/fork`) to explore an alternative | S | Both CLIs support it natively |
| B1.8 | Crash detection + auto-restart with resume, with a backoff and a hard cap | S | |
| B1.9 | Per-session env, model, permission mode overrides | M | |
| B1.10 | Session transcript archive owned by *us*, independent of the agent's own store | M | Both vendors state their transcript formats are internal and change between versions. We record our own structured log from hooks/stream events so history survives their refactors |

### B2. The approval inbox

| # | Feature | Tier | Notes |
|---|---|---|---|
| B2.1 | Global queue of every pending decision across every session | M | One list. Badge count in the dock and menu bar |
| B2.2 | Answer from the inbox without focusing the pane | M | |
| B2.3 | Rich rendering per request type: bash command with syntax highlight + safety verdict, file edit as a diff, MCP tool call as structured args, web fetch with the URL and host reputation | M | A permission prompt you can actually evaluate |
| B2.4 | Native notification (Notification Center) with actionable buttons, respecting Focus modes | M | |
| B2.5 | Keyboard-first triage: `j/k` navigate, `a` allow, `d` deny, `A` allow-always-for-this-rule, `e` edit-then-allow | M | `updatedInput` in the hook response makes edit-then-allow real |
| B2.6 | "Why is this being asked" — show the rule that failed and the exact scope an always-allow would grant | M | Consent must be informed or it is theatre |
| B2.7 | Batched decisions: N identical requests answered once | S | `PostToolBatch` exists on the Claude side |
| B2.8 | Inbox history: every decision, who/what made it, timestamp, reversible where possible | M | This is the audit log that makes autonomy safe |
| B2.9 | Snooze / defer a request | S | Claude Code's `PreToolUse` now has a `defer` decision value |

### B3. Structured output blocks

| # | Feature | Tier | Notes |
|---|---|---|---|
| B3.1 | Command blocks from OSC 133: command text, exit code, duration, cwd, collapsible output | M | Works for everything, agent or not |
| B3.2 | Copy / rerun / share / "explain" per block | M | |
| B3.3 | Agent event blocks: tool call (collapsible, with args), file diff, thinking (collapsed by default), assistant message as markdown | M | Fed by hook events + `--include-hook-events` / `codex exec --json`, never by scraping |
| B3.4 | Syntax-highlighted, word-level file diffs with a "reveal in editor" action | M | |
| B3.5 | Live cost + context-window meter per session | M | Claude: status line JSON gives `cost.*`, `context_window.*`, `rate_limits.*`. Codex: token counts from `turn.completed` |
| B3.6 | Timeline scrubber: jump to any point in a session's event history | S | |
| B3.7 | Filter the stream: hide thinking, show only diffs, only errors | S | |
| B3.8 | Blocks degrade to plain text when structure is unavailable | M | Non-negotiable. No structure is better than wrong structure |

### B4. Worktree & parallel orchestration

| # | Feature | Tier | Notes |
|---|---|---|---|
| B4.1 | One-command "new agent task" → creates a worktree, branch, session, and pane | M | `vterm task new "fix flaky auth test" --repo . --agent claude` |
| B4.2 | Worktree registry: which agent owns which tree, since when | M | Prevents the collision that makes parallel agents miserable |
| B4.3 | Run N agents on the same task in parallel trees, diff their results side by side | S | The "race three agents, keep the best" workflow |
| B4.4 | Merge/promote a winning worktree; delete the losers with confirmation | S | |
| B4.5 | Automatic cleanup of stale worktrees (agent dead > N days, branch merged) | S | |
| B4.6 | Guard: refuse to start a second write-capable agent in a tree that already has one | M | |
| B4.7 | Per-worktree resource labels (ports, containers) to avoid dev-server collisions | C | |
| B4.8 | `WorktreeCreate` / `WorktreeRemove` hooks integration | S | Claude Code fires these; we can own worktree creation for it |

### B5. Autonomy — built, shipped off

Every item defaults to disabled. Enabling any of them writes an entry to the audit log.

| # | Feature | Tier | Notes |
|---|---|---|---|
| B5.1 | Policy engine: rules over (agent, repo, tool, argument pattern, safety class) → allow/deny/ask | M | The engine is architecture, not a feature toggle |
| B5.2 | Auto-approve read-only tools in a named repo | S | off by default |
| B5.3 | Never-auto rules that no policy can override: `rm -rf`, force push, credential reads, network egress to new hosts | M | Hard-coded floor, not configurable away |
| B5.4 | Watchdog: idle-with-no-question detection → notify | S | |
| B5.5 | Loop detection: same tool + same args N times → flag and offer to interrupt | S | Genuinely useful; agents do get stuck |
| B5.6 | Background task queue: queued prompts run headless when a slot frees | S | `claude --bg` + `claude agents --json` already exist; we schedule and surface them |
| B5.7 | Every autonomous action is logged with the rule that caused it and is one keystroke from undo/revert | M | |
| B5.8 | "Dry run autonomy": show what *would* have been auto-approved for a week before enabling | C | The honest way to earn trust in a policy set |

### B6. Cross-agent parity

| # | Feature | Tier | Notes |
|---|---|---|---|
| B6.1 | Adapter interface so agents are pluggable: Claude Code, Codex, and a generic PTY-heuristic adapter | M | |
| B6.2 | Claude Code adapter: hooks (`PermissionRequest`, `PreToolUse`, `Notification`, `Stop`, `SubagentStart/Stop`, `FileChanged`, `CwdChanged`), status line JSON, `--include-hook-events`, `claude agents --json` | M | See `03-agent-integration.md` |
| B6.3 | Codex adapter: `hooks.json`, `codex exec --json` JSONL, and **`codex app-server`** JSON-RPC (thread/start, turn/steer, turn/interrupt) | M | Codex's app-server is the better embedding API of the two — use it |
| B6.4 | Generic adapter: PTY heuristics for Aider, Gemini CLI, opencode, Cursor CLI | S | Fragile by nature; clearly labelled as best-effort in the UI |
| B6.5 | Speak **ACP** (Agent Client Protocol) as a client so any ACP agent works | C | ACP already defines `terminal/create`, `terminal/output`, `terminal/wait_for_exit` — it is nearly a spec for our core. Strong candidate for v2 |
| B6.6 | Degraded mode when enterprise policy blocks hook installation (`allowManagedHooksOnly`) | M | Both vendors ship this lock. Detect it and fall back to heuristics with an honest banner |

---

## C. Intelligence — the AI layer

### C1. Inline & inline-adjacent

| # | Feature | Tier | Notes |
|---|---|---|---|
| C1.1 | Ghost-text next-command suggestion, accept with `→`, word-accept with `⌥→` | M | Context: history, cwd, git state, last exit code, last stderr tail |
| C1.2 | History-first ranking, model only when history is a poor match | M | The cheapest suggestion is the one you don't ask a model for |
| C1.3 | Debounce + cancellation + single in-flight request per pane | M | Latency budget: 120 ms p50 to first token or don't render |
| C1.4 | ⌘K natural language → command, staged in the prompt, **never auto-executed** | M | Show the explanation and the safety verdict alongside |
| C1.5 | ⌘K accepts a selection or the last block as context | M | |
| C1.6 | On non-zero exit, an unobtrusive affordance: "⌥E explain" | M | Not a popup. Never interrupt |
| C1.7 | Explain-and-fix returns a patch to the command line, not prose | M | |
| C1.8 | "Explain this output" on any selected region | S | |
| C1.9 | Natural-language scrollback search ("the command that failed with the cert error") | S | Local embeddings over the block index |
| C1.10 | Command palette with fuzzy search over commands, sessions, worktrees, settings | M | |

### C2. Safety classifier

| # | Feature | Tier | Notes |
|---|---|---|---|
| C2.1 | Static classifier: destructive, irreversible, credential-reading, network-egress, privilege-escalating | M | Rules first, model second. A regex that catches `rm -rf /` beats a model that usually does |
| C2.2 | Applies to human-typed commands **and** agent-issued ones — same engine | M | This is why it belongs in the terminal rather than in each agent |
| C2.3 | Warn / confirm / block per class, per workspace | M | |
| C2.4 | Shell-aware parsing, not substring matching | M | VS Code's own docs admit their approach is "best effort" and evadable by quote concatenation. Parse with a real shell grammar and treat unparseable input as unsafe |
| C2.5 | Explain the verdict: which rule, which token | M | |

### C3. Provider layer

| # | Feature | Tier | Notes |
|---|---|---|---|
| C3.1 | Named provider profiles, hot-swappable per pane and per feature | M | |
| C3.2 | Anthropic Messages API native client (streaming, prompt caching, `count_tokens`) | M | |
| C3.3 | OpenAI native (Responses + Chat Completions) | M | |
| C3.4 | Generic OpenAI-compatible: any `base_url` + key — OpenRouter, Groq, vLLM, LM Studio, LiteLLM, your own gateway | M | |
| C3.5 | Local: Ollama, `llama-server`, MLX server, LM Studio | M | `llama.cpp` now speaks the **Anthropic Messages API natively** — one well-built Messages client covers cloud Claude *and* local llama.cpp |
| C3.6 | Per-feature routing: cheap/local for autosuggest, strong for ⌘K and explain | M | |
| C3.7 | Fallback chains with health checks and circuit breaking | S | |
| C3.8 | Cost accounting per feature, per session, per day, with a budget cap that hard-stops | M | |
| C3.9 | Response cache keyed on a normalised context hash | S | |
| C3.10 | Model discovery via `/v1/models` and `GET /v1/models` | S | |
| C3.11 | Never forward unsupported sampling params | M | Anthropic returns **HTTP 400** for non-default `temperature`/`top_p` on recent Opus models. Capability table per provider, not blind pass-through |

### C4. Context construction

| # | Feature | Tier | Notes |
|---|---|---|---|
| C4.1 | Deterministic, inspectable context builder — you can always see the exact payload | M | |
| C4.2 | Sources: cwd, git status/branch/recent commits, last N blocks, shell type, OS, installed tool versions, `--help` of the command being typed | M | |
| C4.3 | Token budget per feature with graceful truncation | M | |
| C4.4 | Project-local context file (`.vambiant-term/context.md`) appended to prompts | S | |
| C4.5 | Redaction runs **before** anything is added to context, not after | M | |

---

## D. Cross-cutting

### D1. Security & privacy

| # | Feature | Tier | Notes |
|---|---|---|---|
| D1.1 | Streaming redaction pipeline with sliding-window matching across chunk boundaries | M | A secret *will* straddle two PTY reads. No existing Rust crate handles this — it is our code |
| D1.2 | gitleaks-derived regex ruleset in a single `RegexSet`, plus entropy scoring for unprefixed secrets | M | |
| D1.3 | Redact the input region (OSC 133 B→C) too — typed `export API_KEY=…` is the most common leak | M | |
| D1.4 | Preview exactly what will be sent, on demand and on first use per workspace | M | |
| D1.5 | API keys in Keychain, never in the config file; optional biometric gating on read | M | |
| D1.6 | Per-workspace policy file `.vambiant-term/policy.toml`: `none` / `redacted` / `full` | M | Work repos locked down, scratch repos open |
| D1.7 | Egress log: every outbound request, host, byte count, feature, timestamp | M | |
| D1.8 | OSC 52 read off, `allowWindowOps`-class sequences gated, paste-safety validation (bracketed paste + newline warning) | M | |
| D1.9 | Hardened runtime, ad-hoc signing for local, notarization path documented | S | |
| D1.10 | No telemetry. Ever. Crash logs stay local unless you attach one to an issue yourself | M | |

### D2. Configuration & extensibility

| # | Feature | Tier | Notes |
|---|---|---|---|
| D2.1 | TOML config, hot-reloaded, schema-validated, errors shown in an overlay rather than silently ignored | M | |
| D2.2 | JSON Schema published for editor completion | S | |
| D2.3 | Theme compatibility with existing formats (Ghostty / iTerm2 / Alacritty / base16) | M | |
| D2.4 | `vterm config check` / `vterm config explain <key>` | S | |
| D2.5 | Loopback HTTP + WebSocket API, token-authenticated, documented | M | Lets your menu bar app, Raycast, scripts and a phone-over-Tailscale answer approvals |
| D2.6 | Scripting hooks: run a command on session state transitions | S | |
| D2.7 | MCP client so the terminal itself can expose sessions as tools to an agent | C | Reflexive and slightly dangerous; design it, ship it later |

### D3. The CLI

| # | Feature | Tier | Notes |
|---|---|---|---|
| D3.1 | `vterm ls` / `attach` / `new` / `kill` / `rename` / `logs` | M | Works inside Ghostty from day one — this is what makes M2 useful before the GUI exists |
| D3.2 | `vterm inbox` — list and answer pending approvals from any terminal | M | |
| D3.3 | `vterm task new/list/promote` — worktree orchestration | S | |
| D3.4 | `vterm ask "<question>"` — one-shot provider query with terminal context | S | |
| D3.5 | JSON output on every command (`--json`) for scripting | M | |
| D3.6 | Shell completions for zsh/fish/bash | S | |

---

## E. Won't build (and why)

| Feature | Why not |
|---|---|
| Cross-platform in v1 | Every macOS-native affordance we want (CoreText cascade, Keychain, Notification Center, native tabs) is the thing that makes it good. Portability stays in the Rust core; the shell does not pretend |
| Cloud sync of sessions | Adds an account, a server, a threat model, and a subscription. The loopback API + Tailscale covers the real need |
| Our own coding agent | The agents are good and improving faster than we could follow. Supervising them is the durable position |
| Embedded editor / LSP | That is Zed's job and Zed does it well |
| Bidi and complex-script shaping in v1 | Ghostty needs a dedicated crate for it. Scope it out honestly rather than shipping it broken |
| Windows / ConPTY | Not in the plan. The Rust core keeps `alacritty_terminal`'s Windows support compiled but untested |
| Reading `~/.claude/projects/*.jsonl` as a primary data source | **Both vendors explicitly document their transcript formats as internal and version-unstable.** We may read them opportunistically for backfill, never as the mechanism |
| Blocks-as-a-social-feature (share links, team workflows) | Single-user tool. No |

---

## F. Ordering rationale

The dependency spine is: **PTY + VTE correctness → shell integration → blocks → supervisor daemon → agent adapters → inbox → provider layer → intelligence → autonomy.**

Two things can start early and in parallel because they touch nothing else: the **provider layer** (pure Rust, testable headless) and the **redaction pipeline** (pure function over byte streams). `07-implementation-plan.md` exploits that.
