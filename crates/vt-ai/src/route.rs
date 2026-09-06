//! `providers.toml`: named profiles, model ids, pricing and the
//! feature → profile routes (docs/04 §3, docs/09 `[ai.routes]`). No model
//! id lives in code; the bundled default file is data.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::anthropic::Anthropic;
use crate::cost::Pricing;
use crate::http::Http;
use crate::openai::OpenAiChat;
use crate::provider::Provider;

/// The file shipped when the user has none (`vterm ai doctor` writes it).
pub const DEFAULT_FILE: &str = include_str!("../providers.default.toml");

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(missing_docs)] // field names are the wire format's own vocabulary
pub enum Kind {
    Anthropic,
    Openai,
    Compat,
    Ollama,
    Llamacpp,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[allow(missing_docs)] // field names are the wire format's own vocabulary
pub struct Profile {
    pub name: String,
    pub kind: Kind,
    #[serde(default)]
    pub base_url: Option<String>,
    /// Default model for this profile.
    pub model: String,
    /// Every model id this profile may use (the default is added if absent).
    #[serde(default)]
    pub models: Vec<String>,
    /// Environment variable consulted before the Keychain.
    #[serde(default)]
    pub api_key_env: Option<String>,
    /// Models whose sampling parameters must stay at defaults (Anthropic).
    #[serde(default)]
    pub default_only_sampling: Vec<String>,
    /// The profile to try when this one is rate-limited, down, or cooling
    /// off (docs/04 §8); chains are followed until one answers.
    #[serde(default)]
    pub fallback: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[allow(missing_docs)] // field names are the wire format's own vocabulary
pub struct ProvidersFile {
    #[serde(default, rename = "profile")]
    pub profiles: Vec<Profile>,
    /// feature → profile name, or "none".
    #[serde(default)]
    pub routes: BTreeMap<String, String>,
    /// model id → pricing.
    #[serde(default)]
    pub pricing: BTreeMap<String, Pricing>,
}

impl ProvidersFile {
    /// Parses TOML and validates that every route names a profile.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut f: Self = toml::from_str(text).map_err(|e| format!("providers.toml: {e}"))?;
        for p in &mut f.profiles {
            if !p.models.contains(&p.model) {
                p.models.push(p.model.clone());
            }
        }
        let names: Vec<&str> = f.profiles.iter().map(|p| p.name.as_str()).collect();
        for (feature, target) in &f.routes {
            if target != "none" && !names.contains(&target.as_str()) {
                return Err(format!(
                    "providers.toml: route `{feature}` points at unknown profile `{target}`"
                ));
            }
        }
        Ok(f)
    }

    /// Reads `path`, or the bundled default when it does not exist.
    pub fn load(path: &Path) -> Result<Self, String> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::parse(DEFAULT_FILE),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }

    /// A profile by name.
    pub fn profile(&self, name: &str) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.name == name)
    }

    /// The profile a feature routes to; `None` when routed to "none".
    pub fn route(&self, feature: &str) -> Option<&Profile> {
        let target = self.routes.get(feature)?;
        if target == "none" {
            return None;
        }
        self.profile(target)
    }

    /// Pricing for a model, zero when unlisted (local models).
    pub fn pricing(&self, model: &str) -> Pricing {
        self.pricing.get(model).copied().unwrap_or_default()
    }
}

/// Builds the client for a profile; `key` comes from the Keychain layer.
pub fn build(profile: &Profile, key: Option<String>) -> Box<dyn Provider> {
    let http = Http::default();
    match profile.kind {
        Kind::Anthropic | Kind::Llamacpp => Box::new(Anthropic {
            name: profile.name.clone(),
            base_url: profile
                .base_url
                .clone()
                .unwrap_or_else(|| "https://api.anthropic.com".into()),
            api_key: key.or_else(|| matches!(profile.kind, Kind::Llamacpp).then(|| "local".into())),
            models: profile.models.clone(),
            default_only_sampling: profile.default_only_sampling.clone(),
            http,
        }),
        Kind::Openai | Kind::Compat | Kind::Ollama => Box::new(OpenAiChat {
            name: profile.name.clone(),
            base_url: profile
                .base_url
                .clone()
                .unwrap_or_else(|| match profile.kind {
                    Kind::Ollama => "http://127.0.0.1:11434".into(),
                    _ => "https://api.openai.com".into(),
                }),
            api_key: key,
            models: profile.models.clone(),
            key_required: matches!(profile.kind, Kind::Openai),
            http,
        }),
    }
}
