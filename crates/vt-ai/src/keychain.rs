//! API keys live in the Keychain (docs/04 §8), never in a config file. The
//! lookup order for a profile is: its `api_key_env` environment variable
//! (CI, headless), then the Keychain generic password `com.vambiant.term`
//! / `<profile>`. Nothing here ever logs a key.

use security_framework::passwords::{
    delete_generic_password, get_generic_password, set_generic_password,
};

/// Keychain service name every profile's key is filed under.
pub const SERVICE: &str = "com.vambiant.term";

/// The key for `profile`, if any.
pub fn secret(profile: &str, env_var: Option<&str>) -> Option<String> {
    if let Some(var) = env_var
        && let Ok(v) = std::env::var(var)
        && !v.trim().is_empty()
    {
        return Some(v);
    }
    get_generic_password(SERVICE, profile)
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
        .filter(|s| !s.is_empty())
}

/// Saves (or replaces) the key for `profile`.
pub fn store(profile: &str, key: &str) -> Result<(), String> {
    set_generic_password(SERVICE, profile, key.as_bytes()).map_err(|e| format!("keychain: {e}"))
}

/// Deletes the key for `profile`; already-absent is not an error.
pub fn remove(profile: &str) -> Result<(), String> {
    match delete_generic_password(SERVICE, profile) {
        Ok(()) => Ok(()),
        Err(e) if e.code() == -25300 => Ok(()), // errSecItemNotFound: already gone
        Err(e) => Err(format!("keychain: {e}")),
    }
}
