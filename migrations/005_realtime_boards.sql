-- Realtime flag: boards backed by Redis for high-throughput score submission.

ALTER TABLE boards ADD COLUMN realtime boolean NOT NULL DEFAULT false;

-- Realtime requires accumulative + scored.
ALTER TABLE boards ADD COLUMN clear_on_snapshot boolean NOT NULL DEFAULT false;

ALTER TABLE boards ADD CONSTRAINT realtime_requires_accumulative_scored
  CHECK (NOT realtime OR (accumulative AND board_type = 'scored'));

-- clear_on_snapshot only makes sense on realtime boards.
ALTER TABLE boards ADD CONSTRAINT clear_on_snapshot_requires_realtime
  CHECK (NOT clear_on_snapshot OR realtime);
