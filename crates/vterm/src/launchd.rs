//! launchd LaunchAgent management for `vtermd` (ADR-0004: `KeepAlive`).
//!
//! The plist lives in `~/Library/LaunchAgents/com.vambiant.term.vtermd.plist`
//! and is loaded into the user's GUI domain with `launchctl bootstrap`, which
//! also starts it. Logs go to `~/.local/state/vambiant-term/logs/`.

use std::path::{Path, PathBuf};
use std::process::Command;

/// launchd label; the bundle id prefix is permanent (docs/00 §8).
pub const LABEL: &str = "com.vambiant.term.vtermd";

fn home() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from)
}

/// Where the plist is written.
pub fn plist_path() -> PathBuf {
    home()
        .join("Library/LaunchAgents")
        .join(format!("{LABEL}.plist"))
}

/// The vtermd binary: explicit, next to this executable, or on PATH.
pub fn vtermd_path(explicit: Option<&Path>) -> PathBuf {
    if let Some(p) = explicit {
        return p.to_path_buf();
    }
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("vtermd")))
        .filter(|p| p.exists())
        .unwrap_or_else(|| PathBuf::from("vtermd"))
}

fn domain() -> String {
    // SAFETY: getuid cannot fail and touches no memory.
    format!("gui/{}", unsafe { libc::getuid() })
}

fn launchctl(args: &[&str]) -> Result<(), String> {
    let out = Command::new("launchctl")
        .args(args)
        .output()
        .map_err(|e| format!("cannot run launchctl: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "launchctl {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// Write the plist and bootstrap it (starts vtermd immediately).
pub fn install(vtermd: Option<&Path>) -> Result<PathBuf, String> {
    let bin = vtermd_path(vtermd);
    let bin = bin
        .canonicalize()
        .map_err(|e| format!("vtermd binary {} not found: {e}", bin.display()))?;
    let logs = home().join(".local/state/vambiant-term/logs");
    std::fs::create_dir_all(&logs).map_err(|e| format!("cannot create {}: {e}", logs.display()))?;
    let plist = plist_path();
    if let Some(dir) = plist.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    let body = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{LABEL}</string>
  <key>ProgramArguments</key>
  <array><string>{bin}</string></array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>ProcessType</key><string>Interactive</string>
  <key>StandardOutPath</key><string>{logs}/vtermd.out.log</string>
  <key>StandardErrorPath</key><string>{logs}/vtermd.err.log</string>
</dict>
</plist>
"#,
        bin = bin.display(),
        logs = logs.display()
    );
    if plist.exists() {
        let _ = launchctl(&["bootout", &domain(), &plist.display().to_string()]);
    }
    std::fs::write(&plist, body).map_err(|e| format!("cannot write {}: {e}", plist.display()))?;
    launchctl(&["bootstrap", &domain(), &plist.display().to_string()])?;
    Ok(plist)
}

/// Start (or restart) the installed agent.
pub fn kickstart() -> Result<(), String> {
    launchctl(&["kickstart", "-k", &format!("{}/{LABEL}", domain())])
}

/// Stop the installed agent without removing it.
pub fn stop() -> Result<(), String> {
    if !plist_path().exists() {
        return Err("vtermd is not installed as a LaunchAgent (`vterm daemon install`)".into());
    }
    launchctl(&["bootout", &format!("{}/{LABEL}", domain())])
}

/// Bootout and delete the plist.
pub fn uninstall() -> Result<(), String> {
    let plist = plist_path();
    if !plist.exists() {
        return Err(format!(
            "nothing to remove: {} does not exist",
            plist.display()
        ));
    }
    let _ = launchctl(&["bootout", &domain(), &plist.display().to_string()]);
    std::fs::remove_file(&plist).map_err(|e| format!("cannot remove {}: {e}", plist.display()))
}
