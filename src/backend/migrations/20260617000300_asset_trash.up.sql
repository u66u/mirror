ALTER TABLE assets
    ADD COLUMN trashed_at TIMESTAMPTZ;

CREATE INDEX assets_owner_active_created_at_idx
    ON assets(owner_id, created_at DESC, public_id DESC)
    WHERE trashed_at IS NULL;

CREATE INDEX assets_owner_trashed_at_idx
    ON assets(owner_id, trashed_at DESC)
    WHERE trashed_at IS NOT NULL;
