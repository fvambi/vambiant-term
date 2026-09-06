//! The daemon's side of the provider layer: resolves a feature to a
//! profile (`config.toml` `[ai.routes]` first, then `providers.toml`),
//! fetches the key, builds the terminal context, **redacts every text
//! part or refuses**, runs the request, and records the egress.

use std::fmt::Write as _;
use std::sync::{Arc, Mutex, PoisonError};

use vt_ai::provider::{Message, Request};
use vt_ai::route::{Profile, ProvidersFile};
use vt_proto::jsonrpc::RpcError;
use vt_store::Store;
use vt_store::egress::EgressRecord;

use crate::registry::{Registry, now};

/// Where `providers.toml` lives: next to `config.toml`.
pub fn providers_path() -> std::path::PathBuf {
    vt_config::load::Paths::default_paths()
        .config
        .with_file_name("providers.toml")
}

fn internal(msg: impl Into<String>) -> RpcError {
    RpcError::new(RpcError::INTERNAL, msg.into())
}

/// The profile a feature routes to. `config.toml` wins when it names a
/// profile the file knows; "none" is an explicit refusal.
fn route<'a>(
    cfg: &vt_config::Config,
    file: &'a ProvidersFile,
    feature: &str,
) -> Result<&'a Profile, RpcError> {
    let routes = &cfg.ai.routes;
    let from_config = match feature {
        "suggest" => Some(routes.suggest.as_str()),
        "classify" => Some(routes.classify.as_str()),
        "ask" => Some(routes.ask.as_str()),
        "explain" => Some(routes.explain.as_str()),
        "search" => Some(routes.search.as_str()),
        _ => None,
    };
    if let Some(name) = from_config {
        if name == "none" {
            return Err(RpcError::new(
                RpcError::INVALID_PARAMS,
                format!("feature `{feature}` is routed to `none` in config.toml [ai.routes]"),
            ));
        }
        if let Some(p) = file.profile(name) {
            return Ok(p);
        }
    }
    file.route(feature).ok_or_else(|| {
        RpcError::new(
            RpcError::INVALID_PARAMS,
            format!(
                "no profile routes feature `{feature}`: set [ai.routes] {feature} in config.toml or [routes] in {}",
                providers_path().display()
            ),
        )
    })
}

/// Terminal context for a prompt: cwd and the last command blocks, each
/// with a bounded slice of output (docs/01 C4.2, C4.3).
fn context(registry: &Registry, store: &Store, session: Option<&str>, max_bytes: usize) -> String {
    let Some(id) = session else {
        return String::new();
    };
    let Some(handle) = registry.find(id) else {
        return String::new();
    };
    let info = handle
        .info
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let mut out = format!("Working directory: {}\n", info.cwd.display());
    let blocks = store.blocks(&info.id, 0, 10_000).unwrap_or_default();
    let commands: Vec<_> = blocks
        .iter()
        .filter(|b| matches!(b.block.kind, vt_blocks::BlockKind::Command { .. }))
        .rev()
        .take(3)
        .collect();
    for b in commands.into_iter().rev() {
        if let vt_blocks::BlockKind::Command { cmdline, exit } = &b.block.kind {
            let end = b.block.end_line.unwrap_or(b.block.start_line);
            let from = b.block.output_line.unwrap_or(b.block.start_line + 1);
            let output = if end > from {
                Registry::export(&handle, from, end - 1, vt_core::core::TextFormat::Plain)
                    .unwrap_or_default()
            } else {
                String::new()
            };
            let tail: String = output
                .lines()
                .rev()
                .take(40)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\n");
            let _ = write!(
                out,
                "\n$ {}\n{}\n[exit {}]\n",
                cmdline.as_deref().unwrap_or("(command line unknown)"),
                tail,
                exit.map_or("?".to_owned(), |e| e.to_string())
            );
        }
        if out.len() > max_bytes {
            break;
        }
    }
    if out.len() > max_bytes {
        out.truncate(max_bytes);
        out.push_str("\n[context truncated]");
    }
    out
}

