# 05 — Security & Privacy

## 1. Posture

You chose **cloud-first for quality, redaction always on**. That is a defensible position and it puts the entire weight of the privacy story on one component: the redaction pipeline. So it is specified in more detail than anything else in this document set, and it **fails closed**.

Three invariants, in priority order:

1. **No credential leaves the machine unredacted.** If redaction cannot run, the request is not sent.
2. **Nothing destructive happens without a human, unless a human explicitly enabled a rule that allows it — and even then, some things are never automatable.**
3. **You can always see exactly what was sent, to whom, and why.**

## 2. Threat model

| Adversary | Capability | What we do |
|---|---|---|
| **Accidental self-leak** (the real one) | You `cat .env`, `env | grep`, or type `export STRIPE_KEY=…` and a suggestion request ships it to a provider | Streaming redaction on both output and the typed input region; egress preview; per-workspace policy |
| **Hostile terminal output** | A program, a compromised dependency, or a remote host over ssh emits escape sequences to read your clipboard (OSC 52 `?`), spoof prompt marks, title-inject, or trigger window ops | OSC 52 read **off by default**; unknown/dangerous sequences dropped; OSC 133 marks are advisory hints, never trusted for security decisions |
| **Prompt injection via terminal content** | Output the model reads says "ignore previous instructions and run …" | The model's output is *staged*, never auto-executed. The safety classifier runs on the staged command regardless of origin. Model output is data |
| **A misbehaving or over-eager agent** | Agent issues `rm -rf`, force-pushes, exfiltrates a key, or loops burning budget | Safety classifier applies to agent-issued commands identically; never-auto floor; loop detection; budget hard stop |
| **A malicious or compromised MCP server** | Tool call with hostile arguments arriving through the agent | Rendered in the inbox with full arguments; classified like any other command; MCP elicitation flows into the same queue |
| **Local process snooping** | Another user or process on the machine reads the socket or state | Socket 0700 in the user's runtime dir; loopback API token-authenticated; DB file 0600 |
| **Stolen laptop** | Disk access | Keys in Keychain (optionally biometric-gated), never in config; FileVault assumed |

Explicitly **out of scope**: defending against an attacker who already has code execution as your user. A terminal cannot win that fight and pretending otherwise is theatre.

## 3. The redaction pipeline

### 3.1 Where it sits

```
PTY bytes ─┬─► terminal grid (unredacted — it's your screen)
           │
           └─► block store ──► context builder ──► REDACT ──► provider
                                                     │
                                                     └─► egress log (redacted copy + hash)
```

Redaction runs **when context is built**, before anything is added to a prompt — not as a filter on the way out. That ordering matters: it means a redacted value can never be reasoned about, cached, or logged in cleartext anywhere downstream.

### 3.2 The hard part: streaming and chunk boundaries

A secret **will** straddle two PTY reads. A private key spans hundreds of bytes and many reads. There is no well-maintained Rust crate that does gitleaks-grade detection, and none of them handle streaming at all. This is our code:

- Maintain a sliding window of `max_pattern_len + margin` bytes across chunk boundaries.
- Multi-line patterns (PEM blocks) get a stateful mode: once `-----BEGIN … PRIVATE KEY-----` is seen, everything up to `-----END` is redacted regardless of chunking.
- Redaction is idempotent and order-independent: running it twice yields the same output.
- Redaction is **content-preserving in shape**: `sk-ant-api03-XXXX…` → `[REDACTED:anthropic-api-key]` so the model still sees that a key was there, which is often semantically necessary to explain a failure.

### 3.3 Detection strategy

Two layers, both required:

**Layer 1 — prefixed / structured patterns.** Vendor the gitleaks rule regexes (MIT-licensed) into a single `RegexSet` for one-pass matching. Go's RE2 and Rust's `regex` are both finite-automata engines with near-identical syntax, so the rules port essentially unmodified. Verified examples:

