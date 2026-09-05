#!/usr/bin/env bash
# Terminal-core throughput benchmark, Ghostty methodology (docs/08 §7):
#   * inputs are pre-generated once and reused (never regenerated per run)
#   * timed with hyperfine, warm-up runs, medians reported
#   * strictly serial — never run two benchmarks in parallel
#
# Usage: scripts/bench/run.sh [--with-ghostty] [--runs N] [out.json]
set -euo pipefail
cd "$(dirname "$0")/../.."

with_ghostty=0; runs=10; out=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --with-ghostty) with_ghostty=1 ;;
    --runs) runs="$2"; shift ;;
    *) out="$1" ;;
  esac; shift
done

corpus=target-bench/corpus
if [[ ! -d $corpus ]]; then
  cargo build --release -p term-core-spike --bin gen-corpus
  ./target/release/gen-corpus "$corpus"
fi

cargo build --release -p term-core-spike --bin alacritty-spike
cmds=(-n alacritty "./target/release/alacritty-spike bench {file} 200 50")
if [[ $with_ghostty -eq 1 ]]; then
  scripts/bench/build-ghostty.sh
  cmds+=(-n ghostty "./target/release/ghostty-spike bench {file} 200 50")
fi

files=$(ls "$corpus"/*.vt | tr '\n' ',' | sed 's/,$//')
export_args=()
[[ -n $out ]] && export_args=(--export-json "$out")
hyperfine --warmup 2 --runs "$runs" -L file "$files" "${export_args[@]}" "${cmds[@]}"
