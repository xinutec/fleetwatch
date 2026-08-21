# fleetwatch — fleet monitoring platform

Design doc, 2026-07-03; status 2026-07-16: **all milestones built and live** on
isis at `https://fleetwatch.xinutec.org` — token-authed ingest, Nextcloud-login
reads, retention, mutes, the full UI, the Android wrapper + poller, and six Mac
producers pushing on launchd timers. What follows records the design decisions;
resolutions are marked in place.

## 1. Goal

A single mobile-friendly place to see all system and code health/status knowledge
for the fleet, from any VPN device. Long term it is the central custom monitoring
platform: everything the CLI scripts print today (`fleet_health.py`, `doc_checks.py`,
`~/Code/check`, per-repo verify gates) plus future producers, rendered as tiles,
bullet lists, and charts.

Two roles, deliberately separated:

- **Producers** — anything that can run a check and POST JSON. First producer is the
  Mac mini running the existing tools on a timer. Later: amun/isis/odin themselves,
  odin backup jobs, in-cluster jobs.
- **The platform (`fleetwatch`)** — one k3s service on isis that ingests reports, stores
  history, and serves the UI. It knows nothing about *what* is being checked; it
  renders whatever verdict-shaped data arrives. Adding a new producer requires zero
  service changes.

### Non-goals (v1)

- No *server-pushed* alerting. fleetwatch never reaches out: no FCM, no mail, no
  webhook. Dead-man alerting stays on healthchecks.io.
  **Superseded in part (2026-07-12):** the Android app now *pulls* — a background
  worker polls `GET /api/problems` every 30 min and raises a local notification when
  the problem set changes (see `android/README.md`). The service stays passive; the phone asks.
  This exists because a dead producer that nobody looks at is a dead producer nobody
  knows about: the pixel5 sensor receiver went silent for 7 hours and was caught only
  by a human noticing a missing line on a chart.
