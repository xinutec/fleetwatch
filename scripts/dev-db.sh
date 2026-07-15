#!/usr/bin/env nix-shell
#!nix-shell -i bash -p mariadb
# Local dev MariaDB for fleetwatch. Data lives in .dev/ (gitignored). Idempotent:
# initialises the datadir on first run, then serves in the foreground on
# 127.0.0.1:3307. Creates the `fleetwatch` database + a `fleetwatch`/`fleetwatch` dev account
# via an init file each boot.
#
#   ./scripts/dev-db.sh
#   DATABASE_URL=mysql://fleetwatch:fleetwatch@127.0.0.1:3307/fleetwatch
#
# Ctrl-C to stop. Delete .dev/ to reset.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DATADIR="$ROOT/.dev/mysql"
SOCKET="$ROOT/.dev/mysqld.sock"
INIT_SQL="$ROOT/.dev/init.sql"
# Override when 3307 is taken by another project's dev-db (life/coach share this
# pattern): FLEETWATCH_DEV_DB_PORT=3310 ./scripts/dev-db.sh
PORT="${FLEETWATCH_DEV_DB_PORT:-3307}"

mkdir -p "$ROOT/.dev"

if [ ! -d "$DATADIR/mysql" ]; then
    echo "Initialising MariaDB data dir at $DATADIR ..."
    mariadb-install-db --no-defaults --datadir="$DATADIR" \
        --auth-root-authentication-method=normal >/dev/null
fi

cat >"$INIT_SQL" <<'SQL'
CREATE DATABASE IF NOT EXISTS fleetwatch CHARACTER SET utf8mb4;
CREATE USER IF NOT EXISTS 'fleetwatch'@'127.0.0.1' IDENTIFIED BY 'fleetwatch';
GRANT ALL PRIVILEGES ON fleetwatch.* TO 'fleetwatch'@'127.0.0.1';
FLUSH PRIVILEGES;
SQL

echo "Serving MariaDB on 127.0.0.1:$PORT (db: fleetwatch) — Ctrl-C to stop"
# --skip-name-resolve: match connections by numeric IP so the '127.0.0.1' grant
# always applies. Without it MariaDB reverse-resolves 127.0.0.1 to 'localhost'
# and denies the fleetwatch user (host mismatch).
exec mariadbd --no-defaults --datadir="$DATADIR" --socket="$SOCKET" \
    --port="$PORT" --bind-address=127.0.0.1 --skip-name-resolve \
    --init-file="$INIT_SQL"
