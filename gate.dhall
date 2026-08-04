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

**The type-drift check is two rows.** It was `gen-types.sh` followed by
`git diff --quiet`, which reports one name for two different faults — a
regeneration that failed, and a regeneration nobody committed. `git diff
--exit-code` needs no shell around it, so the split costs nothing.

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
-}

let G = ../dev-lint/gate/schema.dhall

let inDevShell = \(argv : List Text) -> [ "nix", "develop", "--command" ] # argv

{-| `ng build` tears down its Piscina worker pool at process exit; on macOS /
    Node 24 / libuv 1.52 that teardown intermittently aborts the process AFTER a
    complete, valid bundle is on disk. This lowers the rate — fewer worker pipes
    to race — but does not eliminate it. The build row does not need this: it
    goes through `ng-build`, which sets the knob itself and then decides from the
    artifact anyway. These are the rows that drive a build indirectly.
-}
let oneAngularWorker = toMap { NG_BUILD_MAX_WORKERS = "1" }

in  { name = "fleetwatch"
    , checks =
      [ G.Check::{
        , name = "formatting"
        , argv = inDevShell [ "cargo", "fmt", "--all", "--check" ]
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
            inDevShell [ "cargo", "clippy", "--all-targets", "--", "-D", "warnings" ]
        , env =
            toMap
              { CARGO_TARGET_DIR = "/Users/pippijn/.cache/cargo/clippy-target" }
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
              inDevShell [ "nix", "run", "../dev-lint#with-test-db", "--" ]
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
      , G.Check::{
        , name = "generated types regenerate"
        , argv = inDevShell [ "scripts/gen-types.sh" ]
        , timeout_s = 900
        }
      , {-  Drift: the regeneration above against what is committed. Compares the
            worktree to the git *index*, as the script did — so `git add -A`
            first, or this reads a stale tree.
        -}
        G.Check::{
        , name = "generated types are current"
        , argv =
            [ "git"
            , "diff"
            , "--exit-code"
            , "--"
            , "frontend/src/app/generated"
            , "tests/golden"
            ]
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
        , argv = inDevShell [ "pnpm", "install", "--frozen-lockfile" ]
        , timeout_s = 900
        }
      , G.Check::{
        , name = "frontend lint"
        , cwd = "frontend"
        , argv = inDevShell [ "pnpm", "run", "lint" ]
        , timeout_s = 900
        }
      , G.Check::{
        , name = "frontend typecheck (e2e)"
        , cwd = "frontend"
        , argv = inDevShell [ "pnpm", "run", "typecheck:e2e" ]
        , timeout_s = 900
        }
      , {-  `../../dev-lint`, not `../dev-lint`: cwd is `fleetwatch/frontend`.
        -}
        G.Check::{
        , name = "frontend build"
        , cwd = "frontend"
        , argv =
              inDevShell [ "nix", "run", "../../dev-lint#ng-build", "--" ]
            # [ "--expect"
              , "dist/fleetwatch-web/browser"
              , "--"
              , "pnpm"
              , "exec"
              , "ng"
              , "build"
              ]
        , timeout_s = 1800
        }
      , G.Check::{
        , name = "frontend unit tests"
        , cwd = "frontend"
        , argv = inDevShell [ "pnpm", "test" ]
        , env = oneAngularWorker
        , timeout_s = 1800
        }
      , {-  The L2 phone-width layout harness: `e2e/serve.mjs` serves the dist the
            build row wrote and the specs assert no overlap or overflow at Pixel
            width.
        -}
        G.Check::{
        , name = "frontend ui-check (phone-width layout harness)"
        , cwd = "frontend"
        , argv = inDevShell [ "pnpm", "run", "ui-check" ]
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
      , G.Check::{
        , name = "the table matches its Dhall"
        , argv =
            [ "nix"
            , "run"
            , "../dev-lint#gate"
            , "--"
            , "--check-table"
            , "gate.dhall"
            , "gate.json"
            ]
        , timeout_s = 120
        }
      , {-  Shared fleet rules over the whole repository. `nix run`, never
            result/bin — a pinned build goes stale and silently misses rules
            shipped since.
        -}
        G.Check::{
        , name = "dev-lint"
        , argv = [ "nix", "run", "../dev-lint", "--", "." ]
        , timeout_s = 900
        }
      ]
    }
