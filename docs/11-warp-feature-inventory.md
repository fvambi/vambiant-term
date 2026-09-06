# 11 — Warp feature inventory, mapped against Vambiant Term

> Source: warp.dev/terminal and the complete docs.warp.dev feature tree (terminal, agents, code, knowledge & collaboration, Agent CLI, getting started), captured 2026-09-06 via the `_llms-txt/*.txt` dumps the docs site publishes. Warp is AGPL v3 since 2026; **its code must never be read or copied into this MIT codebase**. Its docs are fair to read. Anything marked *(docs unclear)* was ambiguous or contradictory in Warp's own pages.
>
> Columns: **Ours today** = what exists in this repo on 2026-09-06 (see `07`). **Disposition**: `have` built · `plan` already in `01` (ref) · `new` not in `01`, adopt · `adapt` adopt a local-only equivalent · `won't` excluded by `00 §4` or a non-negotiable, with the reason. `12-warp-parity-plan.md` schedules every `new`/`adapt`/`plan` row.

## A. Blocks

| ID | Warp feature | Warp detail | Ours today | Disposition |
|---|---|---|---|---|
| A1 | Block = command + output | Atomic unit; created on every command via shell integration; grows bottom-up | `vt-blocks` segmenter, gutter, chips | have |
| A2 | Failed-block colouring | Non-zero exit: red background **and** red sidebar | Red gutter + red chip only | plan B3.1: add subtle red tint of default-bg cells, config-gated |
| A3 | Block dividers / compact mode | `appearance.blocks.show_block_dividers` (default on); `appearance.spacing = normal\|compact` | Hairline above each command line, always on | new: `[blocks] dividers` key; compact is N/A (we add no spacing) |
| A4 | Select one block | Click; `⌘↑` selects most recent then `↑/↓` moves | Click gutter/header; `⌘⇧↑/↓` moves selection | have (chords differ; see `12` K1) |
| A5 | Multi-select | `⌘`-click toggle, `⇧`-click range, `⇧↑/⇧↓` extend from the active block | Single selection only | new |
| A6 | Navigate blocks | Arrows between blocks, PgUp/PgDn, Home/End, `⌘⇧↑/↓` top/bottom of selected block, "jump to bottom" button on long blocks | `⌘↑/↓` prompt jumps, `⇧PgUp/PgDn`, `⇧Home/End` | plan A4.5: add top/bottom-of-block, jump button |
| A7 | Sticky command header | Header of the block cut off at the top stays pinned; click jumps to block start; `⌃S` per-pane toggle, `⌘⌃S` global | None | new |
| A8 | Block actions menu | Copy command / output / both; Share…; Toggle bookmark `⌘B`; Find within block `⌘F`; Toggle filter `⌥⇧F`; Re-input `⌘I`; Re-input as root `⇧⌘I`; Copy output `⌥⇧⌘C`; Copy command `⇧⌘C`; `⌃M` opens the menu from the keyboard | Copy command / output, re-run, explain (disabled) | plan B3.2: add copy both, re-input (insert without newline), sudo re-input, keyboard menu |
| A9 | Bookmarks | Per-block flag; indicator strip shows position in history with hover preview (prompt, command, last two output lines); `⌥↑/⌥↓` jump between bookmarks | None | new |
| A10 | Find | `⌘F`: regex, case, "on selected block" scope; searches all blocks bottom-up in the pane; `⌘G`/`⇧⌘G` next/prev | None | plan A1.4 scrollback search |
| A11 | Block filtering | Per-block line filter: plain/regex/invert/case + context lines; non-destructive | None | new (as an overlay panel, not in-grid; see `12` A) |
| A12 | Background blocks | Output Warp attributes to a background process (`&`, outlived parent) gets a command-less block; documented misattribution limits | Output between D and next B lands in the previous command block | new: `BlockKind::Background` from the segmenter |
| A13 | Clear blocks | `⌘K` drops blocks from view **and** the restoration DB; `⌃L` keeps scrollback | `⌃L` via shell only | new: `session.clear` that truncates scrollback |
| A14 | Block sharing | Permalinks at `app.warp.dev/block/<id>`, HTML embeds, anyone-with-link, unshare in settings | None | won't (cloud, `00 §4`); adapt: export block as text/HTML/Markdown to clipboard or file |
| A15 | Copy on select, smart selection, rectangular selection | `terminal.copy_on_select` (default on); double-click selects URLs/paths/emails/IPs/floats; `⌘⌥`-drag column select; `terminal.smart_select.word_char_allowlist` | No text selection at all (⌘C beeps without a block) | plan A4.5 semantic selection: mouse selection is a prerequisite for most of this table |
| A16 | Input position modes | Pin to bottom (default), start at top (classic), pin to top (reverse, newest first) | Native terminal: shell decides | won't for "reverse"; "classic" is what a terminal does anyway |
| A17 | Session restoration of blocks | Blocks and scrollback restored from `warp.sqlite` on relaunch | Daemon keeps sessions alive; blocks persisted in `vt-store` | have (stronger: sessions survive, not just their text); layout restore is M9 |

