# 09 — Configuration Reference

TOML. Hot-reloaded on write. Schema-validated: an invalid config shows an error overlay naming the file, line and key — it never silently falls back to defaults, because a silent fallback is how you spend an hour wondering why a setting does nothing.

## File locations

| Path | Purpose |
|---|---|
| `~/.config/vambiant-term/config.toml` | Main config |
| `~/.config/vambiant-term/providers.toml` | Provider profiles — **no secrets** |
| `~/.config/vambiant-term/policy.toml` | Global safety + autonomy policy |
| `~/.config/vambiant-term/themes/*.toml` | Themes |
| `~/.config/vambiant-term/keymap.toml` | Keybinding overrides |
| `<repo>/.vambiant-term/policy.toml` | Per-repo policy — **narrows only** |
| `<repo>/.vambiant-term/context.md` | Project context appended to prompts |

Env overrides: `VAMBIANT_TERM_CONFIG`, `VAMBIANT_TERM_STATE`, `VAMBIANT_TERM_LOG`.

A JSON Schema is published at `schema/config.schema.json` for editor completion. *(Not yet generated; the daemon's `config.get` serves the same field metadata — see below.)*

> **As built (M4, 2026-09-05).** `vt-config` implements `config.toml`, `keymap.toml` and `themes/*.toml` with every default in this file; `vtermd` loads them at start, re-reads them within a second of a write (mtime poll), and serves them over JSON-RPC: `config.get` (files, parse errors with line numbers, validation warnings, resolved keymap, themes, per-field metadata, defaults), `config.set {key, value}` (edits the file in place — comments and layout survive — then re-validates the whole file and refuses to write if it does not pass), `config.keymap.set {chord, action?}`, `config.theme.save {theme}`, `config.reload`, and the `config.changed` notification. Both the app's Settings window (⌘, — bound as `settings.open`, an action this file's keymap table did not list) and `vterm config get|set|show|path|bind` / `vterm keys` go through those. The Settings window is generated from the field metadata, so a new key needs only a schema entry and a `describe.rs` line; `themes/*.toml` can be created and edited there (built-ins are duplicated first). Rules the types cannot express are checked in `Config::validate()`: `privacy.telemetry` must be `false`, `api.bind` must be loopback, `mux.prefix` must parse. **Every key is stored and validated; not every key is honoured yet.** Each field carries an `applied` marker (`now` or `later: <milestone>`) that the Settings window shows next to the control, so a setting that does nothing yet says so instead of pretending. Honoured now: all of `[font]` except ligatures/thin-strokes/cell-width, `[theme]`, `[window] padding`, `[cursor]`, `[terminal] shell`, `[mux]`, `[agents.*] binary`/`extra_args`, `[agents.generic]`, `[notifications] awaiting_input`, `[privacy] telemetry`/`egress_log_days`, `[storage]` retention and `prune_on_start`. The list of honoured keys is the `applied` field in `crates/vt-config/src/describe.rs`, kept in step with the schema by a test. `providers.toml` and `policy.toml` land with M-AI and M-SEC. Theme import from other terminals is not built; built-in themes are `vambiant-dark` (below) and `vambiant-light`.

## config.toml

```toml
# ─── Appearance ────────────────────────────────────────────────
[font]
family          = "Berkeley Mono"
fallback        = ["SF Mono", "Menlo"]   # CoreText cascade handles the rest
size            = 13.0
line_height     = 1.2
cell_width      = 1.0
ligatures       = true
# Terminal-safe default: these commonly shape unintentionally in code
ligature_disable = ["fl", "fi", "st"]
thin_strokes    = "auto"                  # auto | always | never
bold_is_bright  = false

[theme]
name            = "vambiant-dark"
light           = "vambiant-light"        # used when macOS is in light mode
follow_system   = true

[window]
padding         = { x = 8, y = 6 }
opacity         = 1.0
blur            = 0
decorations     = "native"                # native | none
tab_bar         = "native"
restore_session = true
quake           = { enabled = false, hotkey = "cmd+`" }

[cursor]
style           = "block"                 # block | bar | underline
blink           = true
blink_interval_ms = 600

