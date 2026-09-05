# 03 — Agent Integration

> Every claim in this document was verified against first-party docs in September 2026. Sources and the unverified list are in `10-research-notes.md`.

## 1. The governing principle

**Never build on a transcript file.**

Both vendors state, in their own documentation, that their on-disk session format is internal and changes between releases:

> "The entry format is internal to Claude Code and changes between versions, so scripts that parse these files directly can break on any release." — Claude Code docs, Sessions

Codex's rollout JSONL location is not even documented first-party. So the rule is:

- **Live observability** comes from hooks, structured stream events and RPC. Supported, versioned surfaces.
- **Our own event log** (`vt-store`) is the durable record. We write it as events arrive.
- **Transcript files** may be read *opportunistically* for backfilling sessions that existed before Vambiant Term was installed. That path is allowed to break; nothing else depends on it.

## 2. Adapter interface

```rust
pub trait AgentAdapter: Send + Sync {
    fn kind(&self) -> AgentKind;                        // Claude | Codex | Generic
    fn capabilities(&self) -> Capabilities;             // what this adapter can actually do

    /// Prepare the environment: install hooks, write config, allocate a callback token.
    fn provision(&self, session: &SessionSpec) -> Result<Provisioning>;

    /// Build the argv/env for spawning.
    fn spawn_command(&self, session: &SessionSpec) -> Command;

    /// Consume an inbound event (hook POST, JSONL line, RPC notification, PTY chunk)
    /// and emit normalised AgentEvents.
    fn ingest(&mut self, input: AdapterInput) -> Vec<AgentEvent>;

    /// Answer a pending decision.
    fn decide(&self, req: &ApprovalId, d: Decision) -> Result<()>;

    fn interrupt(&self) -> Result<()>;
    fn resume(&self, session_id: &str) -> Command;
    fn fork(&self, session_id: &str) -> Result<Command>;
}

pub struct Capabilities {
    pub structured_events: bool,   // real events, not scraped text
    pub permission_control: bool,  // we can answer, not just observe
    pub cost_reporting: bool,
    pub interrupt: bool,
    pub fork: bool,
    pub resume: bool,
}
```

`Capabilities` is surfaced in the UI. A session backed by the generic adapter is visibly labelled "limited observability" — we never let a fragile heuristic masquerade as a real integration.

## 3. Normalised event model

Everything an adapter emits collapses to this. The UI, the inbox, the policy engine and the store all speak only this vocabulary.

```rust
enum AgentEvent {
    SessionStarted { agent_session_id: String, model: Option<String>, cwd: PathBuf },
    StateChanged   { from: AgentState, to: AgentState },
    AssistantText  { text: String, streaming: bool },
    Thinking       { summary: Option<String> },
    ToolCallStart  { id: String, name: String, input: serde_json::Value },
    ToolCallEnd    { id: String, ok: bool, output: Option<String>, duration_ms: u64 },
    FileChanged    { path: PathBuf, diff: Option<UnifiedDiff> },
    ApprovalNeeded (ApprovalRequest),
    ApprovalResolved { id: ApprovalId, decision: Decision, by: DecisionSource },
    Usage          { input: u64, output: u64, cache_read: u64, cache_write: u64,
                     cost_usd: Option<f64>, context_used_pct: Option<f32> },
    RateLimit      { window: String, used_pct: f32, resets_at: Option<DateTime<Utc>> },
    SubagentStart  { id: String, kind: String },
    SubagentStop   { id: String },
    Notification   { title: Option<String>, body: String },
    Error          { kind: ErrorKind, message: String, retrying: bool },
    SessionEnded   { reason: String },
}

enum AgentState { Starting, Idle, Thinking, ToolRunning, AwaitingInput, Stopped, Crashed }
```

`AwaitingInput` is the state the entire product is organised around.

## 4. Claude Code adapter

### 4.1 What we install

On `provision()`, for a session, we write a **session-scoped settings file** and pass it with `--settings`, rather than mutating the user's `~/.claude/settings.json`. Hooks merge across levels rather than replacing, so the user's own hooks keep running — we add, we never take over.

