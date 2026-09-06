# 12 — Warp parity plan

> Companion to `11-warp-feature-inventory.md`, which lists every Warp feature with its disposition. This document plans the work for every row marked `plan`, `new` or `adapt`, in build order, with the crate or file that owns each piece, the mechanism, the tests, and the docs that change. Estimates are engineer-weeks for one person and assume the M0–M5 foundations that exist on 2026-09-06.
>
> Status: **accepted direction** (ADR-0011, 2026-09-06): the owner decided the app must look like Warp and ship the same features. The decisions below are resolved accordingly; §L is the visual spec.

## 0. Principles that decide the shape

1. **The app owns the line editor (Warp mode).** A native editor pinned at the bottom of the pane submits commands to the shell; the injected integration makes the shell's own prompt invisible and prints one blank row per prompt that the app fills with Warp's context line (cwd, branch, duration). While a command runs, keystrokes pass straight to the PTY; alt-screen apps get the whole pane. `[input] mode = "classic"` restores the shell's editor. Warp mode is where syntax highlighting, autosuggestions, history search, vim keys and multi-line editing live, as native text-view features.
2. **Blocks are data in the daemon, chrome in the app.** Bookmarks, find, filters and exports are RPCs over `vt-store`/`vt-core`; the app never re-parses output.
3. **Local files replace Warp Drive.** Workflows, notebooks, prompts, env sets, rules and layouts are files under `~/.config/vambiant-term/` and `<repo>/.vambiant-term/`, versionable in git. Nothing syncs; the UI labels these "local" where Warp would show cloud state.
4. **Agent Mode is ours; CLI agents are supervised.** A first-party conversation view runs on the provider layer with `vt-policy` permissions; third-party CLI agents get the composer, notifications, review comments and context attachment as bytes typed into their PTY.
5. **Warp's defaults are not ours where they conflict with `05`:** redaction stays on, auto-approve never crosses the floor, nothing leaves the machine, no telemetry.

## D. Decisions (resolved by ADR-0011)

| # | Question | Decision |
|---|---|---|
| D1 | Line editor | **Warp mode by default**, classic mode kept |
| D2 | Conversation view | **Agent Mode** on the provider layer, plus the ACP client for third-party agents |
| D3 | Code panel | **Review panel, file tree, basic editor**; LSP later |
| D4 | Sharing / Remote Control | **Local equivalents only**: export to file/clipboard, loopback API for remote steering |
| D5 | Voice | **On-device** recognition only |
| D6 | Completion specs | **Adopt** withfig/autocomplete (MIT) specs for the Warp-mode completion menu |

## L. Look (from Warp's published screenshots, 2026-09-06)

- **Canvas:** near-black warm grey background (≈ `#1b1d23`), light grey text (≈ `#dcdfe4`), 13 pt monospace, generous line height. Window: traffic lights, sidebar toggle, tab strip with the cwd as title, `+` new tab, avatar/settings at the right. Ships as theme `warp-dark` (our own values), default on, not following the OS.
- **Block:** a thin divider above each block; row 1 is the **context line** in dim grey: tool version chip (`v20.15.1`), cwd, `git:(main)`, `(0.027s)` duration; row 2 the **command** in bold foreground; then output. Hover reveals four icons at the top right: bookmark, share/export, filter, kebab. A failed block gets a red left sidebar and a faint red wash. Selected blocks get the selection wash and a thicker sidebar.
- **Input area (bottom):** context chips in rounded pills with icons (tool version, cwd, branch with `± n`), the editor with placeholder text, a hint line (`⌘↩ for new agent`). In Agent Mode the header reads `ESC for terminal` and the conversation shows `Thought for 1 second ›` rows, task ticks, embedded command blocks, and an approval card (`Reject ^C · Edit ⌘E · Run ↵`).
- **Sidebar (vertical tabs):** search field, rows with an icon (terminal or agent brand with a status badge), title, cwd, branch, and a `+34 -9` diff pill; hover shows `⋮` and `✕`.
- **Chips and pills:** small rounded rectangles on a slightly lighter surface, green for ok, red for errors, magenta for "in progress".

## X. Explicitly not planned, with the reason (updated)