# ─── Terminal behaviour ────────────────────────────────────────
[terminal]
scrollback_lines = 100000
shell            = ""                     # empty = user's login shell
term             = "vambiant-term"        # falls back to xterm-256color
kitty_keyboard   = true
confirm_close_with_running_process = true
copy_on_select   = false                  # mouse selection → clipboard on release
bell             = "sound"                # none | sound | flash

[terminal.osc]
clipboard_write  = true
clipboard_read   = false                  # exfiltration primitive — see 05-security-privacy
window_ops       = false
title_report     = false
hyperlinks       = true

[shell_integration]
enabled          = true
shells           = ["zsh", "fish", "bash"]
inject           = "auto"                 # auto | manual | off
warn_on_conflict = true                   # p10k/starship printing preexec hooks corrupt marks
ssh_wrap         = false
sudo_wrap        = false

# ─── Input ─────────────────────────────────────────────────────
[input]
mode             = "warp"                 # warp | classic (ADR-0011; applies to new sessions)

[editor]
program          = ""                     # opens ⌘-clicked files: code | zed | idea | nvim | …; empty = default app

# ─── Blocks ────────────────────────────────────────────────────
[blocks]
dividers         = true                   # hairline above each command block
failed_tint      = true                   # tint the rows of a non-zero exit
sticky_header    = true                   # pin a scrolled-off command line at the top

# ─── Multiplexer ───────────────────────────────────────────────
[mux]
keymap_profile   = "tmux"                 # tmux | macos | both
prefix           = "ctrl+b"
detach_on_close  = true                   # closing a window detaches, never kills
default_layout   = "single"

# ─── Agents ────────────────────────────────────────────────────
[agents]
auto_detect      = true
adopt_external   = true                   # adopt agents started outside Vambiant Term

[agents.claude]
binary           = "claude"
install_hooks    = true
install_statusline = true
resume_by        = "id"                   # never --continue; its skip rules are asymmetric
extra_args       = []

[agents.codex]
binary           = "codex"
transport        = "app-server"           # app-server | exec | hooks-only
extra_args       = []                     # e.g. ["--sandbox", "workspace-write"]; reach the thread the TUI starts
# No profile: as built in M3 the daemon starts a per-session `codex app-server`
# that reads your own config.toml, and the TUI attaches to it with --remote.
# Nothing is written to ~/.codex.

[agents.generic]
enabled          = true
packs            = ["aider", "gemini-cli", "opencode", "cursor-cli"]
# Pack files: the built-in `generic` pack ships in the binary; extra packs are
# `<state>/packs/<name>.json` (state dir: ~/.local/state/vambiant-term).

[notifications]
awaiting_input   = true
agent_finished   = "when_unfocused"       # always | when_unfocused | never
agent_crashed    = true
long_command_ms  = 30000
coalesce_window_ms = 10000

# ─── AI ────────────────────────────────────────────────────────
[ai]
enabled          = true
inline_suggest   = true
suggest_debounce_ms = 40
suggest_budget_ms   = 120                 # a late suggestion is discarded, not shown
explain_on_failure  = true

# A profile may name a `fallback` profile in providers.toml: tried when it is
# rate-limited (60 s breaker), down (after three attempts) or unreachable.
# Profile names come from providers.toml next to this file (`vterm ai doctor`
# prints its path); a bundled default set is used until you write one.
[ai.routes]
suggest   = "local-fast"
classify  = "none"                        # deterministic rules; model is a second opinion only
ask       = "claude-strong"
explain   = "claude-strong"
search    = "local-embed"

[ai.budget]
daily_usd        = 5.0
monthly_usd      = 60.0
hard_stop        = true                   # refuse, loudly. never degrade silently

[ai.context]
include_git       = true
include_last_blocks = 3
include_env       = false                 # off: env is the most common leak vector
include_help_text = true
max_bytes         = 8000

# ─── Privacy ───────────────────────────────────────────────────
[privacy]
egress_mode      = "redacted"             # none | redacted | full
redaction        = "always"               # always | cloud_only  (always is the only sane value)
show_payload_first_use = true
egress_log_days  = 365
telemetry        = false                  # not a setting, a statement. always false
update_check     = false

