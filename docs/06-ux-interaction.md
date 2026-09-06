# 06 — UX & Interaction Design

## 1. Design principles

1. **The terminal is the terminal.** Chrome earns its pixels or it goes away. In a plain shell pane with no agent, Vambiant Term should be indistinguishable from Ghostty.
2. **Never interrupt.** No modal ever steals focus from a running command. Everything that wants attention becomes a badge, an inline affordance, or a notification you chose to enable.
3. **Never lie about state.** A degraded session looks degraded. A heuristic guess is labelled a guess. Stale is shown as stale.
4. **Keyboard first, always.** Every action has a binding. The mouse is an accelerant, never a requirement.
5. **Suggestions are staged, never executed.** The model proposes; you press Enter.

## 2. Window anatomy

```
┌─ Vambiant Term ─────────────────────────────────────────────── ⌘⇧A: 3 ──┐
│ ┌─ Sidebar (⌘\) ──┐┌─ Pane grid ──────────────────────────────────────┐ │
│ │ ▾ vambiant-api  ││                                                  │ │
│ │   ● claude  ✋   ││  $ cargo test                                    │ │
│ │   ○ claude       ││  ┌──────────────────────────────────────────┐    │ │
│ │   ▸ wt/auth-fix  ││  │ ✓ 214 passed          1.8s     exit 0     │    │ │
│ │ ▾ vambiant-web  ││  └──────────────────────────────────────────┘    │ │
│ │   ● codex  ⚙    ││  $ ▏git comm                                     │ │
│ │ ▾ scratch       ││        ██it -m "fix: handle empty payload"       │ │
│ │   ○ zsh          ││                                     ghost text  │ │
│ └─────────────────┘└──────────────────────────────────────────────────┘ │
│ ⚡ claude·api waiting: Bash(rm -rf ./dist)   [a]llow [d]eny [e]dit  ⌘⇧A │
└──────────────────────────────────────────────────────────────────────────┘
```

- **Sidebar** — repos → worktrees → sessions. State glyph per session: `○` idle, `◐` thinking, `⚙` tool running, `✋` awaiting input, `✕` crashed. Collapsible, hidden by default in a single-pane window.
- **Status bar** — one line. Shows the highest-priority pending approval, or nothing. This is the "which pane is blocked" answer without opening anything.
- **Inbox** (`⌘⇧A`) — a sheet, not a window. Opens over the grid, closes on decision or Escape.

## 3. The approval inbox

The most important screen in the product.

```
┌ Approvals (3) ───────────────────────────────────────────────── ⎋ close ┐
│                                                                          │
│ ▸ claude · vambiant-api · wt/auth-fix                       12s ago      │
│   Bash                                                                   │
│   ┌────────────────────────────────────────────────────────────────┐    │
│   │ rm -rf ./dist && npm run build                                  │    │
│   └────────────────────────────────────────────────────────────────┘    │
│   ⚠ destructive — `rm -rf` targets ./dist inside the worktree            │
│   Asked because: no rule matches Bash(rm *) in this repo                 │
│   Always-allow would grant: Bash(rm -rf ./dist) in vambiant-api only     │
│                                                                          │
│   [a] allow   [d] deny   [e] edit & allow   [A] always allow   [s] snooze│
│                                                                          │
│ ▸ codex · vambiant-web                                       1m ago      │
│   Edit src/api/client.ts                                                 │
│   ┌ +12 −4 ──────────────────────────────────────────────────────┐      │
│   │  - const res = await fetch(url)                               │      │
│   │  + const res = await fetch(url, { signal: ctrl.signal })      │      │
│   └───────────────────────────────────────────────────────────────┘      │
└──────────────────────────────────────────────────────────────────────────┘
```

Rules for this screen:

- **Every request shows its scope.** "Always allow" must state exactly what it would grant, in the same words the policy file uses. Uninformed consent is worse than no consent.
- **Every request shows why it was asked.** Which rule failed to match.
- **The safety verdict is inline**, with the specific token that triggered it.
- **Edit-then-allow** is a first-class action, backed by `updatedInput` in the hook response.
- `j`/`k` move, `a`/`d`/`e`/`A`/`s` act, `Enter` opens the source pane, `⎋` closes.
- A request that has been deferred and is still waiting shows a "still waiting — 4m" counter. Permission prompts never time out on the agent side; ours must never look resolved when it is not.