## B. Input editor, entry, completions

Warp replaces the shell's line editor with its own. We keep the shell's editor and decorate it (decision `12` D1). Rows say what that costs.

| ID | Warp feature | Warp detail | Ours today | Disposition |
|---|---|---|---|---|
| B1 | Modern text editing | Click-to-place cursor, multi-line, soft wrap, word/subword motions, `⇧↩` newline, multi-cursor (`⌃⇧↑/↓`, `⌃G` next occurrence), fold | Shell editor (zle/readline) | won't as an editor; `⇧↩` newline and click-to-place are zsh/readline features, documented |
| B2 | Vim keybindings (input) | Warp's own vi emulation; `/ ? * #` open Command Search instead of searching | Shell vi mode | won't; shell vi mode already exists |
| B3 | Autocomplete quotes/brackets | `text_editing.autocomplete_symbols` (default on) | None | won't (zsh plugins do it) |
| B4 | Syntax highlighting of the input | Sub-commands, flags, args, variables coloured as you type; new commands unknown until new session | None | new: colour the B→C region client-side from a shell-word parse (`vt-policy` parser) |
| B5 | Error underlining | Dashed red underline under a command that is not on `PATH` | None | new: daemon `PATH` lookup, same decoration path as B4 |
| B6 | Alias expansion | Type alias + space → expanded inline; `⌥Space` for a literal space; off by default | None | won't (zsh `globalias` does it) |
| B7 | Command Inspector | Hover or `⌘⇧I`: docs for the token under the cursor | None | plan C4.2 (`--help` capture) → adapt: inspector popover from `man`/`--help` |
| B8 | Command corrections | thefuck rules (MIT); corrected command offered above the input after a failure; 21 rule families; on by default | None | new: port the rule families as data, offer via the failure affordance |
| B9 | Command history | Rich per-entry metadata (exit, cwd, duration, last run); `↑` prefix search; `⌃R` fuzzy panel; per-session isolation merged on close | `vt-store` blocks already hold cmdline, exit, cwd, timestamps | new: history palette over `vt-store`, cross-session by design |
| B10 | Command Search (3-in-1) | `⌃R`: history, workflows, prompts, agent history; prefixes `h:` `p:` `a:` | None | adapt: one palette with prefixes, no cloud sources |
| B11 | Synchronized inputs | Whole-command sync across panes in a tab or all tabs; `⌘⌥I` toggle; tab indicator | None | plan A3.4 pane sync |
| B12 | YAML workflows | Parameterised commands; `~/.warp/workflows/`, `<repo>/.warp/workflows/`; `{{arg}}`; `⌃⇧R` search; `⇧Tab` cycles args; open spec at warpdotdev/workflows | None | adapt: local workflows crate, read the same open YAML shape from our own dirs |
| B13 | Autosuggestions | Inline ghost text from history and completions; `→`/`⌃F` accept, `⌃→` accept a word; "Next Command" (LLM) variant free on all plans | None | plan C1.1 ghost text (history first, model second) |
| B14 | Tab completions with specs | Fuzzy menu over Fig-style specs (~430 commands, ~30 "full"); aliases resolved; "open as you type" option; native shell completions as a preview | Shell completion | plan-later: spec-driven menu is C tier; shell completion stays authoritative |
| B15 | Hint text / message bar | Grey hint in the input; `terminal.input.show_hint_text`, `show_terminal_input_message_bar` | None | adapt: status line hints (`⌘K`, `⌥E explain`) per `06 §2` |
| B16 | Natural-language detection | Classifies typed text as prompt vs command, `⌘I` toggles, `!`/`*` prefixes, local classifier, denylist | None | won't as auto-switch; `⌘K` is the explicit NL entry (C1.4) |
| B17 | Generate (legacy) `#` / `` ⌃` `` | NL → command in the input | None | plan C1.4 |

## C. Windows, tabs, panes, sessions

| ID | Warp feature | Warp detail | Ours today | Disposition |
|---|---|---|---|---|
| C1 | Tabs | `⌘T`, `⌘W`, `⌘⇧T` reopen (60 s grace), `⌘1..9`, `⌘⇧{ }`, `⌃⇧←/→` move, rename by double-click, drag to another window | Native NSWindow tabs; no reopen, no numbering, no rename | plan A3.1: add reopen (reattach), `⌘1..9`, rename, `general.new_tab_placement` |
| C2 | Tab colours and groups | Colour per tab (theme ANSI names), per-directory colours, named colourable collapsible groups, pin tabs/groups, `/set-tab-color` | None | new (groups only in the vertical sidebar; native tab bar cannot group) |
| C3 | Tab indicators | Maximised pane, synced input, error exit shown in the tab | None | new |
| C4 | Vertical tabs sidebar | Row per pane or tab; metadata: cwd, branch, worktree, agent status badge (working/blocked/done/error/cancelled), diff stats, PR badge; search; hover sidecar; density; drag/drop | Planned sidebar (B1.5 repo → worktree → session) | plan B1.5: **merge** — our sidebar *is* Warp's vertical tabs plus the repo tree |
| C5 | Split panes | `⌘D`/`⌘⇧D`, `⌘[`/`⌘]`, `⌘⌥arrows`, `⌘⇧↩` maximise, drag pane header between tabs/windows, `⌃⌘arrows` resize, dim inactive, focus follows mouse | Splits, focus, zoom, close | plan A3.2: add prev/next, keyboard resize, drag-to-reparent, dimming, focus-follows-mouse |
| C6 | Tab Configs | TOML: `name`, `title`, `color`, `[[panes]]` tree (`terminal\|agent\|cloud`, `directory`, `commands`, `shell`, `is_focused`, `split`, `children`), `[params]` (`text\|branch\|repo`, `{{autogenerated_branch_name}}`), "Save as new config", "New worktree config", default for `⌘T`, `warp://tab_config/<name>` | None | plan A3.5 layouts **plus** B4.1 task new: one TOML layout format with params and worktree creation |
| C7 | Launch Configurations (legacy YAML) | Multi-window layouts, `active_window_index`, nested `split_direction` | None | fold into C6 (windows level) |
| C8 | Session Navigation palette | `sessions:` lists every session with running command / status; `⌃Tab` MRU option | `vterm ls`; no palette | plan C1.10 palette: `sessions:` prefix |
| C9 | Session restoration | Windows/tabs/panes/blocks restored on relaunch; SQLite; `general.restore_session` | Daemon holds sessions; app does not restore layout | plan A3.6 (M9): layout snapshot in `vt-store`, reattach on launch |
| C10 | Undo close | `general.undo_close.grace_period = 60` | Close = detach (session keeps running) | new: "Reopen closed tab" = reattach, no grace limit needed |
| C11 | Global hotkey / Quake window | Dedicated dropdown window pinned top/bottom/left/right with size %, screen choice, autohide on blur; or show/hide all windows; needs Accessibility | None | plan A3.8 (C tier) |
| C12 | Configurable header toolbar | Reorder/hide panel buttons; side decides where panels open | None | new (small) once the sidebar, inbox and review panels exist |
| C13 | Working directory for new sessions | Home / previous / custom; advanced per window/tab/pane | `[session]` in `09` has cwd policy | have (verify per-source advanced mode) |
| C14 | Quit warning | Warn when a running process exists; "show running processes" | Quit never kills (daemon) | adapt: on quit show "N sessions keep running in vtermd" once |
| C15 | Window size/opacity/blur | Custom cols×rows for new windows, opacity 1–100, blur radius | Window sized 100×30 | plan A2.9 + new window-size keys |

