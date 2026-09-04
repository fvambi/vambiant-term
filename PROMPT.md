# Claude Code kickoff prompt — Vambiant Term

Paste everything below the line into Claude Code from
`~/Documents/Projects/DEVTOOLS/vambiant-term`.

Recommended invocation:

```bash
cd ~/Documents/Projects/DEVTOOLS/vambiant-term
claude --permission-mode plan
```

Start in plan mode. M0 is verification work whose whole purpose is to find out where the spec is wrong — do not let it start writing product code before it has reported back.

---

You are implementing **Vambiant Term**, a native macOS terminal emulator that supervises AI coding agents. The full specification already exists in this repository. Read it before doing anything else:

1. `CLAUDE.md` — the working agreement. These rules are binding.
2. `docs/00-product-brief.md` — what we're building and every decision already locked.
3. `docs/07-implementation-plan.md` — the milestone schedule. **You are starting at M0.**
4. `docs/10-research-notes.md` — every external fact this design rests on, with sources, and an explicit list of what is still unverified.
5. `docs/02-architecture.md` and `docs/adr/` — the structure and why it is that way.

Read `docs/01`, `03`, `04`, `05`, `06`, `08`, `09` as your task requires them. Do not skim `10` — it is the list of things that will bite you.

## My environment

- macOS on Apple Silicon. Ghostty is my current terminal; Vambiant Term is meant to replace it.
- `mise` manages my toolchains — make `mise.toml` the source of truth for versions, and every task a `mise run <task>`.
- `rustup`/`cargo` installed. Full Xcode is being installed for this project (Metal + signing).
- `SwiftLint`, `SwiftFormat`, `gh` installed. I use git worktrees routinely.
- Docker/OrbStack available, but **only use it if something genuinely cannot run natively.** Nothing in this project should need it.
- **Claude Code and Codex CLI are both installed** and are the two agents to target first.
- **No local model runtime is installed yet.** Where the spec assumes one (inline suggestions), design for it and tell me the exact install command rather than assuming it is there.
- Everything stays local. No paid services beyond the model APIs I configure myself.

## Your task: milestone M0 — ground truth

M0 exists because this specification was written from documentation, and the two most important surfaces in it are documented by their own vendors as unstable. **Verify before building.** Deliverables:

### 1. Workspace scaffold
- Cargo workspace with the crate skeletons from `docs/02-architecture.md` §3. Each crate gets a real `lib.rs` with its module structure and doc comment, not a placeholder.
- `rust-toolchain.toml` pinned to stable ≥ **1.98.1** (1.98.1 fixed a vtable miscompilation — do not go lower). Edition 2024.
- `mise.toml` with tasks: `build`, `test`, `lint`, `bench`, `ci`, `release`.
- CI config running the `fast` and `full` job contents from `docs/08` §9, runnable locally via `mise run ci`.
- `cargo deny` configured.
- `.gitignore`, licence header policy, and a `tests/fixtures/` directory.

### 2. Verify the terminal core
- Spike `alacritty_terminal` 0.26.x against a real PTY: spawn, parse, damage, resize, reflow.
- **Confirm the actual `Term::damage()` / `TermDamage` API shapes** — `docs/10` flags these as unverified.
- **In parallel, spike `libghostty-vt` 0.2.1** and benchmark both on the same input corpus using Ghostty's methodology (pre-generated inputs, hyperfine, medians, serial runs — see `docs/08` §7).
- Write a go/no-go recommendation. ADR-0001 chose `alacritty_terminal` on maturity grounds and says explicitly that M0 is the moment to revisit it. If the benchmark says otherwise, say so.

### 3. Verify the Claude Code integration surface
Against the installed binary, not the docs:
- Install session-scoped hooks via a `--settings` file (never mutate my `~/.claude/settings.json`).
- Confirm every hook event listed in `docs/03` §4.2 actually fires. **Capture real payloads to `tests/fixtures/claude/`.**
- Confirm the `http` handler type works against a loopback receiver, including `allowedHttpHookUrls`.
- Install a status line command that also POSTs its stdin JSON to a socket; capture a real sample.
- Run `claude -p --include-hook-events --output-format stream-json` on a trivial task and capture the full stream.
- Check `claude agents --json` output shape.

### 4. Verify the Codex integration surface
- Connect to `codex app-server` over a Unix socket; capture real JSON-RPC traffic for `thread/start`, `turn/start`, `turn/interrupt`.
- Install Codex hooks via a profile we own (`--profile`), never my main config. Capture payloads to `tests/fixtures/codex/`.
- Capture a `codex exec --json` run.
- **Find out where Codex actually stores rollouts** — `docs/10` §7 flags this as unverified against first-party docs.

### 5. Report
Update `docs/10-research-notes.md` in place: for every item, an "as verified on <date>, version <x>" line, and corrections wherever reality differs from the doc. Then tell me, in your final message:
- What differs from the spec, and what that changes.
- Anything in `docs/10` §"Open items for M0" you could not resolve.
- Your go/no-go on the terminal core with the benchmark numbers.

## How I want you to work

- **Ask me when the spec is ambiguous.** Do not invent a behaviour and bury it in code. I would rather answer three questions than review a wrong abstraction.
- Commit in small, reviewable pieces. Conventional commit messages.
- Do not write product code beyond what M0 needs. No terminal UI, no provider clients, no adapters yet.
- If something in the spec is simply wrong, say so directly. The docs were written from research, not from running code — I expect corrections and I would rather have them now.
- When you finish M0, stop and report. Do not roll into M1.

## Things the spec cares about that are easy to get wrong

- Edition 2024 requires `#[unsafe(no_mangle)]`, not `#[no_mangle]`.
- Never parse agent transcript files as a mechanism. Both vendors document those formats as internal and version-unstable.
- Contract tests must assert that an unknown event or field produces a **warning and a degraded-but-correct event** — never a panic, never a silent drop.
- Anthropic returns HTTP 400 for non-default `temperature`/`top_p` on recent Opus models. Capability tables, not blind pass-through. (Relevant from M-AI, not M0 — but do not scaffold a provider trait that forgets it.)
- Permission prompts never time out. Any deferred decision needs a watchdog.

---

## Optional context to add before pasting

If you have MCP servers, skills or plugins installed that are relevant, mention them explicitly — for example a Rust docs MCP, a GitHub MCP beyond `gh`, a Sentry connector, or personal skills for commit style or ADR formatting. Claude Code will use them if it knows they exist. Likewise, if you have a preferred crate for anything listed in `docs/02` §9, say so up front rather than reviewing it in later.
