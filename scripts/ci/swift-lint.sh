#!/usr/bin/env bash
# SwiftLint + SwiftFormat --lint over app/. Until M4 there is no app/, and a
# skipped step must say so loudly rather than pretend it passed.
set -euo pipefail
cd "$(dirname "$0")/../.."
if [[ ! -f app/Package.swift ]]; then
  echo "swift-lint: SKIPPED — app/Package.swift does not exist yet (lands in M4)"
  exit 0
fi
swiftlint lint --strict app
swiftformat --lint app
