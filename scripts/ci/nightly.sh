#!/usr/bin/env bash
# Nightly: everything `full` runs, plus what only makes sense on a schedule.
# The parts that are not built yet are announced as warnings on the run
# rather than pretending: fuzz targets (M1 backlog), ASan/TSan (M4 backlog),
# real-agent round trips (need `claude`/`codex` on the runner), leak tests.
set -euo pipefail
cd "$(dirname "$0")/../.."
mise run ci-full
for missing in "cargo-fuzz targets" "ASan/TSan runs" "real-agent round trips (no claude/codex on the runner)" "leak tests"; do
  echo "::warning title=nightly::not built yet: $missing"
  echo "nightly: NOT BUILT YET — $missing" >&2
done
