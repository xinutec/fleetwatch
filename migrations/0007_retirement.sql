-- Retiring a producer: saying "this (source, collector) is finished" WITHOUT
-- discarding what it measured.
--
-- ⚠ THE PROBLEM THIS SOLVES COST HISTORY. When a collector moves host, the old
-- (source, collector) keeps its last `report` row for ever and `overview()`
-- reports it silent for ever. Report rows are deliberately never deleted
-- (retention.rs keeps the summary tier permanently), the only DELETE route is
-- /api/mutes/{id}, and a mute cannot cover it: mutes key on
-- (source, collector, label) and a stale entry has no label, `problems()`
-- applies the mute set to check rows only, and every mute is clamped to a TTL.
-- So the only remedy was root ssh -> kubectl exec -> a hand-written DELETE. The
-- picade move on 2026-08-11 took 7 days and one of those deletes to silence,
-- and it destroyed 1,757 report rows and 61,468 check rows — the cabinets'
-- entire pre-move record. The SQL could not separate "stop reporting this" from
-- "forget this ever ran".
--
-- A retirement is a READ-TIME OVERLAY, exactly like a mute: the producer's
-- stored facts are never rewritten, and every row it ever wrote stays
-- queryable. `problems()` simply stops counting it as stale.
--
-- ⚠ IT HAS NO EXPIRY, AND THAT IS THE ONE WAY IT DIFFERS FROM A MUTE. 0003 says
-- every mute must expire because "intentional silence that cannot rot into a
-- forgotten blind spot is the whole point" — right for a cabinet that will come
-- back, wrong for a producer that will not. The blind-spot risk a TTL guards
-- against is answered here by a different mechanism: a retired producer that
-- reports AGAIN is surfaced loudly (`Problems.returned`) rather than quietly
-- un-retiring itself. Convenience would be to un-retire; being loud is what
-- catches a host coming back that nobody meant to bring back.
--
-- Match key is (source, collector) — a producer's identity, with no label,
-- because staleness is a fact about the producer and not about any one check.
CREATE TABLE IF NOT EXISTS retirement (
    source     VARCHAR(64)  NOT NULL,
    collector  VARCHAR(64)  NOT NULL,
    reason     TEXT         NOT NULL,
    created_by VARCHAR(128) NOT NULL,               -- Nextcloud user id
    retired_at DATETIME(3)  NOT NULL,
    PRIMARY KEY (source, collector)
);