Hook handlers use the **`http` handler type**, POSTing to `http://127.0.0.1:<port>/hook/<session-token>`. That avoids spawning a process per event and gives us the response channel synchronously. `allowedHttpHookUrls` must include our loopback URL — we write it into the session settings file we own. (Verified 2026-09-04: with the key absent the HTTP hook still runs; with a non-matching list it is skipped with `HTTP hook blocked: … does not match any pattern in allowedHttpHookUrls`. An HTTP hook that times out **fails open** — the call proceeds — so the receiver answers inline, always.)

### 4.2 Events we subscribe to

| Hook event | Why we want it |
|---|---|
| `PermissionRequest` | **The** event. Fires the moment a tool needs a decision. This is the inbox's source, not `Notification` |
| `PreToolUse` | Gate *every* tool call including auto-approved ones — needed for the block stream and the policy engine's view |
| `PermissionDenied` | Show denials in the timeline; `hookSpecificOutput.retry: true` lets us offer "retry with a wider grant" |
| `PostToolUse` / `PostToolUseFailure` | Tool results, `duration_ms`, and `updatedToolOutput` if we ever need to rewrite |
| `PostToolBatch` | Batched decisions |
| `Notification` | Agent-initiated messages |
| `Stop` / `StopFailure` / `SubagentStop` | Turn boundaries; `last_assistant_message` is authoritative (the transcript lags) |
| `SessionStart` / `SessionEnd` | Lifecycle, `source` tells us startup vs resume vs fork |
| `UserPromptSubmit` | Our own timeline entry for what was asked |
| `FileChanged` | Diff blocks without polling git |
| `CwdChanged` / `DirectoryAdded` | Keep the pane title and worktree binding correct |
| `SubagentStart` / `TaskCreated` / `TaskCompleted` / `TeammateIdle` | Subagent tree in the sidebar |
| `PreCompact` / `PostCompact` | Explain the context drop in the timeline instead of leaving it mysterious |
| `PreModelSwitch` / `PostModelSwitch` | Cost attribution changes |
| `Elicitation` / `ElicitationResult` | MCP elicitation flows into the same inbox |

Every event carries `session_id`, `transcript_path`, `cwd`, `hook_event_name`. **As verified on 2026-09-04 (v2.1.260):** `prompt_id` and `permission_mode` are present on tool/turn events but absent on `SessionStart`; **`effort` never appears** (it arrives as the `CLAUDE_EFFORT` env var instead); `PermissionRequest` adds `permission_suggestions`. Parse every field beyond the first four as optional. Of the events above, `PermissionDenied`, `TeammateIdle`, `PreCompact`/`PostCompact`, `PostModelSwitch` and `Elicitation`/`ElicitationResult` were not reached in M0 — see `10-research-notes.md` §6.

### 4.3 Answering

- `PreToolUse` → `hookSpecificOutput.permissionDecision` ∈ `allow` | `deny` | `ask` | `defer`, plus `permissionDecisionReason` and optionally `updatedInput` (this is what makes **edit-then-allow** real in the inbox).
- `PermissionRequest` → `hookSpecificOutput.decision.behavior` ∈ `allow` | `deny`, with `updatedInput` inside the `decision` object.
- Top-level `continue: false` + `stopReason` stops the session **after** the current action — verified on 2026-09-04 that a `PreToolUse` hook returning it did not prevent the tool from running. The emergency stop for a pending call is `deny`; `continue:false` is the "and then halt" switch.
- Exit code 2 blocks — and blocks **even if** the JSON said `allow`. Our handler must be careful never to exit 2 accidentally.
- Strings (`additionalContext`, `systemMessage`, stdout) are capped at **10,000 chars**; overflow spills to a file. Keep responses small.

Timeouts: 600 s default for command/http/mcp_tool handlers, but **10 s for `MessageDisplay`**, 30 s for `UserPromptSubmit` and the model-switch events, and `SessionEnd` hooks share a **1.5 s budget**. Our HTTP handler must answer fast or the agent stalls. Design: the handler returns immediately with `defer` when a human decision is needed and the inbox posts the real answer out-of-band — see §4.6.

