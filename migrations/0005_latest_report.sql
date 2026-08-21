-- The newest report per (source, collector), maintained on ingest.
--
-- Both read views need "the latest report from each producer", and both derived
-- it the same way: ROW_NUMBER() OVER (PARTITION BY source, collector ORDER BY
-- collected_at DESC, id DESC) across the WHOLE report table, on every request.
-- That cost grows with history (~1,400 new reports a day here) to re-answer a
-- question that only changes when a report arrives; under concurrent polling it
-- exhausted the connection pool and 500'd the dashboard. So the answer is now
-- stored where it is decided — ingest writes this pointer in the same
-- transaction as the report, and the views join it.
--
-- The ordering materialised here is EXACTLY the window's: a newer collected_at
-- wins, and on a tie the larger id wins. `repo::rebuild_latest_report` recomputes
-- the table from `report` with that same spelling; the backfill below is its SQL.
--
-- ON DELETE CASCADE rather than repoint-to-previous: retention keeps `report`
-- rows for ever (report::retention), so nothing deletes them in normal service.
-- Dropping a producer's reports by hand drops its pointer and the producer
-- leaves the views, which is what retiring one should do. Deleting only the
-- NEWEST report of a live producer also drops the pointer instead of falling
-- back, so run rebuild_latest_report after that kind of surgery.
CREATE TABLE IF NOT EXISTS latest_report (
    source       VARCHAR(64)  NOT NULL,
    collector    VARCHAR(64)  NOT NULL,
    report_id    VARCHAR(26)  NOT NULL,
    collected_at DATETIME(3)  NOT NULL,               -- the pointed-at report's
    PRIMARY KEY (source, collector),
    INDEX idx_latest_report_id (report_id),
    CONSTRAINT fk_latest_report FOREIGN KEY (report_id)
        REFERENCES report (id) ON DELETE CASCADE
) CHARACTER SET utf8mb4;

-- Backfill from the history that is already stored. Idempotent (the upsert), so
-- re-running this migration on a populated table is a no-op that agrees.
INSERT INTO latest_report (source, collector, report_id, collected_at)
SELECT x.source, x.collector, x.id, x.collected_at
  FROM (SELECT id, source, collector, collected_at,
               ROW_NUMBER() OVER (PARTITION BY source, collector
                                  ORDER BY collected_at DESC, id DESC) rn
          FROM report) x
 WHERE x.rn = 1
    ON DUPLICATE KEY UPDATE report_id    = VALUES(report_id),
                            collected_at = VALUES(collected_at);
