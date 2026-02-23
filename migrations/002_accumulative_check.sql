-- Enforce that only scored boards can be accumulative.
-- Application code already validates this, but a CHECK constraint
-- provides defense-in-depth against direct SQL inserts.
ALTER TABLE boards
    ADD CONSTRAINT chk_accumulative_scored
    CHECK (accumulative = false OR board_type = 'scored');
