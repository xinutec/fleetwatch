#!/usr/bin/env bash
# Generate the frontend TS interfaces from the Rust API types via ts-rs, so the
# backend↔frontend wire shapes are consistent by construction (not transcribed),
# plus the golden wire fixture (tests/golden/) the Android unit tests parse —
# the Kotlin side has no codegen, so its drift is caught by test instead.
#
# Run inside the fleetwatch dev shell (cargo on PATH):
#   nix develop --command scripts/gen-types.sh
#
# Output lands in frontend/src/app/generated/ (committed; imported via
# frontend/src/app/models.ts). The output dir is pinned in .cargo/config.toml
# (TS_RS_EXPORT_DIR).
set -euo pipefail
cd "$(dirname "$0")/.."

OUT="frontend/src/app/generated"
rm -rf "$OUT"
# ts-rs emits one file per #[ts(export)] type; the export tests are named
# export_bindings_*, and export_golden_* writes tests/golden/ — this filter
# runs only generation (no DB needed).
cargo test export_ >/dev/null 2>&1
echo "generated $(find "$OUT" -name '*.ts' | wc -l | tr -d ' ') type(s) -> $OUT + tests/golden"
