#!/usr/bin/env bash
# Builds libghostty-vt (static) from the Ghostty commit pinned by
# libghostty-vt-sys 0.2.1, with zig 0.15.2, into target-bench/ghostty-install,
# and prints the PKG_CONFIG_PATH to export before `cargo build --features ghostty`.
#
# Why by hand: as verified on 2026-09-04, zig 0.15.2 cannot link anything
# against the macOS 26.5 Command Line Tools SDK (undefined _printf/_abort/…),
# and Ghostty refuses zig 0.16. Pointing zig at the older SDK that ships in the
# same CLT works. The sys crate's build.rs cannot pass --sysroot, so we build
# out-of-band and let its `pkg-config` feature find the result.
set -euo pipefail
cd "$(dirname "$0")/../.."

commit=a887df42c56f6de86c0fe6da9c4eeca37931e083   # GHOSTTY_COMMIT in libghostty-vt-sys 0.2.1 build.rs
sdk=${GHOSTTY_SDK:-/Library/Developer/CommandLineTools/SDKs/MacOSX15.4.sdk}
src=target-bench/ghostty-src
prefix=$PWD/target-bench/ghostty-install

if [[ ! -d $src ]]; then
  git clone --filter=blob:none --no-checkout https://github.com/ghostty-org/ghostty.git "$src"
  git -C "$src" checkout -q "$commit"
fi
(cd "$src" && zig build -Demit-lib-vt=true -Doptimize=ReleaseFast -Demit-xcframework=false \
  -Dapp-runtime=none --sysroot "$sdk" --prefix "$prefix" --cache-dir "$PWD/target-bench/zig-cache")
echo "export PKG_CONFIG_PATH=$prefix/share/pkgconfig:\${PKG_CONFIG_PATH:-}"
