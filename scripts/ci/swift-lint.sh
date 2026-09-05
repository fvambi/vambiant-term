#!/usr/bin/env bash
# SwiftLint + SwiftFormat --lint over app/. Until M4 there is no app/, and a
# skipped step must say so loudly rather than pretend it passed.
set -euo pipefail
cd "$(dirname "$0")/../.."
if [[ ! -f app/Package.swift ]]; then
  echo "swift-lint: SKIPPED — app/Package.swift does not exist yet (lands in M4)"
  exit 0
fi
# SwiftLint needs sourcekitdInProc; under the Command Line Tools it only
# finds it when told where the toolchain is.
# Both tools read their config from the directory they run in, so run
# them inside app/ where .swiftlint.yml and .swiftformat live (and where
# .build is excluded).
cd app
TOOLCHAIN_DIR="$(xcode-select -p)" swiftlint lint --strict --quiet .
swiftformat --lint .
