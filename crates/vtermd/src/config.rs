//! The daemon owns the configuration: it reads the files, re-reads them
//! when they change (docs/09: hot-reloaded on write), validates edits
//! made from the settings window, and tells clients what changed.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use vt_config::load::{Loaded, Paths};
use vt_config::{Config, ConfigError};
use vt_proto::session::notification;

use crate::registry::Registry;

/// Loaded files plus their locations.
pub struct ConfigState {
    paths: Paths,
    loaded: Mutex<Loaded>,
}

impl ConfigState {
    /// Reads from the default (or `VAMBIANT_TERM_CONFIG`) directory.
    pub fn load_default() -> Arc<Self> {
        let paths = Paths::default_paths();
        let loaded = vt_config::load(&paths);
        Arc::new(Self {
            paths,
            loaded: Mutex::new(loaded),
        })
    }

    /// The current `config.toml` values (defaults when the file is bad —
    /// `loaded().config_error` says so).
    pub fn config(&self) -> Config {
        self.loaded
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .config
            .clone()
    }

    /// Everything, cloned.
    pub fn loaded(&self) -> Loaded {
        self.loaded
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Re-read the files.
    pub fn reload(&self) -> Loaded {
        let fresh = vt_config::load(&self.paths);
        *self.loaded.lock().unwrap_or_else(PoisonError::into_inner) = fresh.clone();
        fresh
    }

    /// What `config.get` returns: the files, plus the field metadata and
    /// action list the settings window is built from.
    pub fn describe(&self) -> serde_json::Value {
        let loaded = self.loaded();
        let actions: Vec<serde_json::Value> = vt_config::keymap::ACTIONS
            .iter()
            .map(|(id, label, milestone)| {
                serde_json::json!({ "id": id, "label": label, "milestone": milestone })
            })
            .collect();
        serde_json::json!({
            "paths": loaded.paths,
            "config": loaded.config,
            "config_error": loaded.config_error,
            "config_warnings": loaded.config_warnings,
            "keymap": loaded.keymap,
            "keymap_error": loaded.keymap_error,
            "themes": loaded.themes,
            "theme_warnings": loaded.theme_warnings,
            "fields": vt_config::describe::fields(),
            "actions": actions,
            "defaults": Config::default(),
        })
    }

    /// Edit one key of `config.toml`.
    pub fn set(&self, key: &str, value: &serde_json::Value) -> Result<Config, ConfigError> {
        let config = vt_config::load::set_config_value(&self.paths, key, value)?;
        self.reload();
        Ok(config)
    }

    /// Edit one binding of `keymap.toml`.
    pub fn set_binding(
        &self,
        chord: &str,
        action: Option<&str>,
    ) -> Result<vt_config::keymap::Resolved, ConfigError> {
        let config = self.config();
        let resolved = vt_config::load::set_keymap_binding(&self.paths, &config, chord, action)?;
        self.reload();
        Ok(resolved)
    }

    /// Write a theme file.
    pub fn save_theme(
        &self,
        theme: &vt_config::theme::Theme,
    ) -> Result<std::path::PathBuf, ConfigError> {
        let path = vt_config::load::save_theme(&self.paths, theme)?;
        self.reload();
        Ok(path)
    }

    /// The summary broadcast as `config.changed`.
    fn change_summary(loaded: &Loaded) -> serde_json::Value {
        serde_json::json!({
            "config_error": loaded.config_error,
            "config_warnings": loaded.config_warnings,
            "keymap_error": loaded.keymap_error,
            "theme_warnings": loaded.theme_warnings,
        })
    }

    /// Poll the files once a second and re-read on change.
    pub fn start_watch(self: &Arc<Self>, registry: Arc<Registry>) {
        let state = Arc::clone(self);
        let mut watcher = vt_config::reload::Watcher::new(self.paths.clone());
        let _ = std::thread::Builder::new()
            .name("config-watch".into())
            .spawn(move || {
                loop {
                    std::thread::sleep(Duration::from_secs(1));
                    if !watcher.poll() {
                        continue;
                    }
                    let loaded = state.reload();
                    if let Some(e) = &loaded.config_error {
                        eprintln!("vtermd: config: {e}");
                    }
                    for w in &loaded.config_warnings {
                        eprintln!("vtermd: config: {w}");
                    }
                    if let Some(server) = registry.server() {
                        server.broadcast(
                            notification::CONFIG_CHANGED,
                            Some(Self::change_summary(&loaded)),
                        );
                    }
                }
            });
    }
}
