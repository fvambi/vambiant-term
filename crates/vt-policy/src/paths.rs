//! Token-level helpers the checks share: secret-looking paths, system
//! paths, hosts in arguments, protected-branch globs.

use std::path::Path;

/// File names and paths that hold credentials (docs/05 §5.2, §4.1).
#[allow(clippy::case_sensitive_file_extension_comparisons)] // `lower` is lowercased first
pub(crate) fn is_secret_path(raw: &str) -> bool {
    let path = raw.trim_end_matches('/');
    let name = path.rsplit('/').next().unwrap_or(path);
    let lower = name.to_ascii_lowercase();
    let by_name = lower == ".env"
        || lower.starts_with(".env.")
        || lower.ends_with(".env")
        || lower.ends_with(".pem")
        || lower.ends_with(".key")
        || lower.ends_with(".p12")
        || lower.ends_with(".pfx")
        || lower.ends_with(".jks")
        || lower.ends_with(".keystore")
        || lower.ends_with(".kdbx")
        || lower.ends_with(".tfstate")
        || lower.ends_with(".keychain")
        || lower.ends_with(".keychain-db")
        || lower.ends_with(".secret")
        || lower.starts_with("secrets.")
        || lower.starts_with("id_rsa")
        || lower.starts_with("id_dsa")
        || lower.starts_with("id_ecdsa")
        || lower.starts_with("id_ed25519")
        || lower.starts_with("service-account")
        || lower == "credentials"
        || lower == "credentials.json"
        || lower == ".netrc"
        || lower == "_netrc"
        || lower == ".npmrc"
        || lower == ".pypirc"
        || lower == ".git-credentials"
        || lower == "shadow" && path.starts_with("/etc");
    if by_name && !lower.ends_with(".pub") {
        return true;
    }
    let dirs = path.to_ascii_lowercase();
    let in_dir = |d: &str| dirs.contains(&format!("/{d}/")) || dirs.starts_with(&format!("{d}/"));
    (in_dir(".ssh") && !lower.ends_with(".pub") && lower != "known_hosts" && lower != "config")
        || in_dir(".gnupg")
        || in_dir(".aws")
        || in_dir(".kube")
        || (in_dir(".docker") && lower == "config.json")
        || (in_dir(".config/gh") && lower == "hosts.yml")
        || (in_dir(".config/gcloud") && lower.contains("credential"))
}

/// Environment variable names that hold secrets.
pub(crate) fn is_secret_var(name: &str) -> bool {
    let n = name.to_ascii_uppercase();
    [
        "SECRET",
        "TOKEN",
        "PASSWORD",
        "PASSWD",
        "API_KEY",
        "APIKEY",
        "PRIVATE",
        "CREDENTIAL",
        "AUTH",
    ]
    .iter()
    .any(|k| n.contains(k))
}

/// Paths only root should write.
pub(crate) fn is_system_path(path: &Path) -> bool {
    let s = path.to_string_lossy();
    if s.starts_with("/private/tmp")
        || s.starts_with("/private/var/folders")
        || s.starts_with("/var/folders")
        || s.starts_with("/tmp")
    {
        return false;
    }
    [
        "/etc",
        "/usr",
        "/bin",
        "/sbin",
        "/System",
        "/Library",
        "/private",
        "/var",
        "/boot",
        "/opt/homebrew/bin",
    ]
    .iter()
    .any(|p| s == *p || s.starts_with(&format!("{p}/")))
}

/// The host an argument talks to: URLs, `user@host:`, `host::module`.
pub(crate) fn host_of(arg: &str) -> Option<String> {
    if let Some(rest) = arg.split_once("://").map(|(_, r)| r) {
        let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
        let host = authority.rsplit('@').next().unwrap_or(authority);
        let host = host.split(':').next().unwrap_or(host);
        return (!host.is_empty()).then(|| host.to_ascii_lowercase());
    }
    if let Some((_, rest)) = arg.split_once('@') {
        let host = rest.split([':', '/']).next().unwrap_or(rest);
        return (!host.is_empty() && !host.starts_with('-')).then(|| host.to_ascii_lowercase());
    }
    if let Some((host, _)) = arg.split_once("::") {
        return (!host.is_empty()).then(|| host.to_ascii_lowercase());
    }
    if let Some((host, _)) = arg.split_once(':')
        && !host.is_empty()
        && !host.contains('/')
        && host.contains('.')
    {
        return Some(host.to_ascii_lowercase());
    }
    None
}

/// Whether `branch` matches one of the protected globs (`release/*`).
pub(crate) fn is_protected(branch: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|p| glob_match(p, branch))
}

fn glob_match(pattern: &str, text: &str) -> bool {
    fn go(p: &[char], t: &[char]) -> bool {
        match (p.first(), t.first()) {
            (None, None) => true,
            (Some('*'), _) => go(&p[1..], t) || (!t.is_empty() && go(p, &t[1..])),
            (Some('?'), Some(_)) => go(&p[1..], &t[1..]),
            (Some(a), Some(b)) if a == b => go(&p[1..], &t[1..]),
            _ => false,
        }
    }
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    go(&p, &t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_are_read_from_urls_and_scp_targets() {
        assert_eq!(
            host_of("https://api.example.com/v1"),
            Some("api.example.com".into())
        );
        assert_eq!(
            host_of("user@build.internal:/srv"),
            Some("build.internal".into())
        );
        assert_eq!(host_of("mirror::modules"), Some("mirror".into()));
        assert_eq!(host_of("./local:file"), None);
        assert_eq!(host_of("-H"), None);
    }

    #[test]
    fn secret_paths_and_globs() {
        assert!(is_secret_path(".env.local"));
        assert!(is_secret_path("~/.ssh/id_ed25519"));
        assert!(!is_secret_path("~/.ssh/id_ed25519.pub"));
        assert!(is_secret_path("/etc/shadow"));
        assert!(!is_secret_path("src/main.rs"));
        assert!(is_protected("release/2.0", &["release/*".into()]));
        assert!(!is_protected(
            "feature/x",
            &["main".into(), "release/*".into()]
        ));
    }
}
