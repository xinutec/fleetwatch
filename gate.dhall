{-
fleetwatch/gate.dhall — this repository's commit gate.

Was `scripts/verify.sh`, plus `scripts/with-test-db.sh`, which this conversion
deletes: the ephemeral MariaDB is now `dev-lint`'s `with-test-db`, shared with
messages and coach rather than written out a third time. Its header said so
itself — messages' copy opened with "Ported from fleetwatch, socket caveat and
all." The five values that differed between the copies (the database, the
credentials, the port, the env var, the temp-dir prefix) are the row below, where
they are typed and visible.

Four things changed shape in the move.

**The build is checked, not hoped for.** The script set `NG_BUILD_MAX_WORKERS=1`
and said a spurious abort "is worked around by re-running verify" — so a complete,
valid bundle that hit the macOS Piscina teardown abort failed the gate and cost a
manual re-run, and no assertion was ever made about what the build produced.
`ng-build` decides from the artifact instead: index.html present, non-empty,
rewritten by this run, and every script it names parseable as an ES module.

**The type-drift check stopped asking git.** It was `gen-types.sh` followed by
`git diff --quiet` over both generated directories, which reports one name for
two different faults — a regeneration that failed, and a regeneration nobody
committed — and, worse, cannot see an untracked file at all, so a brand-new wire
type left the row green. The ts-rs half is `gen-types --check` now, which
compares content and writes nothing. The golden fixture keeps generate-then-diff,
because it is one tracked file overwritten in place and has no added-file case to
miss; the rows below say so at length.

**The conditional `pnpm install` is gone**, for the reason gamepads', coach's and
memview's were: its own comment justified it on correctness — a node_modules left
behind by npm still has a working `.bin`, so verify would pass against packages
the lockfile no longer describes — and running it unconditionally serves that
better. Measured on gamepads before cutting: an up-to-date `--frozen-lockfile`
install is 455 ms.

**The Android step is two rows, and no longer quiet.** `gradlew -q :app:assembleDebug
:app:testDebugUnitTest` reported one name for two things, and at quiet level a
failure reports "1 failed" and an HTML report path without ever naming the test —
the one thing you want from a gate that has just gone red. The gate prints a
check's output only when it fails, so the cost is noise on a hand-run.

The generated `gate.json` is committed; `the table matches its Dhall` re-renders
and diffs it, so running the gate needs no `dhall`.

**The vocabulary moved into the schema.** `inDevShell`, the clippy target
directory, the Angular worker cap, and the `ng-build` / `dev-lint` /
`check-table` rows were spelled out here and in a dozen other tables
identically — the duplication the shared tools were built to remove, recreated
one level up. They are `G.` values now. Two consequences the rendered JSON
shows: every dev-shell row gains `--no-warn-dirty`, because a gate that prints
"Git tree is dirty" on every row of every run has trained everyone to ignore a
warning; and dev-lint is pinned to its committed HEAD rather than run out of its
worktree, which is what stops a neighbour's half-finished edit failing this gate
for a reason no commit anywhere explains.

-}

let G = ../dev-lint/gate/schema.dhall

