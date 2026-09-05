//! Hot reload by polling mtimes once a second: no watcher dependency,
//! no fs-events edge cases, and a second is well under the time it takes
//! to alt-tab back from an editor.

use std::path::Path;
use std::time::SystemTime;

use crate::load::Paths;

/// Tracks the files' modification times.
#[derive(Debug)]
pub struct Watcher {
    paths: Paths,
    stamps: Vec<Option<SystemTime>>,
}

fn stamp(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

impl Watcher {
    /// Starts from the current state.
    #[must_use]
    pub fn new(paths: Paths) -> Self {
        let stamps = Self::stamps_of(&paths);
        Self { paths, stamps }
    }

    fn stamps_of(paths: &Paths) -> Vec<Option<SystemTime>> {
        let mut v = vec![
            stamp(&paths.config),
            stamp(&paths.keymap),
            stamp(&paths.themes),
        ];
        if let Ok(entries) = std::fs::read_dir(&paths.themes) {
            let mut files: Vec<_> = entries.flatten().map(|e| e.path()).collect();
            files.sort();
            v.extend(files.iter().map(|p| stamp(p)));
        }
        v
    }

    /// True when something changed since the last call.
    pub fn poll(&mut self) -> bool {
        let now = Self::stamps_of(&self.paths);
        if now == self.stamps {
            return false;
        }
        self.stamps = now;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_writes_and_new_theme_files() {
        let dir = std::env::temp_dir().join(format!("vt-reload-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("themes")).unwrap();
        let paths = Paths::in_dir(&dir);
        let mut w = Watcher::new(paths.clone());
        assert!(!w.poll());
        std::fs::write(&paths.config, "[font]\nsize = 12.0\n").unwrap();
        assert!(w.poll());
        assert!(!w.poll());
        std::fs::write(paths.themes.join("x.toml"), "").unwrap();
        assert!(w.poll());
    }
}