> **As built (sessions across restarts, 2026-09-06):** at launch every session still running in the daemon comes back as a tab, oldest first; with none, a fresh shell opens. `⌘⇧T` (`tab.reopen`) reattaches the newest running session no pane shows. Quitting shows, once (suppressible), that N sessions keep running in vtermd and that `vterm ls` lists them; the sessions are never killed by quitting.
>
> **As built (app inbox, 2026-09-06):** the app takes `inbox.list` at start and `inbox.changed` after; every pane whose session has a pending request shows an **approval card** above its input area (who asks to run what, waiting time, the command or `Edit path (+n −m)`, the verdict line with rule and token, the floor reason, "asked because…", Allow / Deny / Inbox ⌘⇧A — buttons, never a bare ↩), and the sidebar row shows `✋ n`. `⌘⇧A` (`inbox.open`, also `<prefix> a` and Agent → Approvals…) opens the **sheet**: pending requests oldest first, `j`/`k`, `a` allow, `d` deny, `e` edit & allow through the command field (`updated_input`; hidden for Codex, whose approvals are accept/decline), `⎋` closes. "Asked because" reads "autonomy is off: every <tool> request is asked" until M8 puts rule evaluation on this path; always-allow and snooze are not drawn until they exist. A request whose hold expired says so and its buttons are disabled. Verified through a fake Claude posting a `PermissionRequest` under an isolated daemon (`VAMBIANT_TERM_SCREENSHOT_AGENT`).

## 4. Blocks

A block is one command (from OSC 133) or one agent event. Rendering:

- **Collapsed by default** when output exceeds N lines; the header stays visible.
- Header: exit code chip, duration, cwd if it differs from the pane's, and a kebab menu (copy command / copy output / rerun / explain / share as text).
- **Thinking blocks collapsed**, tool calls collapsed with a one-line summary, diffs expanded.
- Failed blocks get a subtle left border and an `⌥E explain` affordance in the gutter — not a popup, not a banner.
- Selection is semantic: clicking a block's header selects the whole block; `⌘⇧↑` selects the previous block.
  > **As built (links, 2026-09-06):** the URL or file path under the pointer is underlined with a pointing hand; `⌘`-click opens a URL in the browser and a file in `[editor] program` (`code -g path:line:col`, `zed path:line`, `idea --line N`, `vim +N`, else the bare path), or in the default app when no editor is set. `path:line:col` forms are read; relative paths resolve against the pane's cwd; a missing file says so in the hint line.
  > **As built (text selection, 2026-09-06):** clicking anywhere but the gutter or a header row starts a mouse text selection: drag for a stream, double-click for Warp's smart select (URL, e-mail, path, address, number, else the word), triple-click for the line, `⌥`-drag for a rectangle, `⇧`-click to extend. Rows are absolute, so the selection survives scrolling; the text comes from the daemon's `session.text`, so it can span the scrollback. `⌘C` copies it (falling back to the selected block's output), `[terminal] copy_on_select` copies on release. Clicking a header clears it.
- When shell integration is unavailable or broken, blocks degrade to heuristic segmentation with a small "≈" marker in the gutter, and the tooltip says why.
- Output that arrives at an idle prompt from a background job gets a grey gutter and a "≈ background" chip: it is a guess (see `10 §10`), never a command block.
- **Warp mode** (ADR-0011, default): the shell's prompt is a blank row the app fills with a dim context line (`~/code/app  git:(main)  (0.027s)`); the command line below it is bold; the editor lives at the bottom of the pane with cwd and branch chips and a hint (`⌘↩ for new agent · ⇧↩ newline`). While a command runs the hint says so and keys go to the command.

## 5. Inline suggestions

- Ghost text renders in the dim foreground colour after the cursor. `→` accepts all, `⌥→` accepts one word, any other key dismisses.
- History match beats model completion; a model suggestion only renders if it arrives inside the budget (p50 < 120 ms). **A late suggestion is discarded, not rendered.** Text appearing under your fingers after you have started typing is worse than no suggestion.
- One in-flight request per pane. Every keystroke cancels the previous.
- Never suggest inside a running program's input (we know from OSC 133 whether we are at a prompt).
- A suggestion classified above `benign` renders with a coloured underline; accepting it still requires Enter, and the safety confirm applies.
- **Unknown-command underline (as built 2026-09-06):** the first word of the line gets a dashed red underline when it is neither on PATH (`path.executables`, refreshed each minute), a builtin or keyword, a path, nor an assignment. Aliases and shell functions are not known to the app yet, so they underline as well — a guess, not a verdict.
- **Corrections (as built 2026-09-06):** after a failed command the daemon's `correct.suggest` (12 §B8 rules: typos over PATH and subcommands, `sudo`, `chmod +x`, `mkdir -p`, `rm -r`, `cd` fuzzy, git's upstream hint, "Did you mean") puts the corrected line in the empty editor as ghost text, with the reason in the hint line; `→`/`⌃F` accepts it into the editor, typing or `⎋` drops it, and ↩ runs it through the safety confirm like anything else.

