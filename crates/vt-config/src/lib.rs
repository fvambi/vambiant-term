//! Configuration (docs/09): TOML, hot-reloaded, schema-validated.
//!
//! An invalid config surfaces an error naming file, line and key — it
//! **never** silently falls back to defaults. `providers.toml` carries no
//! secrets; keys live in the Keychain. The settings UI is generated from
//! [`describe::fields`], which a test keeps identical to [`schema::Config`].

pub mod describe;
pub mod keymap;
pub mod load;
pub mod reload;
pub mod schema;
pub mod theme;
pub mod theme_import;

pub use load::{ConfigError, Loaded, Paths, load};
pub use schema::Config;
