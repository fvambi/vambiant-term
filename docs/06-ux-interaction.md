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

## 4. Blocks

A block is one command (from OSC 133) or one agent event. Rendering:

- **Collapsed by default** when output exceeds N lines; the header stays visible.
- Header: exit code chip, duration, cwd if it differs from the pane's, and a kebab menu (copy command / copy output / rerun / explain / share as text).
- **Thinking blocks collapsed**, tool calls collapsed with a one-line summary, diffs expanded.
- Failed blocks get a subtle left border and an `⌥E explain` affordance in the gutter — not a popup, not a banner.
- Selection is semantic: clicking a block's header selects the whole block; `⌘⇧↑` selects the previous block.
- When shell integration is unavailable or broken, blocks degrade to heuristic segmentation with a small "≈" marker in the gutter, and the tooltip says why.

## 5. Inline suggestions

- Ghost text renders in the dim foreground colour after the cursor. `→` accepts all, `⌥→` accepts one word, any other key dismisses.
- History match beats model completion; a model suggestion only renders if it arrives inside the budget (p50 < 120 ms). **A late suggestion is discarded, not rendered.** Text appearing under your fingers after you have started typing is worse than no suggestion.
- One in-flight request per pane. Every keystroke cancels the previous.
- Never suggest inside a running program's input (we know from OSC 133 whether we are at a prompt).
- A suggestion classified above `benign` renders with a coloured underline; accepting it still requires Enter, and the safety confirm applies.

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
| Next pending approval | `⌘⇧↩` | `<prefix> A` |
| Command palette | `⌘⇧P` | `<prefix> :` |
| ⌘K assistant | `⌘K` | `<prefix> k` |
| Explain last failure | `⌥E` | `<prefix> e` |
| Show last egress payload | `⌥⌘E` | — |
| New agent task (worktree) | `⌘⇧N` | `<prefix> N` |
| Interrupt agent | `⌘.` | `<prefix> C-c` |
| Search scrollback | `⌘F` | `<prefix> /` |
| Jump to previous prompt | `⌘↑` | `<prefix> [` |

Conflict rule: when both profiles are active, `⌘K` is the assistant and the palette is `⌘⇧P`. Every binding is remappable; `vterm keys` prints the resolved map and flags conflicts.

> **As built (M4, 2026-09-05):** the table above binds `⌘⇧D` to both "split down" and "detach session", and `⌘⇧↩` to both "zoom pane" and "next pending approval". The shell resolves them as split-down and zoom; detach and next-pending are reachable through the tmux prefix (`<prefix> d`, `<prefix> A`) until the table is corrected. `⌘W` closes the focused pane (detaching its session, never killing it), and `<prefix> x` does the same. Bindings whose feature is not built yet (`⌘⇧A`, `⌘K`, `⌘F`…) beep and log "not available yet: …" rather than falling through to the terminal. `⌘,` opens Settings (`settings.open`), which this table did not list. The shell takes the whole table from the daemon's resolved keymap (`vterm keys`), so `keymap.toml` overrides apply to the GUI as well.

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