### 4.4 `terminalSequence` — the direct channel

A hook may return `terminalSequence`, and Claude Code writes that ANSI string through its own terminal path (hooks have no `/dev/tty`). The allowlist is OSC `0`/`1`/`2` (titles), `9` (incl. `9;4` taskbar progress), `99`, `777`, and bare BEL. CSI, OSC 8, OSC 52 and OSC 1337 are rejected.

We use OSC 777 for agent-originated notifications and OSC 9;4 for progress. Note it is **only emitted in interactive sessions** — ignored under `-p` and the SDK. So it is a nice-to-have signal, never the mechanism.

### 4.5 Live session state — the status line

`statusLine` is the richest documented "external app reads live state" surface Claude Code has. Its stdin JSON gives us, per refresh:

`model.id`, `model.display_name`, `cwd`, `workspace.{current_dir, project_dir, git_worktree, repo.{host,owner,name}}`, `cost.{total_cost_usd, total_duration_ms, total_lines_added, total_lines_removed}`, `context_window.{used_percentage, remaining_percentage, current_usage.{input_tokens, output_tokens, cache_creation_input_tokens, cache_read_input_tokens}}`, `rate_limits.{five_hour, seven_day, spend_limit}.{used_percentage, resets_at}`, `prompt_cache.{warm, hit_ratio, ttl, expires_at}`, `session_id`, `session_name`, `prompt_id`, `transcript_path`, `output_style.name`, `agent.name`, `pr.{number,url,review_state}`, `worktree.{name,path,branch}`.

We install a status line command that **emits a normal status line to stdout and POSTs the JSON to our socket**. `subagentStatusLine` gives us a per-subagent token/status feed the same way.

⚠️ Both are disabled or narrowed under enterprise `allowManagedHooksOnly` and `disableAllHooks`. Degraded mode required.

### 4.6 The deferred-decision protocol

Permission prompts in Claude Code **never time out** — a hook that blocks waiting for a human blocks forever, and a hook handler that holds a connection for ten minutes is a resource leak.

Protocol:

1. `PermissionRequest` arrives at our HTTP handler.
2. Policy engine evaluates. If a rule decides it, answer inline in milliseconds.
3. Otherwise: enqueue an `ApprovalRequest`, notify, and return `defer` (`PreToolUse`) — Claude Code then routes to its normal prompt flow, which the SDK host or `--permission-prompt-tool` answers. Verified 2026-09-04: in a bare `-p` session `defer` ends the turn with the call unexecuted and nothing in `permission_denials`; with `--permission-prompts none` the `PermissionRequest` hook fires and then auto-denies, with `host` and no SDK host it does not fire at all. Bare-CLI sessions therefore use `none` and answer from the hook; `defer`/`null` is an SDK-hosted path only.
4. For sessions we spawn through the **Agent SDK** rather than the bare CLI, `canUseTool` receives a `requestId`; returning `null` lets us answer the `control_response` out-of-band from the inbox process. That is the clean path and the reason a future `vtermd` may host SDK sessions directly.
5. ⚠️ Returning `null` in any case where we do *not* subsequently answer hangs the tool call forever. Every deferred request gets a watchdog and a visible "still waiting" state.

Note the documented gotcha: `canUseTool` fires **only** when the flow falls through to a prompt — never for calls auto-approved by `allowedTools`, allow rules, or the permission mode. To see every tool call, use `PreToolUse`. We do both.

### 4.7 Headless and background sessions

Claude Code ships a supervisor surface of its own that we should cooperate with rather than duplicate:

`claude --bg`, `claude agents --json` (active sessions as JSON; `--json --all` includes completed), `claude attach <id>`, `claude logs <id>`, `claude stop <id>`, `claude rm <id>`, `claude respawn`, `claude daemon status`.

Our background task queue (B5.6) schedules and surfaces these rather than reimplementing them.

