#!/usr/bin/env bash
# `swift build` of the SwiftPM app. Skips loudly until app/ exists (M4).
set -euo pipefail
cd "$(dirname "$0")/../.."
if [[ ! -f app/Package.swift ]]; then
  echo "swift-build: SKIPPED — app/Package.swift does not exist yet (lands in M4)"
  exit 0
fi
if [[ "$(xcode-select -p)" == *CommandLineTools* ]]; then
  echo "swift-build: full Xcode required (Metal + signing); active developer dir is CommandLineTools" >&2
  exit 1
fi
(cd app && swift build -c release)
