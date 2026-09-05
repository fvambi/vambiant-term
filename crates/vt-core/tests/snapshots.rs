//! Snapshot fixtures: `tests/fixtures/vt/<case>/input.vt` replayed through the
//! primary backend and compared with `<case>/expected.txt`. Set
//! `VT_UPDATE_SNAPSHOTS=1` to rewrite expectations after a deliberate change.

#![cfg(feature = "ghostty")]

use std::fs;
use std::path::{Path, PathBuf};

use vt_core::backend::GhosttyCore;
use vt_core::cell::GridSize;
use vt_core::harness::replay;

fn fixtures_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/vt")
}

fn size_for(case: &Path) -> GridSize {
    // Optional `size` file: "<cols>x<rows>"; default 80x24.
    fs::read_to_string(case.join("size"))
        .ok()
        .and_then(|s| {
            let (c, r) = s.trim().split_once('x')?;
            Some(GridSize {
                cols: c.parse().ok()?,
                rows: r.parse().ok()?,
            })
        })
        .unwrap_or(GridSize { cols: 80, rows: 24 })
}

#[test]
fn every_fixture_matches() {
    let root = fixtures_root();
    let mut cases: Vec<PathBuf> = fs::read_dir(&root)
        .expect("tests/fixtures/vt exists")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.join("input.vt").is_file())
        .collect();
    cases.sort();
    assert!(!cases.is_empty(), "no fixtures under {}", root.display());
    let update = std::env::var_os("VT_UPDATE_SNAPSHOTS").is_some();
    let mut failures = Vec::new();
    for case in cases {
        let input = fs::read(case.join("input.vt")).unwrap();
        let mut core = GhosttyCore::new(size_for(&case)).unwrap();
        let actual = replay(&mut core, &input);
        let expected_path = case.join("expected.txt");
        if update || !expected_path.exists() {
            fs::write(&expected_path, &actual).unwrap();
            continue;
        }
        let expected = fs::read_to_string(&expected_path).unwrap();
        if expected != actual {
            failures.push(format!(
                "{}:\n--- expected\n{expected}--- actual\n{actual}",
                case.file_name().unwrap().to_string_lossy()
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "snapshot mismatches:\n{}",
        failures.join("\n")
    );
}
