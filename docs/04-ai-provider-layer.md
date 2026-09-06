# 04 — AI Provider Layer

## 1. The central decision: Messages-shaped, not OpenAI-shaped

The internal provider interface is modelled on the **Anthropic Messages API**, not OpenAI Chat Completions.

Reason: `llama.cpp`'s server now implements `POST /v1/messages` and `POST /v1/messages/count_tokens` natively, with correct Anthropic SSE event types, `tool_use`/`tool_result` blocks, vision and extended thinking (merged January 2026). So one well-built Messages client covers **cloud Claude and local llama.cpp on the same code path**. The OpenAI family then becomes one adapter rather than the substrate.

The secondary reason is prompt caching. Anthropic's OpenAI-compatibility shim silently ignores `cache_control` — and caching is the single largest latency and cost win for a terminal that resends the same shell context on every keystroke. Anthropic's own docs call that shim "not considered a long-term or production-ready solution." We use the native API.

```rust
#[async_trait]
pub trait Provider: Send + Sync {
    fn id(&self) -> &ProviderId;
    fn capabilities(&self) -> ProviderCaps;
    async fn complete(&self, req: CompletionRequest) -> Result<CompletionStream>;
    async fn count_tokens(&self, req: &CompletionRequest) -> Result<u64>;
    async fn models(&self) -> Result<Vec<ModelInfo>>;
    async fn health(&self) -> Health;
}

pub struct ProviderCaps {
    pub streaming: bool,
    pub prompt_caching: bool,
    pub tools: bool,
    pub sampling_params: SamplingSupport,  // ← see §5, this prevents an entire class of 400s
    pub json_schema_output: bool,
    pub fim: bool,                          // fill-in-the-middle
    pub max_context: u32,
    pub typical_ttft_ms: Option<u32>,       // measured, not claimed
}
```

## 2. Adapters

| Adapter | Endpoint | Notes |
|---|---|---|
| `anthropic` | `POST /v1/messages` | Native. Prompt caching, `count_tokens`, streaming SSE |
| `openai-responses` | `POST /v1/responses` | Typed semantic events (`response.output_text.delta`), no `[DONE]` sentinel |
| `openai-chat` | `POST /v1/chat/completions` | Still supported upstream; the compatibility lingua franca |
| `openai-compatible` | any `base_url` | OpenRouter, Groq, vLLM, LM Studio, LiteLLM, a Vambiant gateway |
| `ollama` | `/api/chat`, `/api/generate` (with `suffix` → FIM) | Native API is richer than its `/v1` shim |
| `llamacpp` | `/v1/messages` **or** `/infill` | Messages for chat, `/infill` for completion |
| `mlx` | `/v1/chat/completions` on :8080 | ⚠️ its docs say it "is not recommended for production as it only implements basic security checks" — bind loopback only |

### The safe OpenAI-compatible subset

Verified across Ollama, llama.cpp, LM Studio, vLLM, OpenRouter and Groq. Target only:

- `POST /v1/chat/completions` with `model`, `messages`, `max_tokens`, `temperature`, `top_p`, `stop`, `stream`
- `GET /v1/models` for discovery
- SSE with `choices[].delta.content` terminated by literal `data: [DONE]`
- `tools` + `tool_choice` as **best-effort** — `tool_choice` is unsupported on Ollama

Do **not** rely on: `logprobs` / `top_logprobs` (absent on Ollama, unsupported on Groq), `logit_bias` (unsupported on Groq), `n > 1` (Groq requires n=1), `seed`, strict `response_format` schemas, `stream_options.include_usage`. Groq silently rewrites `temperature: 0` → `1e-8`. Ollama's `/v1/completions` `prompt` takes strings only, not token arrays.

## 3. Feature → model routing

Routing is per-feature, configurable, with sane defaults. Each feature declares a latency budget and a quality floor.

| Feature | Budget | Default route | Rationale |
|---|---|---|---|
| Inline ghost text | **p50 < 120 ms to first token** | local small base model, KV-warm | Fires constantly. Must be free and instant or it is off |
| Command safety classification | **< 20 ms** | **no model** — deterministic rules | A regex that catches `rm -rf /` beats a model that usually does. Model is a *second* opinion only, async, never blocking |
| ⌘K natural language → command | p50 < 1.5 s | strong cloud (Sonnet-class), `effort: low`, thinking disabled | Quality matters, you are waiting on purpose |
| Explain failure | p50 < 2 s | strong cloud | |
| Explain output / selection | p50 < 3 s | strong cloud | |
| NL scrollback search | < 300 ms | local embeddings | |
| Block summarisation (background) | no budget | cheap cloud or local | |