For headless runs we use: `-p`, `--output-format stream-json`, `--include-partial-messages`, **`--include-hook-events`** (streams every hook lifecycle event into the output stream — the single most useful flag for a supervisor), `--forward-subagent-text`, and `--permission-prompts host|none` (`none` = auto-deny anything that would prompt, correct for unattended runs).

The `system/init` event carries a **`capabilities` array of strings** — use it for feature detection instead of comparing version numbers.

The result object gives us `total_cost_usd`, `usage`, **`modelUsage`** (prefer this for accounting — it covers subagents and compaction), `permission_denials`, `num_turns`, `duration_api_ms`, `ttft_ms`. `total_cost_usd` is a **client-side estimate at list price** and can differ from the actual bill; label it as an estimate in the UI.

### 4.8 Resume semantics — the sharp edges

- `claude --resume <session-id>` now searches the current project first, then every other project on the machine, and refuses when two projects hold the same id.
- `--continue` resumes the most recent conversation **in the current directory** and **skips** `-p`/SDK, background and `/loop` sessions — except `claude -p --continue`, which includes them but still skips background. This asymmetry will bite; we always resume by explicit id.
- `--fork-session` gives a resumed session a new id. Our registry must follow the fork, not lose the parent.
- Transcript path: `~/.claude/projects/<cwd-with-non-alnum-replaced-by-dashes>/<session-id>.jsonl`; names over 200 chars are truncated and hashed. `CLAUDE_CONFIG_DIR` relocates the whole tree — never hardcode `~/.claude`.

## 5. Codex adapter

Codex has a *better* embedding story than Claude Code and we should use it.

### 5.1 `codex app-server` — the primary integration

JSON-RPC 2.0 over stdio, WebSocket, or Unix socket (`codex app-server --listen ws://127.0.0.1:<port>`). It exists explicitly for "a deep integration inside your own product" and handles auth, history, approvals and streamed events. Verified 2026-09-04: the Unix-socket transport is **WebSocket framing over the socket** (HTTP Upgrade handshake), not newline-delimited JSON; `initialize` → `initialized` is mandatory per connection; approvals arrive as the server request `item/commandExecution/requestApproval` answered with `{"result":{"decision":"accept"|"decline"}}`. Raw traffic in `tests/fixtures/codex/app-server/`.

Methods we use: `thread/start`, `thread/resume`, `thread/fork`, `thread/list`, `thread/archive`; `turn/start`, `turn/steer`, `turn/interrupt`; `model/list`; `config/read`, `config/value/write`.

`turn/steer` has no Claude Code equivalent and is worth surfacing prominently in the UI — mid-turn redirection is a genuinely different interaction.

### 5.2 Hooks

Codex now has a hook system that closely mirrors Claude Code's: `hooks.json` or inline `[hooks]` in `config.toml`, gated by `[features] hooks`.

Events: `SessionStart`, `SessionEnd`, `UserPromptSubmit`, `PreToolUse`, `PermissionRequest`, `PostToolUse`, `Stop`, `PreCompact`, `PostCompact`, `SubagentStart`, `SubagentStop`, `Interrupt`. Verified 2026-09-04 (0.153.2): the first four plus `PostToolUse`, `Stop` and `SessionEnd` captured; payloads carry `model` and `turn_id` and no `prompt_id`; every hook needs persisted trust (`[hooks.state]`) and project hooks need project trust; handler types are `command` and `mcp_tool` only — no `http`.

The schemas are convergent enough (`hookSpecificOutput`, `permissionDecision`, exit-2 blocking, same stdin field names) that **one hook-handling abstraction serves both vendors**. Codex's set is a strict subset: no `terminalSequence`, no `MessageDisplay`, no `PostToolBatch`. Timeouts: 600 s default, but `SessionEnd` and `Interrupt` get 1 s (max 3 s).

### 5.3 Non-interactive

`codex exec --json` emits JSONL to stdout: `thread.started`, `turn.started`, `turn.completed`, `turn.failed`, `item.*`, `error`. Item types cover agent messages, reasoning, command executions, file changes, MCP tool calls, web searches and plan updates. Progress goes to **stderr**, the final message to **stdout**.

