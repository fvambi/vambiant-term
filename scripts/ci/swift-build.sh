#!/usr/bin/env bash
# Build and test the SwiftPM shell. Works under the Command Line Tools
# alone (docs/10 §2): shaders compile at runtime, and Swift Testing is
# located by hand because the CLT install its frameworks outside the
# toolchain's default search path.
#
# The Rust static library must be built with an explicit
# `--crate-type staticlib`: with the crate's default [staticlib, rlib] and
# thin LTO, cargo emits LLVM bitcode members Apple's ld cannot read.
set -euo pipefail
cd "$(dirname "$0")/../.."
if [[ ! -f app/Package.swift ]]; then
  echo "swift-build: SKIPPED — app/Package.swift does not exist" >&2
  exit 1
fi
cargo rustc --release -p vt-ffi --lib --crate-type staticlib
cd app
swift build -c release
devdir="$(xcode-select -p)"
testflags=()
if [[ -d "$devdir/Library/Developer/Frameworks/Testing.framework" ]]; then
  f="$devdir/Library/Developer/Frameworks"
  l="$devdir/Library/Developer/usr/lib"
  testflags=(-Xswiftc "-F$f" -Xlinker "-F$f" -Xlinker -rpath -Xlinker "$f" -Xlinker -rpath -Xlinker "$l")
fi
swift test "${testflags[@]}"