# ─── Data ──────────────────────────────────────────────────────
[storage]
block_retention_days   = 90
event_retention_days   = 90
scrollback_max_mb      = 2048
prune_on_start         = true

# ─── API ───────────────────────────────────────────────────────
[api]
enabled          = true
bind             = "127.0.0.1"            # never anything else
port             = 7433
# token lives in Keychain; rotate with `vterm api rotate-token`
```

## providers.toml

No secrets. Keys are referenced by Keychain account name and resolved at call time.

```toml
[[provider]]
id        = "claude-strong"
kind      = "anthropic"
base_url  = "https://api.anthropic.com"
key_ref   = "keychain:vambiant-term/anthropic"
model     = "claude-sonnet-5"
max_tokens = 2048
prompt_cache = true
cache_ttl    = "5m"                       # "5m" | "1h"

[[provider]]
id        = "openai-strong"
kind      = "openai-responses"
base_url  = "https://api.openai.com"
key_ref   = "keychain:vambiant-term/openai"
model     = "gpt-5.6-terra"

[[provider]]
id        = "local-fast"
kind      = "llamacpp"
base_url  = "http://127.0.0.1:8080"
model     = "qwen3.5-2b-base-q4"
endpoint  = "infill"                      # infill | messages | chat
keep_warm = true

[[provider]]
id        = "ollama-fast"
kind      = "ollama"
base_url  = "http://127.0.0.1:11434"
model     = "qwen3.5:2b"
keep_alive = -1                           # pin resident; load latency dominates otherwise

[[provider]]
id        = "gateway"
kind      = "openai-compatible"
base_url  = "https://gateway.internal.vambiant.com/v1"
key_ref   = "keychain:vambiant-term/gateway"
model     = "whatever-it-serves"

[[provider]]
id        = "local-embed"
kind      = "ollama"
base_url  = "http://127.0.0.1:11434"
model     = "nomic-embed-text"
task      = "embedding"

[fallback]
"claude-strong" = ["openai-strong", "local-fast"]
"local-fast"    = []                      # no cloud fallback for keystroke-rate features
```

`vterm ai doctor` resolves this file, queries `/v1/models` where supported, prints the capability table per provider, and warns about configured models the endpoint does not offer.

## policy.toml

```toml
[safety]
destructive        = "confirm"     # allow | confirm | block
irreversible_remote = "confirm"
credential_read    = "confirm"
network_egress     = "warn"
privilege          = "confirm"
obfuscated         = "block"
unparseable        = "confirm"     # can never be "allow" — enforced in code

protected_branches = ["main", "master", "develop", "release/*"]

[autonomy]
enabled     = false                # master switch. off.
dry_run     = true                 # log decisions without making them
watchdog    = false
loop_detect = false
task_queue  = false

# Rules are evaluated in order; first match wins.
# All of these are examples and all are inert while autonomy.enabled = false.
[[autonomy.rule]]
name    = "read-only tools in this repo"
match   = { repo = "vambiant-api", tool = ["Read", "Glob", "Grep"] }
decide  = "allow"

[[autonomy.rule]]
name    = "tests in a worktree"
match   = { tool = "Bash", command = "^(cargo test|npm test|pytest)\\b", in_worktree = true }
decide  = "allow"

[[autonomy.rule]]
name    = "writes outside the worktree always ask"
match   = { tool = ["Edit", "Write"], outside_worktree = true }
decide  = "ask"
```

### The never-auto floor

Hard-coded, not configurable, applied after every rule:

- `destructive` targeting a path outside the session's worktree
- anything classified `credential_read`
- anything classified `obfuscated`
- force-push to a protected branch
- **any command from a generic-adapter session** — heuristic observability is not a basis for autonomous consent
- any command whose shell parse failed

A policy file that tries to allow one of these is a validation error, reported at load, not silently ignored.

> **As built (2026-09-06):** `vt-policy` parses exactly this schema plus `[egress]` from docs/05 §4.1 (`mode`, `allow_providers`, `max_context_bytes`, `never_include.paths/commands`). `unparseable`, `obfuscated` and `credential_read` refuse `"allow"` at load (`PolicyError::FloorViolation`); a rule's `command` regex that does not compile is a load error too. Rule `match` keys are `repo`, `tool` (string or list), `command` (regex), `in_worktree`, `outside_worktree`; `decide` is `allow | ask | deny`. Evaluation: classify → floor → first matching rule → `ask` when nothing matches; a `[safety]` class set to `block` turns a rule's `allow` into `deny`. The daemon does not load the file yet.

`<repo>/.vambiant-term/policy.toml` uses the same schema and is **intersected** with the global one. It can narrow, never widen: a repo may set `egress_mode = "none"` when the global is `redacted`; it may not set `"full"`.

```toml
[privacy]
egress_mode = "none"

