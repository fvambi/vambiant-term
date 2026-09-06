//! `correct.suggest`: the session's last failed command block, its output
//! tail and what the shell can see (executables on PATH, entries of the
//! cwd) go through `vt_blocks::correct`. Suggestions are data; nothing
//! here runs a command.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use vt_store::Store;

use crate::registry::Registry;

/// When PATH was last scanned, and what it held.
type PathCache = Mutex<(Option<Instant>, Arc<Vec<String>>)>;

/// Executable names on this process's PATH, rescanned every minute.
pub(crate) fn executables() -> Arc<Vec<String>> {
    static CACHE: OnceLock<PathCache> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new((None, Arc::new(Vec::new()))));
    let mut guard = cache.lock().unwrap_or_else(PoisonError::into_inner);
    if guard
        .0
        .is_none_or(|t| t.elapsed() > Duration::from_secs(60))
        || guard.1.is_empty()
    {
        let mut names = BTreeSet::new();
        for dir in std::env::var("PATH").unwrap_or_default().split(':') {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for e in entries.flatten() {
                    if e.file_type().is_ok_and(|t| !t.is_dir()) {
                        names.insert(e.file_name().to_string_lossy().into_owned());
                    }
                }
            }
        }
        *guard = (Some(Instant::now()), Arc::new(names.into_iter().collect()));
    }
    Arc::clone(&guard.1)
}

/// The last failed command block and its output tail.
fn last_failure(
    registry: &Registry,
    store: &Store,
    session: &str,
) -> Option<(String, i32, String, std::path::PathBuf)> {
    let handle = registry.find(session)?;
    let info = handle
        .info
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let blocks = store.blocks(&info.id, 0, 10_000).ok()?;
    let last = blocks
        .iter()
        .rev()
        .find(|b| matches!(b.block.kind, vt_blocks::BlockKind::Command { .. }))?;
    let vt_blocks::BlockKind::Command { cmdline, exit } = &last.block.kind else {
        return None;
    };
    let exit = (*exit)?;
    if exit == 0 {
        return None;
    }
    let end = last.block.end_line.unwrap_or(last.block.start_line);
    let from = last.block.output_line.unwrap_or(last.block.start_line + 1);
    let output = if end > from {
        Registry::export(&handle, from, end - 1, vt_core::core::TextFormat::Plain)
            .unwrap_or_default()
    } else {
        String::new()
    };
    let tail: Vec<&str> = output.lines().rev().take(20).collect();
    let tail = tail.into_iter().rev().collect::<Vec<_>>().join("\n");
    Some((cmdline.clone()?, exit, tail, info.cwd))
}

/// `correct.suggest { session }`.
pub fn suggest(
    registry: &Arc<Registry>,
    store: &Arc<Mutex<Store>>,
    session: &str,
) -> serde_json::Value {
    let found = {
        let store = store.lock().unwrap_or_else(PoisonError::into_inner);
        last_failure(registry, &store, session)
    };
    let Some((cmdline, exit, output, cwd)) = found else {
        return serde_json::json!({ "corrections": [] });
    };
    let entries: Vec<String> = std::fs::read_dir(&cwd)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    let exes = executables();
    let corrections = vt_blocks::suggest(&vt_blocks::Failed {
        cmdline: &cmdline,
        exit,
        output: &output,
        executables: &exes,
        entries: &entries,
    });
    serde_json::json!({ "cmdline": cmdline, "exit": exit, "corrections": corrections })
}