Config expresses this directly:

```toml
[ai.routes]
suggest       = "local-fast"
classify      = "none"
ask           = "claude-strong"
explain       = "claude-strong"
search        = "local-embed"
```

### Latency levers that matter

- `output_config.effort = "low"` and `thinking = {type = "disabled"}` on Anthropic for command-shaped work.
- **Prompt caching**: `cache_control: {"type":"ephemeral"}` (5 min, 1.25× write) or `ttl: "1h"` (2× write); reads are 0.1×. **Max 4 breakpoints.** Minimum cacheable length is model-dependent — 512 tokens on the current Opus/Fable tier, 1024 on Sonnet, but **4096 on Haiku 4.5**, which makes small-prompt caching useless on the cheap tier. Design the shell-context prefix to exceed the floor of whichever model it targets, or don't cache it at all.
- **Anthropic "fast mode"** (`speed: "fast"` + beta header) raises output tokens/sec up to 2.5× but **explicitly does not improve TTFT**, costs premium pricing, and switching speed **invalidates the prompt cache** (separate prefix pools). Terminal completions are TTFT-bound. **Do not use it.** Documented here so nobody discovers it later and thinks it was an oversight.
- Local: keep the model resident (`keep_alive: -1` on Ollama), use `--cache-reuse` and `--spec-draft-model` on `llama-server`. Model load latency dominates everything else if you let the model unload.

## 4. Local model guidance

`llama-server` is the recommended local backend: it is the only option with a first-class `/infill` endpoint, KV-cache reuse across requests, grammar-constrained sampling, speculative decoding — and it doubles as a local Anthropic-Messages endpoint. Ollama is the easiest to support because users already have it. LM Studio is worth opportunistic support since it is often already running, and its REST v0 API exposes tokens/sec and **time-to-first-token**, which makes it a useful reference for our own telemetry.

Model choice for line completion (as of Sept 2026): the current small sweet spot is the **Qwen3.5 dense family** — `Qwen3.5-0.8B` / `-2B` / `-4B`, and specifically the **`-Base`** variants, because instruct-tuned models are the wrong shape for raw completion. **Gemma 4 E2B/E4B** with official QAT q4_0 builds is the other strong on-device candidate.

⚠️ Two things to verify empirically before committing (flagged in `10-research-notes.md`): FIM token support is **not documented** on the Qwen3.5-Base or Qwen3-Coder-Next model cards, and there is no credible 2026 TTFT benchmark for small models on M-series silicon. `Qwen2.5-Coder-1.5B` is the fallback precisely because its FIM tokens *are* documented. Measure before you choose.

Llama and Phi are **not** current choices in 2026 — Meta has published no models since late 2025 and Microsoft's recent releases are not Phi language models. Do not carry stale defaults forward.

## 5. Capability tables prevent an entire class of bugs

**`temperature`, `top_p` and `top_k` are deprecated on recent Anthropic Opus models — non-default values return HTTP 400.**

A naive shared abstraction that forwards sampling params to every provider will break on Anthropic and behave subtly differently on Groq. Therefore:

```rust
pub struct SamplingSupport {
    pub temperature: ParamSupport,  // Supported | DefaultOnly | Unsupported
    pub top_p: ParamSupport,
    pub top_k: ParamSupport,
    pub stop: ParamSupport,
    pub seed: ParamSupport,
}
```

The request builder **drops** unsupported params and logs a debug line. It never guesses, never passes through blindly, and `vterm ai doctor` prints the resolved table per configured provider so a surprise is one command away from an explanation.

## 6. Streaming

Normalise all three shapes into one internal stream:

- **Anthropic**: `message_start` → (`content_block_start` → `content_block_delta`* → `content_block_stop`)* → `message_delta`+ → `message_stop`, with `ping` interspersed. Delta kinds: `text_delta`, `input_json_delta` (**partial JSON fragments — accumulate the string, never parse per event**), `thinking_delta`, `signature_delta`. `message_delta` usage counts are cumulative. Tolerate unknown event types; the docs say so explicitly.
- **OpenAI Responses**: `response.created`, `response.output_text.delta`, `response.completed`, `error`. No `[DONE]`.
- **OpenAI Chat / compatible**: `choices[].delta` fragments, terminated by literal `data: [DONE]`.

