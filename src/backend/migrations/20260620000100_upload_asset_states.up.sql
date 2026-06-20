ALTER TABLE upload_sessions
    ADD COLUMN promotion_started_at TIMESTAMPTZ,
    ADD COLUMN promoted_at TIMESTAMPTZ,
    ADD COLUMN failed_at TIMESTAMPTZ,
    ADD COLUMN failure_reason TEXT CHECK (
        failure_reason IS NULL OR length(failure_reason) BETWEEN 1 AND 1000
    );

DO $$
DECLARE
    constraint_name TEXT;
BEGIN
    FOR constraint_name IN
        SELECT conname
        FROM pg_constraint
        WHERE conrelid = 'upload_sessions'::regclass
          AND contype = 'c'
          AND pg_get_constraintdef(oid) LIKE '%status%'
    LOOP
        EXECUTE format('ALTER TABLE upload_sessions DROP CONSTRAINT %I', constraint_name);
    END LOOP;
END $$;

ALTER TABLE upload_sessions
    ADD CONSTRAINT upload_sessions_status_valid CHECK (
        status IN ('open', 'verified', 'promoting', 'completed', 'failed', 'cancelled')
    ),
    ADD CONSTRAINT upload_sessions_completed_at_state CHECK (
        completed_at IS NULL OR status IN ('verified', 'promoting', 'completed')
    ),
    ADD CONSTRAINT upload_sessions_cancelled_at_state CHECK (
        cancelled_at IS NULL OR status = 'cancelled'
    ),
    ADD CONSTRAINT upload_sessions_promotion_started_at_state CHECK (
        promotion_started_at IS NULL OR status IN ('promoting', 'completed', 'failed')
    ),
    ADD CONSTRAINT upload_sessions_promoted_at_state CHECK (
        promoted_at IS NULL OR status = 'completed'
    ),
    ADD CONSTRAINT upload_sessions_failed_at_state CHECK (
        failed_at IS NULL OR status = 'failed'
    );

ALTER TABLE assets
    ADD COLUMN status TEXT NOT NULL DEFAULT 'ready' CHECK (
        status IN ('pending_original', 'original_available', 'processing', 'ready', 'failed', 'corrupt')
    );

CREATE INDEX assets_owner_status_idx
    ON assets(owner_id, status, created_at DESC);