## D. Appearance

| ID | Warp feature | Warp detail | Ours today | Disposition |
|---|---|---|---|---|
| D1 | Themes | 21 bundled; YAML custom themes (`accent`, `cursor`, `background`, `foreground`, `details`, `terminal_colors.normal/bright`, gradients, `background_image{path,opacity}`); theme creator from an image; OS sync with a light and a dark pick | TOML themes, follow-system, contrast check, importers planned | plan D2.3: add Warp YAML import (themes repo licence to verify), gradients/background image as S |
| D2 | Text | Font, weight, size, line height, thin strokes (`never\|low\|high\|always`), minimum contrast (`never\|only_named_colors\|always`), ligatures, separate AI font | Font family/fallback/size/line height, bold-is-bright | plan A2.6: thin strokes, min contrast, weight, ligatures (A2.3) |
| D3 | Cursor | Bar/block/underline, blink | Same | have |
| D4 | Prompt | Warp-native prompt with drag-and-drop chips (cwd, git branch + dirty count, k8s context, pyenv, time) vs shell PS1; compatibility table for p10k/starship/… | Shell PS1 only | won't (our prompt is the shell's); chips belong in the sidebar/status line |
| D5 | App icons | 16 built-in dock icons | One | won't (C at best) |
| D6 | Pane dimming / focus follows mouse | Two toggles | None | see C5 |
| D7 | Tabs behaviour | Indicators, tab bar visibility (`always\|hide_fullscreen\|on_hover`), close button side | Native | plan A3.1 subset |
| D8 | Alt-screen padding | `alt_screen_padding` custom uniform padding for full-screen apps, default 0 | Fixed padding | new (small) |
| D9 | Zoom level / font size keys | `⌘+`/`⌘-`/`⌘0` | None | new (small) |