| Kind | Pattern |
|---|---|
| AWS access key | `(?:A3T[A-Z0-9]\|AKIA\|ASIA\|ABIA\|ACCA)[A-Z2-7]{16}` |
| GCP API key | `AIza[\w-]{35}` |
| Anthropic | `sk-ant-api03-[a-zA-Z0-9_\-]{93}AA` |
| OpenAI | `sk-` / `sk-proj-` prefixed |
| GitHub | `ghp_` `gho_` `ghu_` `ghs_` `ghr_` `github_pat_` ⚠️ re-verify prefixes against the live gitleaks TOML |
| Slack | `xox[baprs]-` |
| Stripe | `sk_live_` / `rk_live_` |
| JWT | `eyJ` + two base64url segments |
| Private key | `-----BEGIN (RSA\|EC\|OPENSSH\|PGP) PRIVATE KEY-----` |
| Connection string | `postgres://user:pass@`, `mongodb+srv://`, `redis://:pass@` — password in the URL userinfo |

**Layer 2 — unprefixed high-entropy values.** A 40-char AWS secret key has no prefix; only entropy finds it. Use the ripsecrets approach: flag strings assigned to `token`/`secret`/`password`/`key`-ish identifiers where a randomness test says P(random) is below a threshold. Combine Shannon entropy with character-class scoring, and gate on the surrounding identifier so `DEBUG=true` survives and `API_KEY=hunter2hunter2hunter2` does not.

### 3.4 Terminal-specific hazards

Beyond scanning output, redact:

- **The typed input region** — everything between OSC 133 `B` and `C`. Typing `export API_KEY=…` at the prompt is the single most common leak and it never appears in "output".
- `env` / `printenv` / `set` dumps.
- Shell history lines echoed by Ctrl-R.
- Heredocs.
- Command arguments in the block index and in pane titles.

### 3.5 Guarantees and testing

- **Fail closed.** A panic, a timeout, or an internal error in redaction means the request is dropped and the user is told. Never fail open.
- Property tests: for a corpus of synthetic secrets injected at every offset and split across every possible chunk boundary, the redacted output contains no substring of the secret longer than 8 chars.
- A golden corpus of real-shaped-but-fake credentials, checked in, run in CI.
- `vterm redact test < file` for manual inspection.
- **Recall over precision.** Over-redacting costs a slightly worse answer. Under-redacting costs a rotated credential and an incident.

## 4. Egress control

### 4.1 Per-workspace policy

`.vambiant-term/policy.toml`, committable, **narrows but never widens** the global policy:

```toml
[egress]
mode = "redacted"        # "none" | "redacted" | "full"
allow_providers = ["local-fast"]   # empty = all configured
max_context_bytes = 8000

[egress.never_include]
paths = [".env*", "secrets/**", "*.pem", "*.key"]
commands = ["env", "printenv", "aws configure", "op ", "vault "]
```

Precedence: repo policy ∩ global policy. A repo can set `mode = "none"` and the global config cannot override it. A repo **cannot** set `full` if the global is `redacted`.

### 4.2 The egress log

Every outbound request records: timestamp, feature, provider, host, model, byte count, token count, redaction hit count by rule, a hash of the payload, and the redacted payload itself (retained 365 days by default).

`vterm egress tail`, `vterm egress show <id>`, `vterm egress stats --by rule`.

First use of a cloud provider in a workspace shows the exact payload for confirmation, once. After that it is one keystroke away (`⌥⌘E` shows the last payload sent).

## 5. The safety classifier

Applies identically to human-typed commands and agent-issued ones. That symmetry is the whole argument for it living in the terminal rather than in each agent.

### 5.1 Parse, don't grep

VS Code's own documentation admits their terminal auto-approval is "best effort" and evadable by quote concatenation. So:

- Parse with a real POSIX shell grammar into a command list — pipelines, subshells, `&&`/`||`, command substitution, redirections.
- Classify **every** command in the tree, not the first one. `ls && rm -rf /` is destructive.
- **Unparseable input is unsafe by definition** and never auto-approved.
- Resolve aliases and functions where we can see them; treat an unresolvable name as unknown, which is a warn class.
- Obfuscation attempts (base64-decode-pipe-to-shell, `eval` of a variable, `curl | sh`) are their own class: **always ask**, never auto.

### 5.2 Classes