Internal type:

```rust
enum Chunk { TextDelta(String), ToolInputDelta { id: String, json_fragment: String },
             ThinkingDelta(String), Usage(UsageDelta), Done(StopReason), Error(ProviderError) }
```

Cancellation is mandatory and immediate: one in-flight suggestion request per pane, aborted on the next keystroke. An orphaned stream is a cost leak and a latency lie.

## 7. Cost accounting and budgets

- Track input / output / cache-read / cache-write tokens per request, attributed to (feature, session, provider, model).
- Local models cost 0 but are still counted for latency telemetry.
- Rolling budgets: per-session, per-day, per-month. `budget.hard_stop = true` refuses further cloud calls and says so in the pane rather than silently degrading.
- Anthropic's `total_cost_usd` from agent runs is a **client-side estimate at list price** — display it labelled as an estimate.
- `vterm ai spend --by feature --since 7d` is a real command, not a nice-to-have. You cannot control what you cannot see.

> **As built (2026-09-06):** every egress record carries its list-price estimate and the session it was made for (store v8). `[ai.budget] daily_usd` / `monthly_usd` are checked in the daemon before a request is built; with `hard_stop = true` the request is refused with the numbers ("daily AI budget reached: $x of $y…") and the provider never sees it (e2e). Replies carry the budget status; the app writes it in the answer footer and raises a banner once at 80 % and at 100 %. `ai.spend { since, session }` and `vterm ai spend [--since 24h|7d|30d|2026-09] [--session]` total the log by purpose and provider. Per-session budgets and token attribution by feature are not built.

## 8. Credentials

- Keys live in the **Keychain**, never in `config.toml`. `providers.toml` holds `base_url`, model ids, routing — no secrets.
- The app is code-signed, so use `keyring` 4.x with `apple-native-keyring-store` and the **`protected`** feature (Apple's Protected Data store, iCloud-syncable across your Macs). Drop to `security-framework` directly if you want `SecAccessControl` biometric gating (`BIOMETRY_CURRENT_SET` + `USER_PRESENCE`) on key reads — the `keyring` abstraction does not expose it.
- ⚠️ Synchronized and non-synchronized Keychain items live in **completely different stores**; items are identified by service + account **plus** the sync flag. Choose one and be explicit — passing `None` behaves differently on get, set and delete.
- Never log a key, never include one in an error message, and wrap key material in `zeroize`.

## 9. Failure and degradation

| Failure | Behaviour |
|---|---|
| Timeout | Cancel, no retry for suggestions; one retry with jitter for ⌘K |
| 429 / rate limit | Circuit breaker opens for that provider; fall back down the route chain; show the reset time when the provider reports one |
| 5xx / overloaded | Exponential backoff, jittered, capped at 3 attempts |

> **As built (2026-09-06):** `vt_ai::resilience` — 5xx and transport errors retry on the same profile with jittered backoff (500 ms · 2ⁿ, three attempts); a 429 opens the profile's breaker for 60 s; the daemon then walks `fallback` (a new per-profile key in `providers.toml`, chains followed until one answers, never twice through the same profile) for `ai.ask`, streaming and Agent Mode's first turn. No retry once a chunk has been delivered: a duplicated answer is worse than a failed one. 4xx other than 429, a missing key and an unknown model give up at once. `vterm ai doctor` shows `fallback` and `cooling_down_secs`. Reset times from `Retry-After` are not read yet; the cooldown is fixed.
| Local model not running | Detect once, offer the exact command to start it, disable the feature until it is |
| Budget exceeded | Hard stop with a clear banner. Never a silent quality drop |
| Redaction failure | **Request is not sent.** Fail closed, always |

## 10. Model deprecation is a maintenance task, not a surprise

Model ids churn faster than this document. Two consequences:

1. Never hardcode a model id in code. Ids live in `providers.toml` with a documented default set, and `vterm ai models` lists what the configured endpoint actually offers via `/v1/models`.
2. `vterm ai doctor` warns when a configured model is missing from the provider's live list.

Concrete near-term item: **Haiku 4.5's retirement is "not sooner than 2026-10-15" with no announced successor at time of writing.** Anything that defaults to it needs a check before shipping.