Cloud: Warp Drive sync, teams, admin, SSO, billing, session sharing, Remote Control, permalinks, settings sync, cloud agents, Oz, Factories (`00 §4` "not a cloud product", `05`). Windows/Linux (`00 §4`). Telemetry (`05`). Everything else Warp documents is in scope.

## A. Blocks (M5, 2.5 weeks)

Owner crates: `vt-blocks`, `vt-store`, `vtermd`, `vt-core`; app `Render/`, `Session/Blocks.swift`.

| Item | Mechanism | Tests | Docs |
|---|---|---|---|
| A2 failed tint | `[blocks] failed_tint = true`: renderer tints default-bg cells of failed blocks with `mix(bg, palette[1], 0.08)` | Swift: decoration → tint rows | `09` |
| A3 dividers key | `[blocks] dividers = true` gates the hairline | config test | `09` |
| A5 multi-select | `selectedBlocks: Set<Int64>` + `anchor`; `⌘`-click toggle, `⇧`-click range, `⇧↑/⇧↓` extend; copy/rerun act on the set in row order | `BlocksTests`: range and toggle arithmetic | `06 §4` |
| A6 block navigation | `block.top`/`block.bottom` actions scroll to `start`/`end-1`; "jump to bottom" chip on blocks taller than the viewport | Swift decoration test | `06 §7` |
| A7 sticky header | `StickyHeaderView` (NSView strip) shows the cmdline of `blocks.command(at: viewportTop)` when its `start < top`; click → `session.scroll row`; `⌃S` per pane | Swift: header block resolution | `06 §4` |
| A8 actions | Add copy-both, re-input (`send(text: cmdline)` without newline), sudo re-input (`"sudo " + cmdline`), `⌃M` keyboard menu, export HTML/Markdown (libghostty `Format::Html` via a `session.text` `format` param) | `scroll_e2e`: `format: "html"` returns markup | `06 §4` |
| A9 bookmarks | `blocks.bookmarked` column; `session.block.bookmark {id, seq, on}`; `session.blocks` returns the flag; indicator ticks in the right gutter at `start/total` proportional height with hover preview from `session.text(start, start+2)`; `⌥↑/⌥↓` jump | store migration test; e2e set/list | `02` RPC list |
| A10 find | `session.find {id, query, regex, case, from, to, limit}` → `[{row, col, len}]` scanned in the session thread over `text_range` in chunks of 500 rows; app find bar with `⌘F/⌘G/⇧⌘G`, match decorations, "in selected block" scope | Rust: regex and literal, chunk boundaries; e2e | `06` |
| A11 filter | Overlay sheet listing matching lines with context (plain/regex/invert/case) built from `session.text(outputRows)`; not in-grid (grid rows cannot be hidden without lying) | Swift filter unit test | `06 §4` says it is a panel |
| A12 background blocks | Segmenter: bytes with a newline arriving while no command is open and after a `D` create `BlockKind::Background {start,end}` closed on the next `A`; app draws a grey gutter labelled "background" | `segment.rs` tests | `10 §10` limits |
| A13 clear | `TerminalCore::clear_scrollback()` (erase history only, keep the live grid; libghostty's `\e[3J` path) → `session.clear`; `⌘K`-equivalent chord per K1; blocks below the cut are marked `cleared` not deleted | core test: total rows drop | `06 §7` |
| A14 export | Covered by A8 (text/HTML/Markdown to clipboard or file) | — | — |
| A15 selection | Mouse drag selection in the app (cell coordinates, absolute rows); `session.text` gains `from_col`, `to_col`, `rect`; double-click word/smart (regex over the line text for URL/path/email/IP/float), triple-click line; `⌘⌥`-drag rectangle; `[terminal] copy_on_select`; selection survives scrolling (absolute rows) | Rust: column-bounded text; Swift: hit-testing and smart-select regexes | `06 §4`, `09` |

## B. Input decoration, history, corrections (M5.5, 3 weeks)

Owner: `vt-policy` (shell parser), `vt-store`, `vtermd`, new `vt-workflows`; app `Input/`, `Palette/`.

| Item | Mechanism | Tests | Docs |
|---|---|---|---|
| B4 input highlighting | Daemon tracks the live B→C region (row/col span of the last `B` mark until `C`); on each delta it tokenises the region's text with `vt-policy`'s POSIX parser and emits `input.tokens {id, spans:[{row,col,len,kind}]}`; app colours those cells (command, flag, arg, string, variable) over the shell's own colours, off when the shell already highlights (`[input] highlight = auto\|on\|off`) | parser span tests; e2e with a typed command | `09`, ADR-0009 note that the parser is shared |
| B5 error underline | First token not found on the session's `PATH` (daemon `which` cache, refreshed on `PATH` change via OSC 7-style env report or `hash -r` heuristic) → dashed red underline decoration; suppressed for shell builtins/aliases/functions (`vt-shell` snippet exports the alias/function list once per prompt via a private OSC) | e2e: unknown command underlined, `ls` not | `10 §10` new OSC documented |
| B8 corrections | `vt-corrections` module in `vt-blocks`: rule families as data (`rules/*.toml` ported from thefuck, MIT, credited): `git` (typo subcommands, `--set-upstream`), `cd` (fuzzy path), `chmod +x`, `sudo`, `brew`, `npm`, `pip`, `cargo`, `docker`, `mkdir -p`, generic misspelled executable (edit distance over `PATH`); on non-zero exit compute ≤1 correction; app shows it in the failure affordance (`06 §4`) next to "explain"; accept inserts into the input | rule tests per family, corpus from thefuck's tests rewritten (no code copied) | `06 §4`, licence note in `10` |
| B9 history palette | `⌃R`: palette over `vt-store` blocks (`session.history {query, cwd, limit}` fuzzy on cmdline, ranked by recency and cwd match) showing exit, cwd, duration, session; `↩` inserts, `⌘↩` runs; `↑` in an empty prompt stays the shell's | store query test; e2e | `06 §7` |
| B10 unified search | Same palette with prefixes `h:` history, `w:` workflows, `p:` prompts, `sessions:`, `actions:`, `files:` (repo files via `git ls-files`) | palette parsing tests | `06 §2` |
| B11 pane sync | App-side fan-out: `syncGroup` on the window; typed bytes go to every member; tab indicator; `⌘⌥I` toggles current tab, palette action for all tabs | Swift: group membership | `06 §7` |
| B12 workflows | `vt-workflows`: YAML `{name, command, description, tags, arguments:[{name, description, default_value, enum?}], shells}` read from `~/.config/vambiant-term/workflows/` and `<repo>/.vambiant-term/workflows/` (and, read-only, `.warp/workflows/` for drop-in compatibility); `{{arg}}` substitution; `⇧Tab` cycles argument fields in the input; "Save as workflow" from a block | parse/substitute tests, e2e insert | `09`, `02` |
| B13 ghost text | M6 as planned (history first) | — | — |
| B15 hints | Status line hints per `06 §2` | — | — |
| B7 inspector | `⌘⇧I` on a token: `man -w` section or `<cmd> --help` captured by the daemon (C4.2), rendered in a popover | e2e for `ls` | `06` |

## C. Windows, tabs, panes, layouts (M5.5 + M9, 3 weeks)

| Item | Mechanism | Tests | Docs |
|---|---|---|---|
| C1 tabs | `⌘1..9`, `⌘⇧{ }`, rename (double-click title or `tab.rename`), `[window] new_tab_placement`; reopen closed = reattach the most recently detached session (`session.list` sorted by detach time), `⌘⇧T` | Swift keymap tests | `06 §7`, `09` |
| C2 colours/groups/pin | Tab colour per session stored in `vt-store` session metadata (`session.meta {color}`), rendered in the native tab (`NSWindow.tab.accessoryView`) and in the sidebar; groups and pins live in the sidebar model (C4); per-directory colours map in config | store test | `09` |
| C3 indicators | Sidebar/tab accessory shows: failed last command, synced input, zoomed pane, agent state glyph | Swift | `06 §2` |
| C4 sidebar = vertical tabs | `Sidebar/` SwiftUI panel: repo → worktree → session tree (`01` B1.5) where each row carries Warp's metadata: cwd, branch (daemon `git` probe via `vt-worktree`), diff stats (`git diff --shortstat`, throttled), PR badge (`gh pr view --json` when `gh` exists, opt-in), agent state badge, unread dot, last command/conversation title; search filter; density; hover sidecar; drag to reorder | daemon probe tests; Swift model tests | `06 §2`, `09` |
| C5 panes | Prev/next pane, keyboard resize via `NSSplitView` divider moves, drag pane header to another tab/window, `[panes] dim_inactive`, `focus_follows_mouse` | Swift | `06 §7` |
| C6 layouts (Tab Configs + B4.1) | `~/.config/vambiant-term/layouts/<name>.toml`: `name`, `title`, `color`, `[[panes]]` tree (`type = shell\|agent`, `agent = claude\|codex`, `directory`, `commands`, `shell`, `is_focused`, `split`, `children`), `[params]` (`text\|branch\|repo`, `{{autogenerated_branch_name}}` via `vt-worktree`); "Save as layout", default layout for `⌘T`, `vterm layout open <name>`, URI `vambiant-term://layout/<name>`; M7 wires `type = agent` + worktree creation | parse tests with the four Warp-shaped examples; e2e open | `09`, `02` |
| C7 windows level | `[[windows]]` wrapper in the same file (legacy Warp launch-config shape) | parse | `09` |
| C8 session palette | `sessions:` prefix lists running sessions with state, last command, cwd; `⌃Tab` MRU option `[keys] ctrl_tab = tabs\|mru` | Swift | `06 §7` |
| C9 restore | M9: `vt-store` layout snapshot per window on change; on launch reattach; blocks come from the daemon | e2e restart | `07` M9 |
| C10 undo close | Covered by C1 reopen | — | — |
| C11 quake window | M9 (C tier): `NSPanel` pinned edge, `[hotkey]` keys, Accessibility prompt | manual | `09` |
| C12 toolbar | After sidebar/inbox/review exist: order and side in `[window] toolbar` | — | `09` |
| C14 quit | One-time sheet "N sessions keep running; `vterm ls` to find them" | — | `06` |
| C15 window size/opacity | `[window] cols/rows` for new windows; opacity/blur via `NSVisualEffectView` (A2.9) | — | `09` |

## E. Terminal features, links, notifications (M5.5, 1.5 weeks)

| Item | Mechanism | Tests | Docs |
|---|---|---|---|
| E1 SGR | `Attrs` gains underline style (single/double/curly/dotted/dashed), underline colour, overline, blink; wire cell adds `ul: [u8;4]`; renderer draws styles, blink capped at 5 s (`[cursor]`-style setting) | snapshot fixtures | `08` |
| E2 mouse toggle | `[terminal] mouse_reporting` + palette toggle passed to the backend | — | `09` |
| E4 links | Path/URL detector over line text (`session.text`) on hover; `⌘`-click opens URL, or file in `[editor] program` (`$EDITOR`, `code`, `zed`, JetBrains `idea`, …) with `line:col` forms; Dock drop opens a tab at the folder; Finder service "New Vambiant Term tab here" | detector unit tests on the six suffix forms | `09` |
| E6 bell | `[terminal] bell = none\|sound\|flash`; `TermEvent::Bell` → `NSSound`/flash | — | `09` |
| E7 notifications | Daemon emits `session.event {command_finished, duration}` and `password_prompt` (heuristic on `[Pp]assword:` at the cursor with echo off, via the observer); app `UNUserNotificationCenter` when not frontmost, `[notifications] long_running_threshold_s = 30`, sound, toast duration; OSC 9/777 already parsed | observer tests | `06 §8`, `09` |
| E9 URI scheme | `vambiant-term://new-window?path=`, `new-tab?path=`, `layout/<name>`, `settings?q=`; `CFBundleURLTypes` in `Info.plist` | Swift URL parser test | `09`, ADR-0010 |
| E14 scrollback | `[terminal] scrollback_bytes` exposed | — | `09` |
| D8/D9 | Alt-screen padding key; `⌘+`/`⌘-`/`⌘0` font zoom | — | `09` |

## F. Nested shells and SSH (M-later, 2 weeks)

| Item | Mechanism | Tests | Docs |
|---|---|---|---|
| F1 subshells | Our snippets emit a private DCS `\eP$vt{"hook":"ready","shell":"zsh"}\e\\` from rc files when `VAMBIANT_TERM=1`; the daemon answers a nested shell by re-sending the integration through the PTY (sourced from a temp file it writes) only when the user accepted a per-command allowlist (`docker exec`, `poetry shell`, …) | e2e nested zsh | `10 §10` |
| F2 ssh | Detect an interactive `ssh` cmdline (B→C) → banner "enable blocks on this host"; on accept, type a one-line bootstrap that `curl`-free heredocs the snippet into `$TMPDIR` and sources it; no remote daemon, no ports; `[shell_integration] ssh = ask\|always\|never`, host denylist | manual + e2e against `ssh localhost` when available | `10 §10`, `05` (what the bootstrap can see) |

## G. Agent supervision parity (M5 remainder + M10, 5 weeks)

| Item | Mechanism | Tests | Docs |
|---|---|---|---|
| G2 detection | Generic adapter matches argv/process names: `claude, codex, opencode, amp, auggie, copilot, cursor-agent, gemini, droid, pi, goose, agy, hermes, vibe`; per-agent regex packs as today | fixture tests | `03` |
| G3 notifications | Inbox history becomes the **mailbox** (`06 §3`): All/Unread/Errors, `↑↓ ↩ ⇧Tab Esc`; toasts (max 2, hover pauses) for other-tab events; tab/sidebar badges; event kinds complete/request/error from adapters | Swift model tests; e2e events | `06 §3`, `06 §8` |
| G4 composer | `⌃G` opens a `ComposerView` (NSTextView) above the pane: multi-line, `@` picker over `git ls-files` + symbols (ctags-free: regex outline for common languages), image → saved to `$TMPDIR` and its path inserted, `/` local prompts; submit sends via bracketed paste + `\r`; auto-show when the session enters `awaiting_input` (adapter state, not a plugin); `[agents] composer_auto = true` | Swift tests for `@` resolution and submit encoding | `06 §2`, `03` |
| G5 context | "Attach block" (`⌘↑` while composer open) pastes `session.text` of the block as a fenced quote; "attach selection" (`⌘L`) same; URLs are the agent's job | Swift | `06` |
| G6 review comments | From H2: comments `[{file, line, text}]` rendered as a Markdown list and pasted into the agent's composer (G4) | Swift | `06` |
| G7 profiles | `vt-policy` named profiles in `policy.toml`: per action `ask\|allow\|deny\|agent_decides→ask`, regex allow/deny lists (deny wins), MCP allow/deny; per-session "run until done" sets `allow` for allowlisted classes only; **floor is unbypassable** (ADR-0009) | policy tests incl. floor-vs-auto-approve | `05`, ADR-0009 amendment |
| G8–G10 inspectors | Read-only "Applies here" panel: rule files (`AGENTS.md`, `CLAUDE.md`, `WARP.md`, `.cursorrules`, …) up the tree, skills dirs (ten vendor paths), MCP configs (`~/.claude.json`, `.mcp.json`, `~/.codex/config.toml`, `.agents/.mcp.json`); project-scoped MCP marked "not trusted until you say so" but we never spawn servers | path discovery tests | `03` |
| G11 events | Render `TodoWrite` (task list ● ✔ ○ ■), plan documents, `AskUserQuestion` (numbered options card answering into the inbox), thinking (collapsed), compaction markers; prompt queue per session: `session.queue {id, text}` sent when state becomes `idle`/`awaiting_input`, panel with reorder/edit/delete, `↩` on empty input sends next | adapter fixture replays; e2e queue | `03`, `06 §4` |
| G14 voice | C tier, on-device only, push-to-talk `[agents] voice_key` | manual | `05` |
| G15 routers | C3.6 gains `[[routes]] when = "complexity:easy" \| "match: <regex on prompt>"` → model; complexity classifier is local (length/keywords) not a model call | provider tests | `04` |
| G16 active AI | M6 rows C1.1/C1.6/C1.7 | — | — |
| D2 ACP pane | If accepted: `vt-acp` client (JSON-RPC over stdio per the ACP spec), conversation pane rendering ACP `session/update` events with the same block renderers as G11 | fixture replays | ADR-0011 |

## H. Code review panel (M7, 2 weeks, needs D3)

`vt-git` (libgit2 via `git2`, or shelling to `git`): `review.diff {repo, base: worktree\|main\|<branch>}` → files/hunks; `review.revert_hunk`, `review.discard_file`, `review.discard_all` (confirm). App: `ReviewPanel` with file list, hunk view (reuse B3.4 word-diff renderer), revert buttons, "open in editor" (E4), "attach as context", inline comments → G6; git chip in the status line (branch, files, +/−) opens it; refresh on FS events. Tests: fixture repos; e2e revert. Docs: `06 §2`, new `01` row H2.

## K. Keymap resolution (K1, with `06 §7` amendment)

Adopt Warp's block chords where ours are free, keep ours where `06` already decided:

| Chord | Warp | Ours today | Proposed |
|---|---|---|---|
| `⌘↑ / ⌘↓` | select prev/next block | previous/next prompt | keep: prompt jump also selects that block |
| `⌘⇧↑ / ⌘⇧↓` | scroll to top/bottom of selected block | select prev/next block | swap to Warp's; block select moves to `⌥⌘↑/↓` |
| `⌘K` | clear blocks | AI ask | keep AI (`06 §7`); clear = `⌘⇧K` |
| `⌘I` | re-input | unbound | re-input |
| `⌘B`, `⌥↑/↓` | bookmark, jump | unbound | adopt |
| `⌘F`, `⌘G`, `⇧⌘G` | find | `scrollback.search` unbuilt | adopt |
| `⌘⇧C`, `⌥⇧⌘C` | copy command / output | menu only | adopt |
| `⌃R` | history | unbound | adopt |
| `⌘[`, `⌘]`, `⌃⌘arrows` | pane prev/next, resize | unbound | adopt |
| `⌘1..9`, `⌘⇧T` | tabs | unbound | adopt |
| `⌘⇧P` | palette | palette (unbuilt) | same |
| `⌃G` | (editor) | unbound | composer |

## S. Sequencing and estimates

| Order | Package | Contents | Weeks | Exit check |
|---|---|---|---|---|
| 1 | M5 finish | A2–A15, G3 mailbox/toasts/badges, G11 event blocks, C4 sidebar, cost meters, inbox sheet | 4 | `01` B3/B2 exits; every Warp block action has an equivalent or an export |
| 2 | M5.5 input & shell parity | B4, B5, B8, B9, B10, B11, B12, B7; C1–C3, C5, C8, C14; E1, E2, E4, E6, E7, E9, E14, D8, D9 | 5 | Typing `gti status` shows an underline and a correction; `⌃R` finds a command by exit code; links open |
| 3 | M6 intelligence | as `07` + G16 | 3–4 | unchanged |
| 4 | M7 worktrees + layouts + review | C6, C7, H2, G6, B4 rows | 4 | `07` M7 exit + a layout with `{{autogenerated_branch_name}}` opens an agent in a fresh tree |
| 5 | M8 autonomy | G7 profiles on top of the engine | 3 | dry-run week; auto-approve provably cannot cross the floor |
| 6 | M10 agent composer | G2, G4, G5, G8–G10, prompt queue, D2 ACP pane if accepted | 4 | Claude Code session: `⌃G`, `@file`, image, block attach, queued prompt delivered on idle |
| 7 | M9 polish | C9, C11, C12, C15, D1 themes import incl. Warp YAML, D2 text options, E10 a11y, JSON schema | 3 | unchanged |
| 8 | M11 remote | F1, F2 | 2 | blocks inside `docker exec` and `ssh` sessions |

Total ≈ 28 weeks on top of the existing plan's M6–M9, of which ≈ 12 are net-new scope (packages 2, 6, 8 and the H2/C6 halves of 4).

## Docs to amend when packages land

`01` (new rows A5 A7 A9 A11 A12 A13 B4 B5 B8 B9 B12 C2 C3 C6 E7 E9 G3 G4 G7 H2 with tiers), `06 §2/§4/§7`, `09` (every new key above), `02` (RPC list), `03` (adapters, inspectors), `05` (SSH bootstrap, voice), `10 §10` (private OSC/DCS), ADR-0009 amendment (profiles vs floor), ADR-0011 (ACP pane, if D2), ADR-0010 (URL scheme).
