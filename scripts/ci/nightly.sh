#!/usr/bin/env bash
# Nightly: cargo-fuzz, ASan/TSan, real-agent round trips, leak tests.
# Nothing here exists before M3 / M-SEC; say so instead of green-lighting nothing.
set -euo pipefail
echo "nightly: NOT IMPLEMENTED — fuzz targets (M1), sanitizers (M4), agent round-trips (M3) not built yet" >&2
exit 1
