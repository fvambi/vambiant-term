# ADR-0005 — The internal provider interface is Messages-shaped, not OpenAI-shaped

**Status:** Accepted · 2026-09-04

## Context
We need one abstraction over Anthropic, OpenAI, arbitrary OpenAI-compatible endpoints, and local runtimes. The obvious default is OpenAI Chat Completions because everything speaks it.

## Decision
Model the internal interface on the **Anthropic Messages API**; OpenAI becomes one adapter among several.

## Rationale
1. **`llama.cpp`'s server implements `POST /v1/messages` natively** (merged 2026-01-19), with correct Anthropic SSE events, tool blocks, vision and thinking. One well-built Messages client covers cloud Claude *and* the recommended local backend on the same code path.
2. **Prompt caching.** Anthropic's own OpenAI-compat shim ignores `cache_control`, and Anthropic documents that shim as "not considered a long-term or production-ready solution." Caching is the largest latency/cost win for a terminal that resends the same shell context constantly.
3. The Messages content-block model (text / tool_use / thinking) maps onto every other provider's output; the reverse is lossy.

## Consequences
- Every provider carries an explicit **`ProviderCaps`** including a `SamplingSupport` table. This is not optional: **Anthropic returns HTTP 400 for non-default `temperature`/`top_p` on Opus 4.7+.** The request builder drops unsupported params rather than passing through blindly.
- The safe OpenAI-compatible subset we target is `/v1/chat/completions` + `/v1/models` + SSE with `[DONE]`. Not `logprobs`, `logit_bias`, `n>1`, `seed`, strict `response_format`, or `stream_options.include_usage`.
- No model id is ever hardcoded. Ids live in `providers.toml`; `vterm ai doctor` checks them against the live `/v1/models`.
- **Fast mode is explicitly not used**: it raises output tokens/sec but does not improve TTFT, costs premium pricing, and invalidates the prompt cache on switch. Terminal completions are TTFT-bound.
