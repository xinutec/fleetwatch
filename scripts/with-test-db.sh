#!/usr/bin/env bash
# Run a command against an ephemeral MariaDB, then tear it down.
#
#   scripts/with-test-db.sh cargo test
#
# Exports FLEETWATCH_TEST_DATABASE_URL so the DB integration tests (tests/*_db.rs)
# actually run instead of skipping. The datadir is a temp dir, wiped afterwards;
# nothing touches the long-lived dev DB (scripts/dev-db.sh, .dev/, :3307).
#
# Needs mariadb on PATH — run inside `nix develop` (the flake dev shell carries
# it), which is what verify.sh does.
set -euo pipefail

PORT="${FLEETWATCH_TEST_DB_PORT:-3317}"
DBDIR="$(mktemp -d "${TMPDIR:-/tmp}/fleetwatch-test-db.XXXXXX")"

# The socket lives OUTSIDE the datadir, in a short path of its own.
#
# A Unix socket path is capped at 103 bytes by the kernel, and $TMPDIR is not
# short when this runs under nested nix-shells: `~/Code/check --full` invokes
# this repo's verify inside its own shell, giving a $TMPDIR like
# /private/tmp/nix-shell-<pid>-<n>/nix-shell.XXXXXX/nix-shell.XXXXXX/. The
# datadir has no such limit, so only the socket needs to escape.
#
# It failed exactly one way: standalone `verify.sh` passed and the fleet gate
# failed, with mariadbd aborting on "socket file path is too long" — a fault
# that only appears when something else nests a shell around this one.
SOCKDIR="$(mktemp -d /tmp/fw-sock.XXXXXX)"
SOCKET="$SOCKDIR/d.sock"
if [ ${#SOCKET} -gt 103 ]; then
    echo "socket path is ${#SOCKET} bytes, over the 103 the kernel allows: $SOCKET" >&2
    exit 1
fi

cleanup() {
    [ -n "${DB_PID:-}" ] && kill "$DB_PID" 2>/dev/null && wait "$DB_PID" 2>/dev/null
    rm -rf "$DBDIR" "$SOCKDIR"
}
trap cleanup EXIT

mariadb-install-db --no-defaults --datadir="$DBDIR/data" \
    --auth-root-authentication-method=normal >/dev/null

cat >"$DBDIR/init.sql" <<'SQL'
CREATE DATABASE IF NOT EXISTS fleetwatch CHARACTER SET utf8mb4;
CREATE USER IF NOT EXISTS 'fleetwatch'@'127.0.0.1' IDENTIFIED BY 'fleetwatch';
GRANT ALL PRIVILEGES ON fleetwatch.* TO 'fleetwatch'@'127.0.0.1';
FLUSH PRIVILEGES;
SQL

# --skip-name-resolve: match by numeric IP so the '127.0.0.1' grant applies
# (same reason as dev-db.sh).
mariadbd --no-defaults --datadir="$DBDIR/data" --socket="$SOCKET" \
    --port="$PORT" --bind-address=127.0.0.1 --skip-name-resolve \
    --init-file="$DBDIR/init.sql" >"$DBDIR/mariadbd.log" 2>&1 &
DB_PID=$!

for _ in $(seq 1 100); do
    if mariadb-admin --no-defaults --socket="$SOCKET" -u root ping >/dev/null 2>&1; then
        break
    fi
    if ! kill -0 "$DB_PID" 2>/dev/null; then
        echo "mariadbd died during startup:" >&2
        cat "$DBDIR/mariadbd.log" >&2
        exit 1
    fi
    sleep 0.2
done

export FLEETWATCH_TEST_DATABASE_URL="mysql://fleetwatch:fleetwatch@127.0.0.1:$PORT/fleetwatch"
"$@"
