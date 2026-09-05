#!/usr/bin/env bash
# Builds the libghostty-vt spike binary (`--features ghostty`).
#
# Why a wrapper: libghostty-vt-sys 0.2.1 clones Ghostty (pinned commit) and runs
# `zig build`; Ghostty requires zig 0.15.2 exactly, and — as verified on
# 2026-09-04 — zig 0.15.2 cannot link against the macOS 26.5 Command Line Tools
# SDK: its libSystem.tbd no longer lists the arm64-macos target. zig locates
# the SDK via `xcrun --sdk macosx --show-sdk-path` (SDKROOT is ignored), so we
# put a shim first on PATH that reports the 15.4 SDK shipped in the same CLT.
# Remove this once Ghostty builds with zig ≥ 0.16.
set -euo pipefail
cd "$(dirname "$0")/../.."
sdk=${GHOSTTY_SDK:-/Library/Developer/CommandLineTools/SDKs/MacOSX15.4.sdk}
[[ -d $sdk ]] || { echo "build-ghostty: SDK not found: $sdk (set GHOSTTY_SDK)" >&2; exit 1; }
export GHOSTTY_SDK="$sdk"
# Hermetic source: the pinned Ghostty commit is vendored as a submodule, so the
# sys crate never clones at build time. `git submodule update --init` once.
if [[ -f third_party/ghostty/build.zig ]]; then
  export GHOSTTY_SOURCE_DIR="$PWD/third_party/ghostty"
else
  echo "build-ghostty: third_party/ghostty is empty; run: git submodule update --init third_party/ghostty" >&2
  exit 1
fi
# The sys crate maps cargo's DEBUG=true (any profile with debug info, including
# our release profile's `debug = 1`) to a Zig *Debug* build, which is ~100×
# slower and would make the benchmark a lie. Force ReleaseFast.
export LIBGHOSTTY_VT_SYS_OPTIMIZE=${LIBGHOSTTY_VT_SYS_OPTIMIZE:-ReleaseFast}
PATH="$PWD/scripts/bench/xcrun-shim:$PATH" cargo build --release -p term-core-spike --features ghostty --bin ghostty-spike "$@"
