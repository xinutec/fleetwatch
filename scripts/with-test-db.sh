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
SOCKET="$DBDIR/mysqld.sock"

cleanup() {
    [ -n "${DB_PID:-}" ] && kill "$DB_PID" 2>/dev/null && wait "$DB_PID" 2>/dev/null
    rm -rf "$DBDIR"
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