/// `ai.ask`.
pub fn ask(
    registry: &Arc<Registry>,
    store: &Arc<Mutex<Store>>,
    prompt: &str,
    feature: &str,
    session: Option<&str>,
) -> Result<serde_json::Value, RpcError> {
    let cfg = registry.cfg();
    if !cfg.ai.enabled {
        return Err(RpcError::new(
            RpcError::INVALID_PARAMS,
            "AI features are off ([ai] enabled = false)",
        ));
    }
    let file = ProvidersFile::load(&providers_path()).map_err(internal)?;
    let profile = route(&cfg, &file, feature)?;
    let key = vt_ai::keychain::secret(&profile.name, profile.api_key_env.as_deref());
    let provider = vt_ai::route::build(profile, key);

    let ctx = {
        let store = store.lock().unwrap_or_else(PoisonError::into_inner);
        context(
            registry,
            &store,
            session,
            usize::try_from(cfg.ai.context.max_bytes).unwrap_or(8000),
        )
    };
    // Redaction runs on every part that leaves the machine. Any failure
    // means nothing is sent (ADR-0007).
    let system_text = format!(
        "You are the assistant inside Vambiant Term, a macOS terminal. Answer briefly and concretely. \
         When the answer is a shell command, give the command first on its own line.\n\n{ctx}"
    );
    let system = vt_redact::redact(&system_text).map_err(|e| internal(e.to_string()))?;
    let user = vt_redact::redact(prompt).map_err(|e| internal(e.to_string()))?;
    let redactions = u64::try_from(system.replacements + user.replacements).unwrap_or(u64::MAX);

    let req = Request {
        model: profile.model.clone(),
        system: Some(system.text),
        messages: vec![Message::user(user.text.clone())],
        tools: vec![],
        max_tokens: 1024,
        temperature: None,
        top_p: None,
        stop: vec![],
    };
    let bytes_sent = u64::try_from(serde_json::to_vec(&req).map_or(0, |v| v.len())).unwrap_or(0);
    let done = provider
        .stream(&req, &mut |_| {})
        .map_err(|e| RpcError::new(RpcError::INTERNAL, e.to_string()))?;
    let text: String = done
        .content
        .iter()
        .filter_map(|c| match c {
            vt_ai::Content::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("");
    let cost = file.pricing(&profile.model).estimate(done.usage);
    if let Ok(store) = store.lock() {
        let _ = store.record_egress(&EgressRecord {
            at: now(),
            provider: profile.name.clone(),
            model: profile.model.clone(),
            purpose: feature.to_owned(),
            bytes_sent,
            redactions,
            payload: None,
        });
    }
    Ok(serde_json::json!({
        "text": text,
        "profile": profile.name,
        "model": profile.model,
        "usage": done.usage,
        "cost_usd_estimate": cost,
        "redactions": redactions,
        "stop": done.stop,
    }))
}

/// `ai.doctor`: what is configured, what has a key, what answers.
pub fn doctor(registry: &Arc<Registry>) -> Result<serde_json::Value, RpcError> {
    let path = providers_path();
    let file = ProvidersFile::load(&path).map_err(internal)?;
    let cfg = registry.cfg();
    let profiles: Vec<serde_json::Value> = file
        .profiles
        .iter()
        .map(|p| {
            let key = vt_ai::keychain::secret(&p.name, p.api_key_env.as_deref());
            let has_key = key.is_some();
            let provider = vt_ai::route::build(p, key);
            let (reachable, detail) = match provider.models() {
                Ok(models) => (true, serde_json::json!(models)),
                Err(e) => (false, serde_json::json!(e.to_string())),
            };
            serde_json::json!({
                "name": p.name, "kind": p.kind, "base_url": p.base_url, "model": p.model,
                "key": has_key, "reachable": reachable,
                "models": if reachable { detail.clone() } else { serde_json::Value::Null },
                "error": if reachable { serde_json::Value::Null } else { detail },
                "start_hint": match p.kind {
                    vt_ai::route::Kind::Ollama => vt_ai::local::start_hint("ollama"),
                    vt_ai::route::Kind::Llamacpp => vt_ai::local::start_hint("llamacpp"),
                    _ => "",
                },
            })
        })
        .collect();
    Ok(serde_json::json!({
        "providers_path": path.display().to_string(),
        "exists": path.exists(),
        "profiles": profiles,
        "routes": {
            "suggest": cfg.ai.routes.suggest, "classify": cfg.ai.routes.classify,
            "ask": cfg.ai.routes.ask, "explain": cfg.ai.routes.explain, "search": cfg.ai.routes.search,
        },
    }))
}