- No remote command execution — strictly ingest + display. The service never reaches
  out to the fleet (it can't reach the Mac anyway; the Mac is a one-way VPN peer).
- No metric-scraping agents (node_exporter etc.). Producers are our own tools.
- No auth UI. VPN + source-range whitelist gates reads; a bearer token gates writes.

## 2. Why custom (alternatives considered)

- **Prometheus + Grafana**: our data is *verdict-shaped* (pass/fail/warn with
  observed/expected strings and doc refs), not metric-shaped; the pull model can't
  reach the one-way Mac; and Grafana Cloud's public-dashboard limits already bit us.
  The existing tools are check-runners, not exporters — bending them into metrics
  loses exactly the part we like (the labelled ✓/✗ lines with context).
- **healthchecks.io**: only liveness of jobs, no content.
- **Extending home.xinutec.org**: home is public-by-design and sensor-focused;
  monitoring state is fleet-internal and belongs behind the VPN.

Custom service, common libraries (axum, sqlx, Angular Material, a small chart lib),
same stack as the rest of the fleet so `~/Code/check` keeps it consistent.

## 3. Existing landscape (what feeds this)

| Tool | Lives | Checks | Output (pre-fleetwatch) |
|---|---|---|---|
| `fleet_health.py` | `xinutec-infra/mac-mini/` | hosts, k3s pods, backups+drills, restic, TLS/DNS, blocklists, healthchecks.io, VPN one-way, git drift | ANSI text, exit code |
| `doc_checks.py` | `xinutec-infra/mac-mini/` | documented claims vs live fleet, each with `file:line` ref | ANSI text, exit code |
| `~/Code/check` (`dev_lint.fleet`) | dev-lint engine + `~/Code/check` config | fleet consistency, per-repo dev-lint, per-repo gate (`--full`) | ANSI text, exit code |
| `dev-lint` | `~/Code/dev-lint` | per-file `path:line:col: RULE msg` | text, exit code |

At design time none emitted JSON, but all held structured data in memory
(`CheckResult(section, label, verdict, observed, expected)` in `_checks.py`;
`Finding`/`Run` tuples in `fleet.py`; `Violation` dataclass in dev-lint). The
`--json` emitters (since built) are serializers, not parsers — no text-scraping
anywhere.

## 4. Data model

### 4.1 Report schema (the wire format, `schema: 1`)

```jsonc
{
  "schema": 1,
  "id": "01J...",              // ULID minted by the producer (idempotency key)
  "collector": "fleet-health", // which tool produced this
  "collected_at": "2026-07-03T14:00:00Z",  // producer clock, start of run
  "duration_ms": 84211,
  "interval_s": 3600,          // producer's own declared cadence → staleness
  "checks": [
    {
      "section": "isis",              // grouping, mirrors the CLI section headers
      "label": "disk usage /",        // STABLE key part — must not embed values
      "subject": "isis",              // optional: which host/repo this is ABOUT
      "verdict": "pass",              // pass | fail | warn | skip
      "observed": "43% used",         // free text, mirrors CLI detail
      "expected": "< 85%",            // optional free text
      "value": 43.0, "unit": "%",     // optional numeric → trend charts
      "ref": "backups.md:57",         // optional doc/source reference
      "detail": "…"                   // optional multi-line drill-down text
    }
  ]
}
```

Notes on the shape — each of these is a deliberate decision:

- **`source` is not in the body.** The ingest token *is* the identity: the server
  maps token → source name and stamps it server-side. A compromised or buggy
  producer cannot spoof another machine's status.
- **Check identity for trends** is `(source, collector, section, label)`. This is a
  *contract on producers*: labels must be stable across runs, with run-varying data
  in `observed`/`value`, never in the label. `_checks.py` already separated label
  from observed; `fleet.py` did not (free-form consistency messages) — §7.3 was
  that refactor.
- **`subject`** exists because tools report *about* machines they don't run on
  (`fleet_health.py` runs on the Mac, reports about odin). It enables a future
  "everything about isis" view regardless of which producer said it.
- **One optional numeric per check.** A check needing several numbers is several
  checks. Keeps charting trivial.
- **`interval_s` self-declares cadence** so staleness needs no server-side config:
  the last report's declared interval drives the overdue computation. A producer
  that changes cadence updates it with its next report.
- **`collected_at` vs `received_at`**: both stored. The spool (§7.2) means uploads
  can arrive late; producer clocks can skew. Truth-time is `collected_at`;
  `received_at` is diagnostic.
- **Report-level `ok` is derived** (no check has verdict `fail`), never sent.
- **`schema` version field** from day one; the server rejects unknown versions
  rather than guessing.

### 4.2 Database (MariaDB, own instance, `life` pattern)

```
report   id (ULID pk), source, collector, schema, collected_at, received_at,
         duration_ms, interval_s, ok (derived), raw (LONGTEXT, the payload)
check_result
         report_id (fk), seq, section, label, subject, verdict,
         observed, expected, value, unit, ref, detail,
         -- denormalized for the trend query, avoids the join:
         source, collector, collected_at
token    (none — tokens live in the k8s secret as env, see §6)
```

Indexes: `report(source, collector, collected_at)`;
`check_result(source, collector, section, label, collected_at)` for history;
`check_result(verdict, collected_at)` for the problems view.

Later migrations added `sessions` (0002, see §6), `mute` (0003, see §5), made
`verdict` an ENUM (0004) so a corrupt write fails at the producer — where the
spool retry makes it visible — instead of poisoning reads, and added
`latest_report` (0005).

`latest_report(source, collector) → report_id` is the newest report per
producer, written by ingest in the report's own transaction. Both read views need
that answer and both used to derive it per request, by ranking the entire
`report` table with a window function; the cost grew with history to re-answer a
question that only changes on ingest, and concurrent polling of it exhausted the
connection pool. Storing it where it is decided makes both reads an indexed join.
The pointer's ordering is the window's — newer `collected_at` wins, ties broken
by the larger id — and `repo::rebuild_latest_report` recomputes it from `report`
when the two need re-reconciling.

**Volume estimate**: ~200 checks/report × hourly × 3 collectors ≈ 15–20k check
rows/day, ~7M/year — comfortable for MariaDB with the above indexes. Raw payloads
(~100 KB each) are the real weight: ~5–7 MB/day, ~2.5 GB/year.

**Retention** (background task, daily): raw payloads pruned after **30 days**
(kept that long for schema-evolution replays and debugging); `check` rows after
**400 days** (a year of trends + margin); `report` summary rows kept forever
(they're tiny and answer "how long has this been running").

## 5. API

All under `/api`, plus `/healthz`. Types shared Rust→TS via ts-rs (life pattern).

| Route | Purpose |
|---|---|
| `POST /api/reports` | Ingest. `Authorization: Bearer <token>`. 201 on store, **200 on duplicate `id`** (idempotent replay from the spool), 401 bad token, 422 bad schema. |
| `GET /api/overview` | Sources × collectors: latest verdict rollup, check counts by verdict, `collected_at`, staleness state. The home-screen query. |
| `GET /api/problems` | All checks with verdict fail/warn from each collector's *latest* report, plus overdue collectors. "What's wrong right now." The **only** endpoint a *read token* opens (`FLEETWATCH_READ_TOKENS`, `Authorization: Bearer`), for the Android poller — a background worker can't do an interactive NC login. Every other read stays session-only, so a leaked phone token can't walk the report history. |
| `GET /api/reports?source&collector&limit` | Report list (history of runs). |
| `GET /api/reports/:id` | One report with all checks, grouped by section — the CLI-output-mirror view. |
| `GET /api/history?source&collector&section&label&from&to` | Time series for one check: `(collected_at, verdict, value)` tuples. Feeds sparklines/charts and "since when is this red". |
| `GET /api/mutes` · `POST /api/mutes` · `DELETE /api/mutes/:id` | Expiring mutes (see below). Session-only (`AuthUser`) — a mute is an accountable human decision, so the read token can't create one and `created_by` is stamped from the session. |

Ingest validation is strict (unknown verdicts, missing fields, unknown `schema` →
422 with a reason). Producers are ours; failing loudly beats storing junk.

### Mutes (deliberate, expiring suppression)

Some failures are expected: a Pi powered off on purpose, a host down for
maintenance. Its check honestly keeps reporting `fail` — the producer's verdict is
a fact, never rewritten. A **mute** is a read-time overlay keyed on a check's
identity `(source, collector, label)`:

- `GET /api/problems` moves a muted check out of `checks` (so the Android poller
  stays quiet) into a separate `muted` list — kept visible, with its reason and
  expiry, so a silence is never invisible.
- `GET /api/overview` subtracts muted fail/warn from a tile's live counts and its
  `worst` verdict, so a muted-only failure shows green with a "muted" marker
  rather than staying permanently red.

Every mute **must expire** (`expires_at NOT NULL`, TTL clamped to 1h–90d) and
carries a mandatory `reason` + `created_by`: intentional silence that cannot rot
into a forgotten blind spot. When it lapses the problem simply reappears — no
reticketing. Deleting a mute (unmute) restores the problem on the next read.
Because it is a pure overlay, stored history and the report rollups are never
touched: the truth stays queryable and the mute is auditable.

### Staleness (first-class)

A push-based monitor's worst failure mode is a dead producer looking green. The
overview computes, per (source, collector):

- `fresh` — age ≤ 1.5 × `interval_s`
- `overdue` (rendered as warn) — age ≤ 3 × `interval_s`
- `silent` (rendered as fail) — age > 3 × `interval_s`

Staleness is computed at read time from the last report — no cron, no server
config. A source that has *never* reported is unknown to fleetwatch; the eventual
guard for "expected producers exist" is a `doc_checks.py`-style check (a producer
asserting the producer list — turtles, but it works and it's one more check row).

## 6. Service: `fleetwatch` on isis

Stack: **Rust axum + MariaDB + Angular 22 zoneless**, cloned from the `life`
skeleton (main/lib split, `routes/` layer, sqlx migrations, ts-rs types, three-stage
Dockerfile → `xinutec/fleetwatch:latest`, nonroot 65532, read-only rootfs, tight
requests/limits).

k8s (`code/kubes/fleetwatch/k8s/`, numbered like life):

- `00-namespace` `01-pvc` (5 Gi) `02-db` (mariadb:11.8, Recreate, hardened)
  `03-app` `04-ingress` `05-networkpolicy` (db-from-app-only) + `secret.sh`.
- **VPN-only** exactly like messages: DNS `A fleetwatch → 10.100.0.2`
  (`proxied = false`, in `code/dns/xinutec_org.tf`, copy the `org_messages`
  block) + cert-manager issuer **`letsencrypt-dns`** (DNS-01; the ClusterIssuer +
  cloudflare token secret already exist on isis from messages — reuse, don't
  recreate).
- `whitelist-source-range: "10.100.0.0/24"` was TRIED on deploy (2026-07-03) and
  REMOVED: behind k3s servicelb the client's WireGuard source IP is SNAT'd before
  nginx sees it, so the rule 403s even a legit VPN client. So fleetwatch runs at
  messages-parity — DNS-only concealment; the write path is token-gated regardless.
  True L7 VPN-only would need a cluster-wide `externalTrafficPolicy: Local` +
  forwarded-headers change affecting every service, deliberately not done.

**Ingest tokens**: env var in the k8s secret, `FLEETWATCH_TOKENS="mac-mini:<random>"`
(comma-separated `source:token` pairs), generated by `secret.sh` from
`/dev/urandom` like life's secrets. No token table, no management UI; adding a
producer = edit secret + rollout. Constant-time comparison on the server. On the
Mac the token lives in a `0600` file under `~/.config/fleetwatch/`, read by the pusher
— never in a repo, never in launchd plist XML.

**Reads were unauthenticated in v1** (VPN + whitelist as the gate).
**Superseded**: with the whitelist unenforceable (above), reads now require a
Nextcloud login — fleetwatch keeps its own DB-backed sessions (`src/session.rs`),
touching NC only at login. The one exception is the read token on
`/api/problems` (§5). Writes need the ingest token.

**The login in progress rides in a signed cookie** (`src/pending_login.rs`), not
in a `state`-keyed map. NC's `oauth2/authorize` does not redirect back to a
browser with no NC session — it bounces to its own Login Flow and drops every
query parameter, returning to the callback with `state=` **empty**. Keyed on
`state`, such a login can never complete: found 2026-07-28, when the Android
wrapper's WebView lost its NC cookie and was locked out for good. The cookie
binds the login to the browser that started it — the property `state` was there
to prove — and, being self-contained, also survives a pod restart mid-login.
`state` is still sent and still checked whenever NC returns it.

CI: this repo's own `.github/workflows/build.yml` — a `verify` (clippy + cargo
test against a throwaway MariaDB) and `fe-verify` (angular-eslint + unit tests +
prod build) gate, then an `image` job that builds and pushes `xinutec/fleetwatch:latest`.
Deploy stays manual `kubectl apply` on isis (isis is not Flux-managed).

## 7. First producer: the Mac mini

Built. Three parts, all in `xinutec-infra/mac-mini/` next to the tools they
wrap; the live registry of collectors + cadences is `COLLECTORS` in
`fleetwatch_push.py`.

### 7.1 `--json` emitters

- **`_checks.py`** grows a `--json` mode used by both `fleet_health.py` and
  `doc_checks.py`: serialize `Checker.results` (`CheckResult` →
  check objects; `Verdict` → lowercase strings) plus run metadata into the §4.1
  report shape. One shared implementation, ~30 lines. The existing human output is
  untouched; `--json` writes the report to stdout (or `--json-out FILE`).
- **`dev_lint/fleet.py`** grows `--json` similarly: `Run(name, status, detail)` →
  one check per repo per stage (section `lint` / `verify`, label = repo name,
  `value` = violation count where parseable, `detail` = captured output);
  consistency findings → section `consistency` (see 7.3).
- **dev-lint itself gets no emitter in v1** — `fleet.py` already captures its
  per-repo output and count; per-violation structure can come later if the
  drill-down wants it.

### 7.2 `fleetwatch_push.py` (spool + upload)

```
fleetwatch_push.py run <collector> -- <command...>   # run tool, capture JSON, spool, flush
fleetwatch_push.py flush                             # retry everything in the spool
```

- Runs the collector with `--json`, stamps `id` (ULID) + `duration_ms`, writes the
  report to `~/.local/state/fleetwatch/spool/<id>.json`, then attempts upload of every
  spooled file to `https://fleetwatch.xinutec.org/api/reports`; deletes on 2xx
  (200-duplicate counts as success — that's the idempotency working).
- Survives isis downtime, VPN flaps, and Mac sleep: nothing is lost, order doesn't
  matter, `collected_at` stays true. At-least-once + server dedupe on `id` =
  effectively exactly-once.
- A run whose collector *crashes* (non-zero exit and no JSON) spools a synthetic
  single-check report (`section: collector, verdict: fail, detail: stderr tail`) —
  the platform must show tool breakage, not go silent.

### 7.3 `fleet.py` stable-key refactor (prerequisite, small)

Done. Consistency findings were `(level, "free-form message with specifics")` —
unusable as trend keys — so each consistency check's function name became its
stable id: `section="consistency", label=check_id, subject=repo,
observed=message`. Human output unchanged. This was the only producer-side
refactor the design needed.

### 7.4 Schedule (launchd)

One agent per collector invoking `fleetwatch_push.py run <collector>`, generated
by home-manager (`hm-agents.nix`); the cadence lives in the `COLLECTORS`
registry, not here, so it can't drift from this doc. The SSH-keys risk resolved
as predicted: the agents are login-session LaunchAgents with a folded nix
toolbox on PATH (`fleetwatch-tools` — the scripts' nix-shell shebangs hang under
launchd).

## 8. UI

Angular 22 zoneless + Angular Material (M3 tokens per the DL-SCSS rules), signals,
mobile-first. Served single-origin by the axum binary; Android single-WebView
wrapper at `android/` (`org.xinutec.fleetwatch`, hardcoded
`FLEETWATCH_URL`, recall `#android` nix shell, `deploy.sh` sideload) — identical to
life's wrapper.

Views, in the order you'd reach for them:

1. **Overview** (home) — one tile per (source, collector): worst-verdict colour,
   pass/warn/fail counts, "12 min ago" freshness, overdue/silent badge. Everything
   green fits on one phone screen.
2. **Problems** — flat list of every current fail/warn across all collectors +
   overdue producers, each linking into its report context. The "something's red,
   what is it" view; arguably the real home screen when it's non-empty.
3. **Collector detail** — the latest report rendered as the CLI renders it:
   sections, ✓/✗/⚠ lines, observed/expected, `ref` shown as `file:line`. Familiar
   by construction. Each check line links to its history.
4. **Check history** — verdict timeline strip ("red since Tuesday 14:00") and, for
   checks with `value`, a line chart (disk %, cert days, snapshot count,
   violation counts trending to zero).
5. **Runs** — report list per collector with duration + ok, for "did it even
   run". Built as the "Recent runs" list on the history view, not a separate page.

Charts (resolved): the history chart is hand-rolled SVG
(`features/history/chart.ts`, pure functions + a slim component) — no chart lib.
Tiles carry verdict counts, not sparklines.

## 9. Milestones

All built; 5 stays open-ended by design.

1. **Platform skeleton** — `code/kubes/fleetwatch/` cloned from life: schema,
   migrations, ingest + all five GET routes, token auth, retention task, tests
   (`tests/` public-API style), verify.sh, k8s manifests, DNS record, CI job.
   Deployed and accepting `curl` reports. Add `fleetwatch` to `~/Code/check` REPOS.
2. **Mac producer** — `_checks.py --json`, `fleet.py` stable keys + `--json`,
   `fleetwatch_push.py`, launchd timers. Real data flowing.
3. **UI core** — overview, problems, collector detail. Android wrapper. This is
   the point it replaces squinting at a terminal.
4. **History** — timeline strips + charts + runs view.
5. **More producers** (open-ended) — odin backup job reports, amun/isis
   in-cluster checks, anything else; zero service changes by design.

## 10. Open questions / risks

- ~~**whitelist-source-range vs servicelb**~~ — RESOLVED (2026-07-03): the client
  IP does NOT survive servicelb (SNAT'd); annotation removed, running at
  messages-parity. Token still gates writes.
- ~~**launchd + SSH keys**~~ — RESOLVED: login-session LaunchAgents + folded
  toolbox (§7.4).
- ~~**`fleet.py` key refactor**~~ — DONE (§7.3).
- **Label stability is a convention, not enforced.** A producer that embeds a
  value in a label silently forks its own history. Mitigation: a dev-lint rule is
  overkill for now; instead the history view makes breakage visible (series
  stops), and emitter tests pin the label sets.
- **Clock skew**: `collected_at` is producer-stamped; the Mac is NTP-synced so
  this is theoretical, but the phone-mic lesson says record `received_at` anyway
  (done, §4.1).
