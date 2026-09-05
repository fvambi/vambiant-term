#!/usr/bin/env bash
# Compiles terminfo/vambiant-term.terminfo into ~/.terminfo (or $1) with tic -x.
# The .app bundle ships the compiled entry and points TERMINFO at it (M9).
set -euo pipefail
cd "$(dirname "$0")/.."
dest=${1:-$HOME/.terminfo}
tic -x -o "$dest" terminfo/vambiant-term.terminfo
echo "installed vambiant-term into $dest"; TERMINFO="$dest" infocmp -x vambiant-term | head -2
