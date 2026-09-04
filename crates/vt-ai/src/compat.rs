//! Generic OpenAI-compatible adapter (Ollama, llama.cpp, LM Studio, vLLM,
//! OpenRouter, Groq). Avoids `logprobs`, `logit_bias`, `n>1`, `seed`,
//! strict `response_format` — the lowest common denominator (docs/10 §9).