> **As built (safety confirm, 2026-09-06):** in Warp mode every submitted line goes through `policy.classify` (docs/05 §5) before it reaches the shell. `confirm` opens a sheet titled with the class ("Run destructive command?") showing the line and one row per finding — rule, token, what it does, "(outside the worktree)" or "(protected branch)" — with Run and Cancel; Cancel puts the line back in the editor. `block` refuses with the same explanation and keeps the line. `warn` runs and shows the verdict in the hint line. If the classifier does not answer, the sheet says so and asks anyway. The coloured underline while typing is not built yet; the inbox card shows the verdict and the floor reason on every request that carries a command (§3), and Agent Mode's stage buttons show the class next to the command.

## 6. ⌘K — natural language to command

```
┌ ⌘K ──────────────────────────────────────────────────────────────────┐
│ find every file over 100mb changed in the last week                   │
├───────────────────────────────────────────────────────────────────────┤
│ find . -type f -size +100M -mtime -7 -print                           │
│                                                                        │
│ Lists files larger than 100 MB modified in the last 7 days, from the   │
│ current directory downward. Read-only.                                 │
│ ✓ benign                                          claude-sonnet · 0.9s │
│                                                                        │
│ ⏎ stage in prompt   ⌘⏎ stage and run   ⌥⏎ explain more   ⎋ cancel      │
└───────────────────────────────────────────────────────────────────────┘
```

- Staging into the prompt is the default. `⌘⏎` runs, and only if the verdict is `benign`; anything else forces the confirm path.
- Context sent is inspectable via `⌥⌘E` before or after. Selection, or the last block, can be attached with `⌘⇧K`.

> **As built (Agent Mode, first slice, 2026-09-06):** not a sheet but Warp's conversation panel between the grid and the input area (ADR-0011 D2). `⌘↩` in the Warp-mode editor, `⌘K` (`ai.ask`) and the Agent menu send the editor's text to the daemon's `ai.ask` with this session as context; `⌥E` (`ai.explain_last_failure`) asks about the last failed block. The panel shows the question, a `Thinking for Ns · route ask` row while the call is out, then the answer with fenced blocks in the terminal font, a footer (`model · tokens · ≈$ · redactions · seconds`, plus `cut off at max tokens` when the provider stopped there) and running totals in the header. Every command the answer proposes (fenced shell blocks, `$ ` lines) gets a **Stage** button that puts it in the editor; there is no run button and `⌘⏎` does not run — the user presses ↩. Follow-up questions carry the earlier answered turns (`history`), each redacted like the prompt. A refusal (missing key, redaction failure, route `none`) is shown verbatim in the panel in red; `⎋` hides the panel and keeps the conversation. The answer streams: `ai.ask` with `stream = true` replies `{ request }` and the daemon broadcasts `ai.chunk` per text delta, then `ai.done` (the plain reply) or `ai.error`; the panel turns the thinking row into the live text with a caret and ignores events for any other request id. **The tool loop (same day):** `⌘↩` asks with `agent = true`, so the model may call `run_command`. Each call appears in the conversation as Warp's embedded command block — `▶ command · class`, the model's one-line reason, and its status — and, unless a policy rule decided under autonomy, waits in the **inbox**: the approval card above the panel and the `⌘⇧A` sheet show it with the verdict and the floor reason exactly like a Claude Code request (`source: agent-mode`). Allow types it at the session's prompt (a busy session refuses, and the block says so), the real block appears in the grid, and its output, clipped and redacted, comes back to the model and into the panel's block with the exit status; Deny sends the reason to the model. Text before a command stays in place as a segment; the final answer shows only the last segment and history carries the whole run. An answer that took a second or more gets Warp's `Thought for N seconds ›` row above it; the approval card has `Edit…`, which opens the inbox sheet with the command field focused. `⌥⌘E` (`ai.show_last_payload`, Agent → Show Last Payload Sent…) opens a sheet with the last request sent on this session's behalf, exactly as it left — model, redacted system text, messages, tools — or says that nothing has been sent; `vterm egress tail|last` shows the same from the CLI and the log. Not built yet: the verdict line, `⌥⏎ explain more`, `⌥⌘E` payload view, `⌘⇧K` attach, thinking rows, task ticks and the approval card.

