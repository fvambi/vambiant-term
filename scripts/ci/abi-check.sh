#!/usr/bin/env bash
# Regenerates crates/vt-ffi/include/vt_ffi.h with cbindgen and diffs it against
# the checked-in copy. An ABI drift is a memory-safety bug, not a compile error
# (CLAUDE.md), so drift fails the build. `--update` overwrites instead.
set -euo pipefail
cd "$(dirname "$0")/../.."
header=crates/vt-ffi/include/vt_ffi.h
tmp="$(mktemp -t vt_ffi.XXXXXX.h)"
trap 'rm -f "$tmp"' EXIT
cbindgen --config crates/vt-ffi/cbindgen.toml --crate vt-ffi --output "$tmp" --quiet
if [[ "${1:-}" == "--update" ]]; then
  cp "$tmp" "$header"; echo "abi-check: updated $header"; exit 0
fi
if ! diff -u "$header" "$tmp"; then
  echo "abi-check: $header is out of date. Run \`mise run abi:update\` and review the diff." >&2
  exit 1
fi
echo "abi-check: ok"
