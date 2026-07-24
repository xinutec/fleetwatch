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
#
# GENERATE FIRST, into a scratch dir, and only replace the committed output once
# it has actually worked. The old order was `rm -rf "$OUT"` then generate with
# both streams sent to /dev/null: any compile error in the test tree (a struct
# literal missing a new field is enough) deleted every committed type file and
# said nothing, leaving the tree unbuildable for a reason nothing reported. The
# drift gate below in verify.sh would then have compared against a directory that
# no longer existed. A generator that fails must leave the previous output
# exactly where it was. (Same fix as life/coach; keep the three in step.)
set -euo pipefail
cd "$(dirname "$0")/.."

OUT="frontend/src/app/generated"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/fleetwatch-ts-rs.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT

# ts-rs emits one file per #[ts(export)] type; the export tests are named
# export_bindings_*, and export_golden_* writes tests/golden/ — this filter runs
# only generation (no DB needed). TS_RS_EXPORT_DIR overrides the pinned dir so a
# failed run can't touch the committed types. The golden fixture is unaffected by
# that override (its test writes to CARGO_MANIFEST_DIR/tests/golden itself) and
# is safe either way: it writes in place and never clears first, so a failure
# simply leaves the committed fixture standing.
if ! log="$(TS_RS_EXPORT_DIR="$TMP" cargo test export_ 2>&1)"; then
  echo "gen-types: generation failed — committed types left untouched." >&2
  # The compile errors are the whole point of running this; show them.
  printf '%s\n' "$log" | grep -E '^(error|warning: unused)|^ *-->' >&2 ||
    printf '%s\n' "$log" | tail -30 >&2
  exit 1
fi

count="$(find "$TMP" -name '*.ts' | wc -l | tr -d ' ')"
if [ "$count" -eq 0 ]; then
  # cargo succeeded but emitted nothing — the export tests were filtered out or
  # renamed. Wiping a good directory over that is the same silent loss.
  echo "gen-types: generation produced no types — committed types left untouched." >&2
  exit 1
fi

rm -rf "$OUT"
mkdir -p "$(dirname "$OUT")"
cp -R "$TMP" "$OUT"
echo "generated $count type(s) -> $OUT + tests/golden"
