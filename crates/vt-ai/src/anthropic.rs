//! Anthropic Messages client: SSE streaming, prompt caching with breakpoint
//! budgeting (max 4), `count_tokens`. `input_json_delta` is accumulated,
//! not parsed per event; unknown SSE event types are tolerated.
