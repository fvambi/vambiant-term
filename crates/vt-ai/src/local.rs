//! Local runtimes. Ollama and LM Studio speak the OpenAI-compatible
//! subset; llama.cpp speaks Messages natively. Detection is a health probe
//! of the models endpoint, and the fix is printed as the exact command to
//! run (docs/04 §9), never assumed.

use crate::provider::Provider;

/// What `vterm ai doctor` prints for a local profile that is not answering.
pub fn start_hint(kind: &str) -> &'static str {
    match kind {
        "ollama" => "brew install ollama && brew services start ollama && ollama pull <model>",
        "llamacpp" => "brew install llama.cpp && llama-server -hf <repo> --port 8012",
        _ => "start the local server this profile points at, then run `vterm ai doctor` again",
    }
}

/// Reachable = the models endpoint answered. Local providers need no key.
pub fn reachable(p: &dyn Provider) -> Result<Vec<String>, String> {
    p.models().map_err(|e| e.to_string())
}
