#!/usr/bin/env bash
# fleetwatch verify — rust backend (fmt + clippy + tests, incl. the DB integration
# tests against an ephemeral MariaDB) + angular frontend (build + unit tests +
# phone-width layout harness) + android poller (compile + unit tests) + shared
# rules + type-drift gate.
set -euo pipefail
cd "$(dirname "$0")/.."
nix develop -c bash -c '
  set -euo pipefail
  # @angular/build:application tears down its Piscina worker pool at process
  # exit; on macOS / Node 24 / libuv 1.52 that teardown intermittently aborts
  # the process — a libuv kqueue assertion ("errno == EINTR", uv__io_poll →
  # Abort 6) or "EBADF: bad file descriptor, close" — AFTER "bundle generation
  # complete", i.e. once a complete, valid bundle is already on disk.
  # NG_BUILD_MAX_WORKERS=1 lowers the rate (fewer worker pipes to race) but does
  # NOT eliminate it; a spurious build abort here is worked around by re-running
  # verify. Harmless on Linux/CI, which build cleanly. NOT the sandbox.
  export NG_BUILD_MAX_WORKERS=1
  cargo fmt --all --check
  # Clippy gets its own target dir: clippy-driver and rustc fingerprint the
  # workspace differently and evict each other in a shared dir, forcing a full
  # recompile. A dedicated dir keeps both caches warm.
  CARGO_TARGET_DIR="${CARGO_CLIPPY_TARGET_DIR:-$HOME/.cache/cargo/clippy-target}" \
    cargo clippy --all-targets -- -D warnings
  # The whole suite, DB integration tests included — with-test-db.sh brings up a
  # throwaway MariaDB and exports FLEETWATCH_TEST_DATABASE_URL so the tests/*_db.rs
  # tests run instead of silently skipping.
  scripts/with-test-db.sh cargo test
  # Regenerate the TS types + golden wire fixture and fail if the committed
  # output drifted.
  scripts/gen-types.sh
  if ! git diff --quiet -- frontend/src/app/generated tests/golden; then
    echo "generated types/fixtures are stale — run scripts/gen-types.sh and commit" >&2
    git --no-pager diff -- frontend/src/app/generated tests/golden >&2
    exit 1
  fi
  # ui-check (L2 phone-width layout harness) runs after the build — it serves
  # the freshly-built dist via e2e/serve.mjs and asserts no overlap/overflow at
  # Pixel width. See @xinutec/ui-harness + dev-lint/docs/layout-quality-architecture.md.
  # Frontend deps must exist before lint/build. verify.sh has to run from a clean
  # checkout (a fresh clone, or the tree the fleetwatch collector runs in) — not
  # just a warm dev machine — so install them when absent or the lockfile moved.
  # --frozen-lockfile is pnpm ci: install exactly pnpm-lock.yaml, or fail. The
  # guard is not just a speed-up — a node_modules left behind by npm still has a
  # working .bin, so verify would pass against packages the lockfile no longer
  # describes.
  if [ ! -d frontend/node_modules ] || [ frontend/pnpm-lock.yaml -nt frontend/node_modules ]; then
    ( cd frontend && pnpm install --frozen-lockfile )
  fi
  ( cd frontend && pnpm run lint && pnpm run typecheck:e2e && pnpm exec ng build && pnpm test && pnpm run ui-check )
'
# The Android poller is a real client with real logic (warning filtering,
# fingerprinting, notification decisions) — it compiles and its unit tests run
# here, not only when someone remembers. Toolchain comes from recall's android
# dev shell, same as android/deploy.sh; a missing shell fails the gate rather
# than skipping (a gate that skips is a gate that lies).
( cd android && nix develop ~/Code/recall#android --command ./gradlew --console=plain -q :app:assembleDebug :app:testDebugUnitTest )
dev_lint_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)/dev-lint"
[ -d "$dev_lint_dir" ] || dev_lint_dir="$HOME/Code/dev-lint"
[ -d "$dev_lint_dir" ] || dev_lint_dir="$HOME/code/dev-lint"
nix run "$dev_lint_dir" -- . # dev-lint
