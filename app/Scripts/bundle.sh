#!/usr/bin/env bash
# Assembles "Vambiant Term.app" from the SwiftPM build plus the Rust
# binaries. No .xcodeproj, ever. Signing is ad hoc ("-") until M9 brings a
# Developer ID and notarization; the bundle id com.vambiant.term is
# permanent because TCC grants anchor to it.
#
#   app/Scripts/bundle.sh [debug|release]   → app/build/Vambiant Term.app
set -euo pipefail
config="${1:-release}"
root="$(cd "$(dirname "$0")/../.." && pwd)"
app="$root/app"
out="$app/build/Vambiant Term.app"

cd "$root"
mise run ffi:staticlib
cargo build --release -p vtermd -p vterm
cd "$app"
swift build -c "$config"
bin="$(swift build -c "$config" --show-bin-path)"

rm -rf "$out"
mkdir -p "$out/Contents/MacOS" "$out/Contents/Resources"
cp "$app/Resources/Info.plist" "$out/Contents/"
cp "$bin/VambiantTerm" "$out/Contents/MacOS/VambiantTerm"
cp "$root/target/release/vtermd" "$root/target/release/vterm" "$out/Contents/MacOS/"
if [[ "$config" == "release" ]]; then
  strip -x "$out/Contents/MacOS/VambiantTerm" "$out/Contents/MacOS/vtermd" "$out/Contents/MacOS/vterm"
fi
echo -n "APPL????" > "$out/Contents/PkgInfo"
codesign --force --sign - --identifier com.vambiant.term "$out/Contents/MacOS/vtermd" "$out/Contents/MacOS/vterm"
codesign --force --sign - --identifier com.vambiant.term "$out"
codesign --verify --deep --strict "$out"
echo "bundle: $out"