## E. Terminal features and rendering

| ID | Warp feature | Warp detail | Ours today | Disposition |
|---|---|---|---|---|
| E1 | SGR coverage (Warp's own table) | Warp: **no** dim, italic, curly/coloured/double underline, blink, invisible, overline, sixel, RTL | Bold, italic, underline, strikeout, inverse, dim, hidden | have (beats Warp); new: curly/double/coloured underline, overline, blink-with-cap (A2.7) |
| E2 | Mouse and scroll reporting | Toggles; menu View › Toggle Mouse Reporting | Backend supports modes; no toggle | new (small) |
| E3 | Kitty keyboard protocol | Auto when the app asks | have (M1) | have |
| E4 | OSC 8 hyperlinks, file/URL detection | `⌘`-click opens; `file:line:col` and five other suffix forms; editor choice incl. `$EDITOR`, VS Code, JetBrains, Zed, Cursor…; drag folder onto Dock icon; Finder services; `.command` files | OSC 8 parsed; nothing clickable | plan A1.6 + new: path/URL detection, `[editor]` config, Dock drop, Finder service |
| E5 | Images | Kitty graphics "for most common workflows"; no Sixel | None | plan A1.9 (S) |
| E6 | Audible bell | Off by default | Bell event parsed | new (small) |
| E7 | Desktop notifications | Long-running command (`long_running_threshold = 30 s`), password prompt detected, OSC 9 / OSC 777, only when not frontmost, toast duration | OSC 777 parsed; inbox reminders notify | plan B2.4 + new: command-finished and password-prompt notifications |
| E8 | Markdown viewer | Rendered `.md` in a pane; runnable shell blocks insert into the input; mermaid | None | won't (C at most) |
| E9 | URI scheme | `warp://action/new_window?path=`, `new_tab`, `launch/<cfg>`, `tab_config/<name>`, `settings?q=` and deep links | None | new: `vambiant-term://` (small) |
| E10 | Accessibility | Self-announcements, verbosity level, no VO navigation | None | plan M9 (`06 §10` sets a higher bar) |
| E11 | Full-screen app handling | Alt screen padding, mouse toggle | have alt screen | see D8/E2 |
| E12 | Settings sync (cloud) | Most settings synced via account | TOML files | won't (cloud); `config.toml` in git is the local answer |
| E13 | OSC 52 | `deny\|write_only\|read_write`, default **deny** | write on, read off | have (`05`) |
| E14 | Scrollback limit | `terminal.maximum_grid_size = 50000` rows | 32 MiB byte budget in libghostty | have; expose as a config key |

## F. Warpify: subshells, SSH, containers

| ID | Warp feature | Warp detail | Ours today | Disposition |
|---|---|---|---|---|
| F1 | Subshell warpify | Banner on `bash/zsh/fish`, `docker exec`, `poetry shell`, `aws-vault exec`, …; opt-in DCS `SourcedRcFileForWarp` handshake from rc files; allow/deny command lists | Integration injected once at session start | plan A4.6: re-inject on nested shells via a DCS handshake of our own |
| F2 | SSH extension | Companion server in `~/.warp/remote-server` on the host, no ports, glibc ≥ 2.31, file tree/editor/indexing remotely; legacy wrapper bootstraps blocks over the existing connection | None | adapt: blocks over SSH via a bootstrap snippet sent through the PTY (opt-in); no remote server |
| F3 | Docker Desktop / Raycast / VS Code / JetBrains integrations | Open container shells, open Warp from IDEs | None | new: URI scheme (E9) covers Raycast/IDE "open here"; Docker extension is C |

## G. Agents (Warp Agent and third-party CLI agents)

Warp ships its own agent. `00 §4` says we do not. Rows below separate *supervising* features (ours to build) from *being an agent* (won't, or the agent's own job).

| ID | Warp feature | Warp detail | Ours today | Disposition |
|---|---|---|---|---|
| G1 | Agent Mode conversation view | Dedicated view, streaming markdown, tool cards, diffs, thinking blocks, questions cards, task lists, plans, `⌘↩` to start, `⌘I` toggle, `⌘Y` conversation picker, panel `⌘⇧H` | None | B3.3 for *observed* agents; a first-party conversation UI needs decision `12` D2 (ACP client, B6.5) |
| G2 | Third-party CLI agent detection | Auto-detect Claude Code, Codex, OpenCode, Amp, Auggie, Copilot CLI, Cursor, Gemini, Droid, Pi, Goose, Antigravity, Hermes, Mistral Vibe by command *(mechanism undocumented)*; "agent toolbelt" footer | Claude + Codex adapters, generic heuristic adapter | plan B6.4: widen the generic adapter's process detection to that list |
| G3 | Agent notifications | Plugin-driven for Claude Code / Codex / OpenCode; in-app toasts (max 2), mailbox with All/Unread/Errors, tab badges, desktop alerts; types complete/request/error | Inbox (CLI), reminders, desktop notification on deferred approvals | plan B2 + new: mailbox = inbox history view, toasts, tab badges, complete/error events |
| G4 | Rich input editor for CLI agents | `⌃G` composer: multi-line, `@` files/folders/symbols, images, voice, `/prompts` `/skills`; auto-show when the agent blocks; submit on `⌃↩` option | None | new: composer that pastes into the agent's PTY (bracketed paste), `@` from repo index |
| G5 | Attach context | Blocks (`⌘↑`), selection (`⌘L`), images (5/request), URLs (scraped), `@` mentions; pending vs attached | None | new for CLI agents (paste path/text); B3 blocks are the source |
| G6 | Interactive code review | Inline comments on hunks, batched, sent to the running agent (any supported CLI agent) | None | new: review panel (H2) + "send comments" into the agent's input |
| G7 | Profiles & permissions | Per-profile: model, permission levels (agent decides / always ask / always allow / never) per action, regex allow/denylist (denylist wins), MCP allow/deny, "run until completion" auto-approve **bypasses the denylist by default** | `vt-policy` skeleton; never-auto floor is an ADR | plan B5.1: profiles = named policy sets; auto-approve = per-session override that **cannot** cross the floor (our differentiator) |
| G8 | Rules (`AGENTS.md`/`WARP.md`, global rules) | Precedence subdir › root › global; `/init` links `CLAUDE.md`, `.cursorrules`, …; rules cited in responses | None | adapt: read-only "what applies here" inspector for the agent's own rule files |
| G9 | Skills | `SKILL.md` discovery across ten vendor dirs; `$ARGUMENTS`; `/skill` invocation | None | adapt: inspector only (skills are the agent's) |
| G10 | MCP servers | CLI/HTTP/SSE servers; file-based configs from Warp, Claude, Codex, `.agents`; project-scoped never auto-spawn; OAuth; logs; sharing | None | adapt: inspector over the agents' MCP config files, toggle project-scoped trust; D2.7 (C) is the reflexive server |
| G11 | Planning, task lists, questions, forking, queueing, compaction, rewind | Agent-native conversation features | None | plan B3.3 render TodoWrite/plan/question events; B1.7 fork; new: prompt queue (send when idle) |
| G12 | Full Terminal Use, computer use, browser use, web search, memory, orchestration, cloud handoff | The agent's capabilities | N/A | won't: those are the agents' features; we surface their events |
| G13 | Remote Control / session sharing | Publish a session to the cloud; view/steer from phone; one-week retention; **secret redaction not applied** | Loopback API planned (D2.5) | won't (cloud); D2.5 + Tailscale is the local answer |
| G14 | Voice input | Wispr Flow cloud transcription, push-to-talk | None | adapt (C): on-device `SFSpeechRecognizer` only, no cloud |
| G15 | Models, BYOK, custom endpoint, routers | Curated list; BYOK keys transit Warp's backend; custom endpoint **must be public** (localhost rejected); complexity/rule routers in `~/.warp/custom_model_routers/*.yaml`; fallback chains | `vt-ai` skeleton, `providers.toml` | plan C3: we already allow localhost; add rule/complexity routing to C3.6 as S |
| G16 | Active AI: prompt suggestions, next command, suggested code diffs | Proactive banners | None | plan C1.1, C1.6, C1.7 |
| G17 | Warp Agent CLI (`warp`) | Standalone TUI agent with its own PTY multiplexer | `vterm` supervises, does not converse | won't (it is an agent) |

## H. Code

| ID | Warp feature | Warp detail | Ours today | Disposition |
|---|---|---|---|---|
| H1 | Code editor, file tree, find/replace, LSP, vim | Bundled editor with five LSP servers | None | won't (`00 §4`, Zed's job) |
| H2 | Code Review panel | Uncommitted / vs main / vs branch diffs; revert hunk; discard file/all; open in editor; attach diff as context; git diff chip in the input bar; live refresh; AI commit messages | None | new: read-only diff review with revert/discard; commit-message generation is C3-dependent |
| H3 | Git worktrees | Auto-detected; per-worktree review/chip/index; creation via git CLI only; "New worktree config" tab config | `vt-worktree` stub, M7 | plan B4 (we go further: registry, ownership guard, promote) |
| H4 | Zero state | New tab offers Create / Open / Clone repo, runs `/init` | None | won't (agent-centric); onboarding sheet is M9 |

## I. Warp Drive, teams, admin, sharing

| ID | Warp feature | Warp detail | Ours today | Disposition |
|---|---|---|---|---|
| I1 | Warp Drive (cloud workspace) | Workflows, notebooks, prompts, env vars, rules, MCP; folders, trash, permissions, web app, offline read-only | None | won't (cloud); local files + git per row below |
| I2 | Workflows | See B12; "Save as Workflow" from a block; AI autofill; enum args (static/dynamic) | None | adapt (B12) |
| I3 | Notebooks | Runnable markdown docs | None | won't |
| I4 | Prompts | Saved parameterised prompts, `p:` search | None | adapt: local prompt files for `⌘K` (M6) |
| I5 | Environment variable sets | Static and dynamic (1Password/LastPass/`vault` command) values, load into session / subshell / workflow | None | adapt: `envsets.toml` + Keychain, dynamic via command, loaded via visible `export` lines |
| I6 | Teams, admin panel, enterprise redaction, SSO, billing | Org policy enforcement | None | won't; per-workspace `policy.toml` (D1.6) is the single-user equivalent; note Warp's secret redaction is **off by default**, ours is always on (D1) |
| I7 | Session sharing (real-time) | Server-mediated | None | won't |

## J. Settings and configuration

| ID | Warp feature | Warp detail | Ours today | Disposition |
|---|---|---|---|---|
| J1 | `settings.toml` | Hot reload, error banner with "Open settings file", bundled JSON schema, agent-editable via a skill, migration | `config.toml` + `keymap.toml`, hot reload, Settings window | have; plan D2.2 JSON schema, M9 error overlay |
| J2 | Keybindings | `keybindings.yaml`, action ids (`terminal:copy_outputs`…), conflict highlighting, keysets repo | `keymap.toml`, `vterm keys` conflicts | have |
| J3 | Notification preferences | Long-running threshold, needs-attention, password prompt, sound, toast duration | None | new with E7 |
| J4 | Privacy | `telemetry_enabled` default **true**, crash reporting default true, secret redaction default **false** | No telemetry ever; redaction always on | have (opposite defaults, on purpose) |
| J5 | Settings deep links | `warp://settings?q=` | None | new with E9 |

## K. Keymap (Warp defaults worth matching)

Full table in Warp's docs; chords that shape muscle memory and are free in our map: `⌘↑/⌘↓` select prev/next block (we use them for prompt jumps), `⌘B` bookmark, `⌘K` clear, `⌘F` find, `⌘G`/`⇧⌘G` next/prev match, `⌘⇧C` copy command, `⌥⇧⌘C` copy output, `⌘I` re-input, `⌥↑/⌥↓` bookmarks, `⌘[`/`⌘]` prev/next pane, `⌃⌘arrows` resize, `⌘1..9` tabs, `⌘⇧T` reopen, `⌘+`/`⌘-`/`⌘0` zoom, `⌘⇧P` palette, `⌃R` history, `⌘⌥I` sync inputs, `⌘,` settings. Conflicts with `06 §7`: `⌘K` (ours: AI ask), `⌘I` (unbound), `⌘↑` (ours: previous prompt, which is what Warp's "select previous block" effectively does). Resolution proposed in `12` K1.

## L. Warp features we deliberately exceed

- Sessions outlive the app (daemon, ADR-0004); Warp restores text, we keep the process.
- Redaction always on and fail-closed (ADR-0007); Warp's is off by default and not applied to sharing.
- Never-auto floor (ADR-0009); Warp's auto-approve bypasses its own denylist by default.
- Local models on `localhost` are first-class (C3.5); Warp rejects non-public endpoints.
- No telemetry (`00 §4`); Warp's is on by default.
- Dim, italic and strikethrough render; Warp's own table says they do not.