in  { name = "fleetwatch"
    , checks =
      [ G.Check::{
        , name = "formatting"
        , argv = G.inDevShell [ "cargo", "fmt", "--all", "--check" ]
        , timeout_s = 180
        }
      , {-  Clippy gets its own target directory: clippy-driver and rustc
            fingerprint the workspace differently and evict each other in a
            shared one, forcing a full recompile. A dedicated directory keeps
            both caches warm.

            The script read this from `$CARGO_CLIPPY_TARGET_DIR` with the path
            below as the default. A table's `env` is data, not shell, so there is
            no expansion — and the override had no other caller, so the default
            is simply the value now.
        -}
        G.Check::{
        , name = "clippy"
        , argv =
            G.inDevShell [ "cargo", "clippy", "--all-targets", "--", "-D", "warnings" ]
        , env =
            G.clippyTarget
        , timeout_s = 1800
        }
      , {-  The whole suite, `tests/*_db.rs` included, against a throwaway
            MariaDB. Without the server those tests SKIP rather than fail, which
            is why the database is not optional here.

            No `--grant-all`: this suite uses the one database it is given, and
            the narrow default is what stops it inheriting rights it never asked
            for. Port 3317 — messages' ephemeral server takes 3318 and coach's
            3319, so the fleet gate can run all three at once.
        -}
        G.Check::{
        , name = "tests (against a real MariaDB)"
        , argv =
              G.inDevShell [ "nix", "run", "../dev-lint#with-test-db", "--" ]
            # [ "--database"
              , "fleetwatch"
              , "--user"
              , "fleetwatch"
              , "--password"
              , "fleetwatch"
              , "--port"
              , "3317"
              , "--url-env"
              , "FLEETWATCH_TEST_DATABASE_URL"
              , "--"
              , "cargo"
              , "test"
              ]
        , timeout_s = 1800
        }
      , {-  Generated-types drift: regenerate the ts-rs bindings into a scratch
            directory and fail if the committed frontend output differs. Catches
            a Rust API-type edit that was not regenerated and committed.

            This was two rows — `gen-types.sh`, then `git diff --exit-code` over
            both output directories — and asking git was the defect. `git diff`
            cannot see an untracked file, so a NEW wire type generated but never
            `git add`ed left the row green: measured on life, same tree, exit 0
            from `git diff --quiet` against exit 1 from `gen-types --check`.
            Comparing content against the committed directory has no such blind
            spot, and it writes nothing, so the gate no longer regenerates the
            worktree as a side effect of checking it.
        -}
        G.Check::{
        , name = "generated types are current"
        , argv = G.inDevShell [ "scripts/gen-types.sh", "--check" ]
        , timeout_s = 900
        }
      , {-  The golden wire fixture (tests/golden/problems.json) — the shape the
            Android poller hand-parses with org.json, where a renamed field would
            not crash anything, it would quietly stop reporting problems.

            Still generate-then-diff, and deliberately so. `export_golden_problems`
            resolves CARGO_MANIFEST_DIR itself, so unlike the ts-rs bindings it
            cannot be redirected into a scratch directory; it overwrites one
            tracked file in place and never clears first, so a failed run leaves
            the committed fixture standing. That also removes the blind spot
            above: with a single tracked file there is no added-file case for
            git to miss. A fifth shared tool for "run this and the directory must
            come out unchanged" would have exactly one consumer and no defect
            behind it, which is the kind of speculative vocabulary this whole
            migration has been removing.

            Two rows, because a generation that fails to compile and a
            regeneration nobody committed are different faults.
        -}
        G.Check::{
        , name = "the golden wire fixture regenerates"
        , argv = G.inDevShell [ "cargo", "test", "export_golden" ]
        , timeout_s = 900
        }
      , {-  Compares the worktree to the git *index* — so `git add -A` first, or
            this reads a stale tree. The pre-commit hook does.
        -}
        G.Check::{
        , name = "the golden wire fixture is current"
        , argv = [ "git", "diff", "--exit-code", "--", "tests/golden" ]
        , timeout_s = 120
        }
      , {-  `--frozen-lockfile` is pnpm ci: install exactly pnpm-lock.yaml, or
            fail. The gate has to run from a clean checkout — a fresh clone, or
            the tree this repository's own collector runs in — not just a warm
            dev machine.
        -}
        G.Check::{
        , name = "frontend deps match the lockfile"
        , cwd = "frontend"
        , argv = G.inDevShell [ "pnpm", "install", "--frozen-lockfile" ]
        , env = G.nonInteractive
        , timeout_s = 900
        }
      , G.Check::{
        , name = "frontend lint"
        , cwd = "frontend"
        , argv = G.inDevShell [ "pnpm", "run", "lint" ]
        , env = G.nonInteractive
        , timeout_s = 900
        }
      , G.Check::{
        , name = "frontend typecheck (e2e)"
        , cwd = "frontend"
        , argv = G.inDevShell [ "pnpm", "run", "typecheck:e2e" ]
        , env = G.nonInteractive
        , timeout_s = 900
        }
      , {-  `../../dev-lint`, not `../dev-lint`: cwd is `fleetwatch/frontend`.
        -}
        G.Check::{
        , name = "frontend build"
        , cwd = "frontend"
        , argv =
            G.ngBuild
              "../../"
              [ "dist/fleetwatch-web/browser" ]
              [ "pnpm", "exec", "ng", "build" ]
        , env = G.nonInteractive
        , timeout_s = 1800
        }
      , G.Check::{
        , name = "frontend unit tests"
        , cwd = "frontend"
        , argv = G.inDevShell [ "pnpm", "test" ]
        , env = G.nonInteractive # G.oneAngularWorker
        , timeout_s = 1800
        }
      , {-  The L2 phone-width layout harness: `e2e/serve.mjs` serves the dist the
            build row wrote and the specs assert no overlap or overflow at Pixel
            width.
        -}
        G.Check::{
        , name = "frontend ui-check (phone-width layout harness)"
        , cwd = "frontend"
        , argv = G.inDevShell [ "pnpm", "run", "ui-check" ]
        , env = G.nonInteractive
        , timeout_s = 1800
        }
      , {-  The Android poller is a real client with real logic — warning
            filtering, fingerprinting, notification decisions — so it compiles and
            its unit tests run here, not only when someone remembers. Toolchain
            comes from recall's android dev shell, the same one android/deploy.sh
            uses; a missing shell FAILS these rows rather than skipping them,
            because a gate that skips is a gate that lies.
        -}
        G.Check::{
        , name = "android :app assembleDebug"
        , cwd = "android"
        , argv =
            [ "nix"
            , "develop"
            , "../../recall#android"
            , "--command"
            , "./gradlew"
            , "--console=plain"
            , ":app:assembleDebug"
            ]
        , timeout_s = 1800
        }
      , G.Check::{
        , name = "android :app unit tests"
        , cwd = "android"
        , argv =
            [ "nix"
            , "develop"
            , "../../recall#android"
            , "--command"
            , "./gradlew"
            , "--console=plain"
            , ":app:testDebugUnitTest"
            ]
        , timeout_s = 1800
        }
      , G.checkTable "../dev-lint"
      , G.devLint "../"
      ]
    }