Useful flags: `-o/--output-last-message`, `--output-schema`, `--ephemeral` (skip persisting rollouts), `--sandbox <read-only|workspace-write|danger-full-access>`, `--ask-for-approval`, `--skip-git-repo-check`. Resume via `codex exec resume --last` or `codex exec resume <SESSION_ID>`.

### 5.4 Config

TOML. Precedence, highest first: CLI flags/`--config` → project `.codex/config.toml` → profile `~/.codex/<profile>.config.toml` → user `~/.codex/config.toml` → system `/etc/codex/config.toml`. `CODEX_HOME` relocates the tree.

Keys that matter to us: `sandbox_mode`, `approval_policy` (`on-request` | `never` — **`untrusted` is rejected by 0.153.2** with "no longer supported", verified 2026-09-04), `[sandbox_workspace_write] writable_roots / network_access`, `mcp_servers`, `notify`, and `[hooks.state."<key>"] trusted_hash / enabled` (hook trust, which can live in our profile) plus `[projects."<path>"] trust_level` (project trust; writing it ourselves avoids Codex mutating the user's `config.toml`).

We write a **profile** rather than touching the user's main config, and select it with `--profile`.

⚠️ `codex mcp-server` is deprecated — do not integrate through it. Use `app-server`.

## 6. Generic adapter (Aider, Gemini CLI, opencode, Cursor CLI, anything else)

Heuristics only, and honest about it.

Signals, in descending reliability:

1. **OSC 133 marks** if the agent's TUI happens to emit them.
2. **Terminal mode transitions** — entering the alternate screen, enabling bracketed paste, requesting cursor position, pushing kitty keyboard flags: strong evidence of an interactive prompt.
3. **Idle detection** — no PTY output for N ms while the process is alive and the cursor sits after a prompt-like glyph run.
4. **Prompt shape matching** — per-agent regex packs for the known question forms ("(y/N)", "Apply this edit?", numbered menus), maintained as data files so they update without a release.
5. **Process introspection** — argv, cwd via `proc_pidinfo`, child processes.

Every generic-adapter session is labelled in the UI. We never auto-answer for a generic adapter, regardless of policy — the policy engine hard-refuses (see `05-security-privacy.md`).

## 7. Prior art we are deliberately compatible with

- **ACP (Agent Client Protocol)** — Zed's open standard. Its client-side terminal capability (`terminal/create`, `terminal/output`, `terminal/wait_for_exit`, `terminal/kill`, `terminal/release`, and terminals embedded in tool calls as `{"type":"terminal", terminalId}`) is very nearly a specification of what our daemon already does. Our internal session API should be shaped so that **speaking ACP is an adapter, not a refactor**. That is the v2 unlock: every ACP agent works without us writing an adapter per vendor.
- **VS Code OSC 633** — we consume it, so anything already instrumented for VS Code gives us blocks for free.
- **Warp** — closest competitor; auto-detects ten agent CLIs and overlays notifications for Claude Code, Codex and OpenCode only, requiring a one-time setup step. The mechanism is undocumented; the *shape* of the product validates ours.

## 8. Degraded modes — the honest matrix

| Condition | What we lose | What the UI says |
|---|---|---|
| `allowManagedHooksOnly` / `allow_managed_hooks_only` | Hook install, status line | "Limited observability — your organisation restricts agent hooks" |
| `disableAllHooks: true` | All hooks | Same, with a link to the setting |
| Agent launched outside Vambiant Term | Provisioning; we can still adopt the PTY and read OSC 133 | "Adopted session — reduced detail" |
| Generic adapter | Structured events, approval control | "Heuristic mode — observation only" |
| `--bare` / `CLAUDE_CODE_SIMPLE=1` | Hooks are skipped entirely (docs say `--bare` "will become the default for `-p` in a future release") | "Bare mode — no hook events" |
| Enterprise-locked OTEL endpoint | Metrics feed | Silent; we never depended on it |

Rule: a degraded session must look different at a glance. The failure mode we refuse to ship is a terminal that confidently shows stale or wrong agent state.
