//! Configuration (docs/09): TOML, hot-reloaded, schema-validated.
//!
//! An invalid config surfaces an error naming file, line and key — it
//! **never** silently falls back to defaults. `providers.toml` carries no
//! secrets; keys live in the Keychain.

pub mod keymap;
pub mod load;
pub mod reload;
pub mod schema;
pub mod theme_import;
