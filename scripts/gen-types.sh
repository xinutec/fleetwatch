#!/usr/bin/env bash
# Generate the frontend TS interfaces from the Rust types via ts-rs, so the
# backend↔frontend wire shapes are consistent by construction, not transcribed.
#
#   nix develop --command scripts/gen-types.sh            # regenerate + install
#   nix develop --command scripts/gen-types.sh --check    # report drift, write nothing
#
# The second form is what the gate's generated-types row runs, so the cargo
# invocation below is stated once and both paths use it.
#
# All this file holds is the part that is this repository's: where the bindings
# live and how to make cargo emit them. The rest — generate into a scratch
# directory and install only on success, refuse a generation that emitted
# nothing, copy the types and not whatever else landed beside them, compare by
# content rather than by asking git — is dev-lint#gen-types, shared with the
# four other repositories that had each grown their own version of it.
#
# THE GOLDEN WIRE FIXTURE IS NO LONGER GENERATED HERE. It used to be: the filter
# was `export_` rather than `export_bindings`, which swept up
# `export_golden_problems` as well. That test writes `tests/golden/` in place —
# it resolves CARGO_MANIFEST_DIR itself and so ignores the scratch directory
# TS_RS_EXPORT_DIR names — and a `--check` that writes to the worktree is not a
# check. It is its own gate row now (`cargo test export_golden`, then
# `git diff --exit-code -- tests/golden`), which is adequate there for the
# reason it is not adequate here: one tracked file, overwritten in place, so
# there is no added-file case for git to be blind to.
#
# `.cargo/config.toml` pins TS_RS_EXPORT_DIR to the committed output directory
# so a plain `cargo test` writes somewhere sensible. gen-types sets that
# variable in the environment, and cargo's `[env]` yields to an environment that
# already has it — which is what lets the scratch-directory generation work at
# all. If that pin ever grows `force = true`, this stops being safe.
set -euo pipefail
cd "$(dirname "$0")/.."

exec nix run ../dev-lint#gen-types -- "$@" \
  --out frontend/src/app/generated \
  -- cargo test export_bindings
