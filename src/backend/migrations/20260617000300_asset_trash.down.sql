DROP INDEX assets_owner_trashed_at_idx;
DROP INDEX assets_owner_active_created_at_idx;

ALTER TABLE assets
    DROP COLUMN trashed_at;