[egress.never_include]
paths    = [".env*", "secrets/**", "*.pem", "*.key", "terraform.tfstate"]
commands = ["env", "printenv", "op ", "vault ", "aws configure"]
```

## keymap.toml

```toml
profile = "tmux"

[bindings]
"cmd+shift+a"       = "inbox.open"
"cmd+shift+enter"   = "inbox.next_pending"
"cmd+k"             = "ai.ask"
"alt+e"             = "ai.explain_last_failure"
"alt+cmd+e"         = "ai.show_last_payload"
"cmd+shift+n"       = "task.new"
"cmd+."             = "agent.interrupt"
"prefix a"          = "inbox.open"
"prefix N"          = "task.new"
```

`vterm keys` prints the fully resolved map and flags conflicts — including conflicts against the shell's own bindings where it can detect them.

## Workflows

Parameterised saved commands in Warp's workflow YAML, read from `~/.config/vambiant-term/workflows/*.yaml` and `<repo>/.vambiant-term/workflows/*.yaml`, plus `~/.warp/workflows/` and `<repo>/.warp/workflows/` read-only for drop-in compatibility (as built 2026-09-06):

```yaml
name: Kill process on port
command: lsof -i tcp:{{port}} | awk 'NR!=1 {print $2}' | xargs kill
description: Kill a process running on a given port
tags: [unix, process]
arguments:
  - name: port
    description: The port number
    default_value: 8080
shells: [zsh, bash]
```

`{{name}}` placeholders are arguments; one the file does not declare is added without a default. `vterm workflow list`, `vterm workflow show <name> --arg port=3000` (prints, never runs), and the palette's `w:` scope, which asks for arguments in a sheet and stages the command into the editor.

## Themes

Native format is TOML. On first run the app scans for and offers to import existing themes from Ghostty, iTerm2 (`.itermcolors`), Alacritty and base16/tinted-theming.

```toml
name       = "vambiant-dark"
background = "#0d0f12"
foreground = "#d8dee9"
cursor     = "#7aa2f7"
selection  = "#2a2f3a"
normal  = { black = "#1a1d23", red = "#e06c75", green = "#98c379", yellow = "#e5c07b",
            blue = "#61afef", magenta = "#c678dd", cyan = "#56b6c2", white = "#abb2bf" }
bright  = { black = "#4b5263", red = "#ff7b86", green = "#a9d977", yellow = "#f0d18a",
            blue = "#79c0ff", magenta = "#d7a3ff", cyan = "#6fd3de", white = "#e6e9ef" }

# UI accents for chrome that isn't the grid
[ui]
accent   = "#7aa2f7"
warning  = "#e5c07b"
danger   = "#e06c75"
success  = "#98c379"
```

Imported themes are contrast-checked; a failure produces a warning, not a rejection.

> **As built (2026-09-06):** `vterm theme import <file> [--name] [--format warp|ghostty|alacritty|iterm2|base16]`, `config.theme.import`, and Settings → Themes → Import… (opens in `~/.warp/themes`). Warp YAML (`accent` becomes the cursor and UI accent; selection is a background/foreground blend since Warp has none), Ghostty (`palette = N=…`, `cursor-color`, `selection-background`), Alacritty TOML, iTerm2 `.itermcolors`, base16 YAML (the standard shell mapping). The first-run scan and Alacritty's legacy YAML are not built.