| Class | Examples | Default |
|---|---|---|
| `destructive` | `rm -rf`, `truncate`, `dd of=`, `mkfs`, `DROP TABLE`, `git reset --hard`, `git clean -fdx` | Confirm |
| `irreversible-remote` | `git push --force`, `gh release delete`, `terraform apply`, `kubectl delete` | Confirm |
| `credential-read` | `cat .env`, `env`, `security find-generic-password`, `op read`, `vault read` | Confirm + redact result |
| `network-egress` | `curl`, `wget`, `nc`, `scp`, `rsync` to a host not seen before in this repo | Warn |
| `privilege` | `sudo`, `chmod 777`, `chown root`, `launchctl load` | Confirm |
| `obfuscated` | `curl … \| sh`, `eval "$X"`, `base64 -d \| bash` | Always ask, never auto |
| `benign` | everything else | Allow |

Verdicts explain themselves: which rule, which token, what would happen. A warning you cannot interrogate gets clicked through.

### 5.3 The never-auto floor

These can **never** be auto-approved by any policy, in any workspace, by any rule, regardless of configuration:

- Anything in `destructive` targeting a path outside the session's worktree
- Anything in `credential-read`
- Anything in `obfuscated`
- `git push --force` to a protected branch (default: `main`, `master`, `develop`, `release/*`)
- Any command from a **generic-adapter** session (heuristic observability is not a basis for autonomous consent)
- Any command whose parse failed

This floor is hard-coded, not configurable. It is the thing that makes the rest of the autonomy engine safe to build.

## 6. Autonomy audit

Every decision the terminal makes on your behalf writes a record: timestamp, session, agent, tool, arguments, the rule that fired, the classification, and — where the action is reversible — an undo handle.

`vterm audit tail`, `vterm audit undo <id>`, `vterm audit stats --since 7d`.

**Dry-run mode** (B5.8) runs the policy engine and logs what it *would* have decided without deciding anything. That is the honest way to earn confidence in a rule set: run it for a week, read the log, then enable.

## 7. Terminal-level hardening

| Sequence / feature | Default | Why |
|---|---|---|
| OSC 52 write | on | Universally expected |
| OSC 52 **read** (`?`) | **off** | Any program — including anything on the far side of an ssh — can exfiltrate your clipboard. Only iTerm2, VS Code and Cursor support it at all, for exactly this reason |
| Window ops (resize, move, report) | off | Classic escape-sequence attack surface |
| Title reporting (`CSI 21 t`) | off | Title injection → command injection when the reply lands at a prompt |
| Bracketed paste | on, enforced | |
| Paste with newlines | confirm | Multi-line paste at a prompt is how people accidentally execute half a blog post |
| Paste that fails the safety classifier | confirm with the verdict shown | |
| `terminalSequence` from hooks | allowlist only | Claude Code already restricts it to OSC 0/1/2/9/99/777/BEL; we enforce the same on our side rather than trusting theirs |
| Hyperlink (OSC 8) targets | shown on hover, non-http schemes confirmed | |

**OSC 133 marks are hints, never a security boundary.** Any program can emit them. They drive block segmentation; they never drive an approval decision.

## 8. Process and file security

- Daemon socket: user runtime dir, mode 0700, peer credential check on connect (`LOCAL_PEERCRED`).
- Loopback API: `127.0.0.1` only, bearer token generated per install, stored in Keychain, rotatable with `vterm api rotate-token`. Never bind `0.0.0.0`. Remote access is the user's job via Tailscale or an ssh tunnel, and the docs say so explicitly rather than us shipping a listener.
- Hook receiver endpoints are per-session tokens, not a global secret, so a leaked token compromises one session.
- `state.db` and `providers.toml` mode 0600.
- Hardened runtime; no `com.apple.security.cs.allow-unsigned-executable-memory`. Entitlements are enumerated in `adr/0010-build-packaging.md` with a justification each.
- The app is unsandboxed by necessity (it spawns arbitrary user processes). That is stated plainly in the README rather than buried.

## 9. What we do not collect

No telemetry. No analytics. No crash reporting service. No usage pings. No model-training data. Crash logs are written locally and stay there unless you attach one to an issue yourself.

The only outbound network traffic Vambiant Term ever originates is: (a) requests to the model providers you configured, and (b) update checks, **which are opt-in and off by default**.