## 7. Keymap

Two keymap profiles ship: **`macos`** (⌘-based, Ghostty-like) and **`tmux`** (prefix `C-b`, remappable). `tmux` is the default for multiplexer actions per your preference; both are complete, and the config can mix them.

| Action | macOS profile | tmux profile |
|---|---|---|
| New tab / window | `⌘T` / `⌘N` | `<prefix> c` / — |
| Split right / down | `⌘D` / `⌘⇧D` | `<prefix> %` / `<prefix> "` |
| Focus pane | `⌘⌥←→↑↓` | `<prefix> ←→↑↓` |
| Zoom pane | `⌘⇧↩` | `<prefix> z` |
| Detach session | `⌘⇧D` | `<prefix> d` |
| Session list | `⌘K` then `>sessions` | `<prefix> s` |
| **Approval inbox** | `⌘⇧A` | `<prefix> a` |
| Notifications mailbox | `⌘⇧M` | `<prefix> m` |
| Command history search (Warp-mode editor) | `⌃R` | — |
| Reopen the last closed tab (reattach) | `⌘⇧T` | — |
| Toggle input sync for this pane | `⌘⌥I` | — |
| Rename tab | `tab.rename` (unbound; Shell menu) | — |
| Show last payload sent | `⌥⌘E` | — |
| Next pending approval | `⌘⇧↩` | `<prefix> A` |
| Command palette | `⌘⇧P` | `<prefix> :` |
| ⌘K assistant | `⌘K` | `<prefix> k` |
| Explain last failure | `⌥E` | `<prefix> e` |
| Show last egress payload | `⌥⌘E` | — |
| New agent task (worktree) | `⌘⇧N` | `<prefix> N` |
| Interrupt agent | `⌘.` | `<prefix> C-c` |
| Search scrollback | `⌘F` | `<prefix> /` |
| Jump to previous prompt | `⌘↑` | `<prefix> [` |
| Jump to next prompt | `⌘↓` | `<prefix> ]` |
| Select previous / next block | `⌃⌘↑` / `⌃⌘↓` | — |
| Extend the block selection | `⌃⌘⇧↑` / `⌃⌘⇧↓`; `⌘`-click toggles, `⇧`-click ranges | — |
| Top / bottom of the selected block | `⌘⇧↑` / `⌘⇧↓` | — |
| Bookmark block; previous / next bookmark | `⌘B`; `⌥↑` / `⌥↓` | — |
| Copy block command / output | `⌘⇧C` / `⌘⌥⇧C`; `⌘C` with a block selected copies its output; copy both and copy-as-HTML in the menu | — |
| Re-input command (plain / with sudo); re-run | `⌘I` / `⌘⇧I`; menu | — |
| Block menu from the keyboard | `⌃M` | — |
| Clear scrollback | `⌘⇧K` | — |
| Find in scrollback; next / previous match | `⌘F`; `⌘G` / `⌘⇧G` (↩ / ⇧↩ and ⎋ in the bar) | — |
| Sticky command header | on by default (`[blocks] sticky_header`); Blocks › Toggle Sticky Command Header per pane | — |
| Sidebar (vertical tabs) | `⌘\` | — |
| Scroll back / forward a page | `⇧PgUp` / `⇧PgDn` | `<prefix> PgUp` / `<prefix> PgDn` |
| Scroll to oldest line / live end | `⇧Home` / `⇧End` | — |

Conflict rule: when both profiles are active, `⌘K` is the assistant and the palette is `⌘⇧P`. Every binding is remappable; `vterm keys` prints the resolved map and flags conflicts.

> **As built (M4, 2026-09-05):** the table above binds `⌘⇧D` to both "split down" and "detach session", and `⌘⇧↩` to both "zoom pane" and "next pending approval". The shell resolves them as split-down and zoom; detach and next-pending are reachable through the tmux prefix (`<prefix> d`, `<prefix> A`) until the table is corrected. `⌘W` closes the focused pane (detaching its session, never killing it), and `<prefix> x` does the same. Bindings whose feature is not built yet (`⌘⇧A`, `⌘K`, `⌘F`…) beep and log "not available yet: …" rather than falling through to the terminal. `⌘,` opens Settings (`settings.open`), which this table did not list. The shell takes the whole table from the daemon's resolved keymap (`vterm keys`), so `keymap.toml` overrides apply to the GUI as well.

> **As built (M5 blocks, 2026-09-06):** the viewport is daemon state (`session.scroll`), so wheel, ⇧PgUp/PgDn, ⌘↑/⌘↓ and every attached viewer move together, and typing snaps back to the live end. Block chrome is drawn from the daemon's block rows: a gutter stripe in the left padding (green exit 0, red otherwise, grey when the exit is unknown), a hairline above each command line, an `exit N` chip at the right end of the command line when that space is blank, and `≈` in the chip when the block is a guess (§4). Clicking the gutter or the command line selects the block (tinted); right-click opens the kebab menu with copy command / copy output / re-run; "explain" is listed but disabled until M-AI. Not built yet from §4: collapse, duration and cwd in the header, and "share as text". `block.rerun` has no default chord on purpose — it executes the command.

## 8. Notifications

Off by default except for `awaiting_input`, which is the entire point.

| Event | Default | Channel |
|---|---|---|
| Agent awaiting input | on | Notification Center, actionable (Allow / Deny / Open) |
| Agent finished | on when the app is not frontmost | Notification Center |
| Agent crashed | on | Notification Center |
| Long command finished (> 30 s) | on when the pane is not visible | Notification Center |
| Budget threshold crossed | on | In-app banner |
| Watchdog: agent idle mid-task | off | In-app badge |

Respects Focus modes. Notifications are coalesced per session — three approvals in ten seconds is one notification saying "3 approvals".

> **As built (2026-09-06):** every event becomes a note in the app's **mailbox** (`⌘⇧M`, `<prefix> m`, Agent → Notifications…: All / Unread / Errors, `j`/`k`/`↑`/`↓`, `↩` opens the session, `⎋` closes, mark all read; capped at 200, coalesced per session and kind within `coalesce_window_ms` — "3 approvals waiting"). Where a note goes follows this table: a finished command over `long_command_ms` toasts when its pane is not the focused one of the key window and goes to Notification Center when the app is not frontmost; an agent that stopped follows `agent_finished`; a crash always shows; a new approval toasts when its pane is off screen (the daemon posts the Notification Center entry for approvals itself, so `vterm`-only users get it too — the app does not post a second one); Agent Mode's end goes to Notification Center only when the app is in the background. Toasts: at most two, top right, six seconds, hover pauses, click focuses the session. Notification Center entries are plain (no Allow/Deny buttons yet) and only from the bundled app. Budget and watchdog rows are not built.

## 9. Onboarding

First launch does exactly four things, in one scrolling sheet, all skippable:

1. **Install shell integration?** Shows the exact lines and the file. Detects Powerlevel10k / starship / oh-my-zsh and warns that a printing preexec hook can corrupt marks.
2. **Import a theme?** Detects existing Ghostty / iTerm2 / Alacritty themes on disk and offers them.
3. **Configure a provider.** Local first if a local runtime is detected and running, otherwise a key field that writes to Keychain. Shows what will be sent and lets you set the workspace default to `none` right there.
4. **Detect agents.** Reports which of `claude` / `codex` are on `$PATH`, their versions, and whether hook installation will be possible (checks for `disableAllHooks` / `allowManagedHooksOnly`).

No account. No sign-in. No tour.

## 10. Accessibility

- Full VoiceOver support for chrome; the grid exposes an `NSAccessibility` text model with block boundaries as elements.
- Respect Reduce Motion (no animated block collapse), Increase Contrast (theme adjustment), and Differentiate Without Color (state glyphs are shapes, never colour alone — this is why `✋` and `⚙` are glyphs).
- Minimum contrast checked for shipped themes; imported themes get a warning if they fail, not a rejection.
- Every notification's information is also available in the app, because notifications can be silenced by Focus.
