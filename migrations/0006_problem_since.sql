-- When each currently-failing check identity STARTED failing.
--
-- `problems()` could say what is wrong and never how long it had been wrong, so
-- a check that broke a minute ago looked exactly like one red for a week. On
-- 2026-08-21 `claude-disk`'s trough check had been failing for EIGHT DAYS with
-- the mechanism spelled out in its own expected text — "below which nix deletes
-- store paths mid-build" — and that is what then broke a running test. The whole
-- diagnosis was re-derived from scratch because nothing separated a standing
-- failure from noise.
--
-- ⚠ MAINTAINED ON INGEST, NOT DERIVED ON READ, and that is the whole design.
-- Asking check_result when a run began costs a scan of every row on every
-- request — which is exactly what migration 0005 removed the same day to stop
-- this endpoint exhausting the connection pool. Measured before choosing: the
-- derive-on-read form of the backfill below takes 66s against 574k rows.
--
-- ⚠ The identity here is (source, collector, label) — the same one `mute` uses,
-- and NOT the trend identity, which includes `section`. Note that
-- `idx_check_history` is (source, collector, section, label, collected_at), so
-- section sits between collector and label and that index cannot serve a lookup
-- by this identity. No index is added for it: nothing queries check_result by
-- this identity any more once the table below exists.
CREATE TABLE IF NOT EXISTS problem_since (
    source     VARCHAR(64)  NOT NULL,
    collector  VARCHAR(64)  NOT NULL,
    label      VARCHAR(255) NOT NULL,
    -- The producer's clock at the FIRST report of the current run, not the
    -- server's: the same rule the rest of this schema follows, so an age is
    -- measured against when the fault was observed rather than when we heard.
    first_seen DATETIME(3)  NOT NULL,
    PRIMARY KEY (source, collector, label)
) CHARACTER SET utf8mb4;

-- Backfill: for each identity failing in its collector's LATEST report, the
-- earliest failure since its last pass.
--
-- Scoped to what is currently failing (56 identities here) rather than to
-- everything that ever failed (350). Same answer for every row that matters, and
-- 0.54s against 66s — which matters because migrations run at boot, so a slow
-- one delays every start and every rollout.
INSERT INTO problem_since (source, collector, label, first_seen)
SELECT cur.source, cur.collector, cur.label, cur.started
  FROM (
    SELECT c2.source, c2.collector, c2.label, (
      SELECT MIN(c.collected_at) FROM check_result c
       WHERE c.source = c2.source AND c.collector = c2.collector
         AND c.label = c2.label AND c.verdict IN ('fail', 'warn')
         AND c.collected_at > IFNULL((
               SELECT MAX(p.collected_at) FROM check_result p
                WHERE p.source = c2.source AND p.collector = c2.collector
                  AND p.label = c2.label AND p.verdict IN ('pass', 'skip')
             ), '1970-01-01')
    ) AS started
    FROM (SELECT DISTINCT c.source, c.collector, c.label
            FROM latest_report l
            JOIN check_result c ON c.report_id = l.report_id
           WHERE c.verdict IN ('fail', 'warn')) c2
  ) cur
 WHERE cur.started IS NOT NULL
    ON DUPLICATE KEY UPDATE first_seen = LEAST(first_seen, VALUES(first_seen));
