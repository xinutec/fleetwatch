# fleetwatch — fleet monitoring platform

The central place to see all system and code health/status for the fleet, from
any VPN device. Producers (the Mac mini tools first) POST verdict-shaped reports;
fleetwatch stores their history and serves a mobile UI. Adding a producer needs zero
code here. On isis (`fleetwatch.xinutec.org`); the read UI is gated by
Nextcloud login (ingest stays token-authed).

Design: [`docs/design.md`](docs/design.md).

## Stack

Rust axum + MariaDB + Angular 22 (zoneless), one image (`xinutec/fleetwatch:latest`)
serving both the API and the bundle. Same shape as `life`/`messages`.

- `src/` — backend (main/lib split; `report/` is the domain, `routes/` the HTTP
  layer). Wire types derive `ts-rs` → `frontend/src/app/generated/`.
- `frontend/` — the Angular app (`fleetwatch-web`).
- `android/` — single-WebView wrapper (`org.xinutec.fleetwatch`).
- `migrations/` — sqlx migrations, run at boot.

The k8s manifests (`k8s/` — numbered manifests + `secret.sh` + `sync.sh`) live in
the home monorepo at `xinutec/pippijn` `code/kubes/fleetwatch/k8s/`. This repo
builds and pushes the image; the `xinutec/fleetwatch:latest` tag is the only
contract between the two.

## The report contract

A producer POSTs to `/api/reports` with `Authorization: Bearer <token>`; the
matching `source` is stamped server-side (a producer can only write as itself).
Body (`schema: 1`):

```jsonc
{
  "schema": 1,
  "id": "01J…",                 // producer-minted ULID (idempotency key)
  "collector": "fleet-health",
  "collected_at": "2026-07-03T14:00:00Z",
  "duration_ms": 84211,
  "interval_s": 3600,           // declared cadence → staleness
  "checks": [
    { "section": "isis", "label": "disk usage /", "verdict": "pass",
      "observed": "43% used", "expected": "< 85%",
      "value": 43.0, "unit": "%", "ref": "backups.md:57" }
  ]
}
```

`verdict` ∈ `pass|warn|fail|skip`. A check's trend identity is
`(source, collector, section, label)` — keep `label` stable across runs, put
run-varying data in `observed`/`value`. `label` must be non-empty and both
`section`/`label` at most 255 bytes (longer would be truncated by the columns,
silently splitting one trend into two). `201` = stored, `200` = idempotent
duplicate, `401` = bad token, `422` = bad schema/shape/identity.

## Develop

```sh
./scripts/dev-db.sh                                   # local MariaDB on :3307
export FLEETWATCH_TOKENS=mac-mini:dev-token
export DATABASE_URL=mysql://fleetwatch:fleetwatch@127.0.0.1:3307/fleetwatch
nix develop -c cargo run                              # API on :8080
( cd frontend && npm install && npm start )          # ng serve, proxies /api

nix develop -c scripts/gen-types.sh                   # regenerate TS types
nix develop -c cargo test export_golden               # regenerate tests/golden
FLEETWATCH_TEST_DATABASE_URL=$DATABASE_URL nix develop -c cargo test   # incl. DB tests
nix run ../dev-lint#gate -- . gate.json                # full gate (gate.dhall)
```

## Deploy (isis)

CI here publishes `xinutec/fleetwatch:latest` on push to main. The k8s manifests
live in the home monorepo (`xinutec/pippijn` `code/kubes/fleetwatch/k8s/`); run
the deploy steps from that checkout. On isis:

1. One-time: `letsencrypt-dns` issuer + `cloudflare-api-token` (shared with
   messages, already present); `./k8s/secret.sh` (DB creds + ingest tokens —
   copy each printed token to its producer's `~/.config/fleetwatch/token`); the DNS A
   record `fleetwatch → 10.100.0.2` (`code/dns`, `tofu apply`).
2. `sudo ./k8s/sync.sh`.
