-- Expiring mutes: a deliberate, audited, time-limited suppression of a known
-- problem. When a peer/host is down on purpose (e.g. a Pi powered off for a
-- while), its check honestly keeps reporting fail — the producer's verdict is a
-- fact and is never rewritten. A mute is a READ-TIME overlay: the problems view
-- excludes a muted check (so the notifier stays quiet) and the overview tile
-- stops counting it as a fault, but the stored history still shows the truth.
--
-- Every mute MUST expire (expires_at is NOT NULL): intentional silence that
-- cannot rot into a forgotten blind spot is the whole point. When it lapses the
-- problem simply reappears — no reticketing, no cleanup. `reason` and
-- `created_by` are mandatory so a mute is always attributable.
--
-- Match key is (source, collector, label) — a check's identity minus section
-- (label is unique within a collector; section is only presentation grouping).
CREATE TABLE IF NOT EXISTS mute (
    id         VARCHAR(26)  NOT NULL PRIMARY KEY,   -- ULID
    source     VARCHAR(64)  NOT NULL,
    collector  VARCHAR(64)  NOT NULL,
    label      VARCHAR(255) NOT NULL,
    reason     TEXT         NOT NULL,
    created_by VARCHAR(128) NOT NULL,               -- Nextcloud user id
    created_at DATETIME(3)  NOT NULL,
    expires_at DATETIME(3)  NOT NULL,               -- hard stop; the mute self-heals
    INDEX idx_mute_active (source, collector, label, expires_at)
);
