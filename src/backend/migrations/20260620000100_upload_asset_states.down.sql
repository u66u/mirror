DROP INDEX assets_owner_status_idx;

ALTER TABLE assets
    DROP COLUMN status;

ALTER TABLE upload_sessions
    DROP CONSTRAINT upload_sessions_status_valid,
    DROP CONSTRAINT upload_sessions_completed_at_state,
    DROP CONSTRAINT upload_sessions_cancelled_at_state,
    DROP CONSTRAINT upload_sessions_promotion_started_at_state,
    DROP CONSTRAINT upload_sessions_promoted_at_state,
    DROP CONSTRAINT upload_sessions_failed_at_state;

UPDATE upload_sessions
SET status = 'verified',
    completed_at = COALESCE(completed_at, promoted_at, promotion_started_at, updated_at),
    updated_at = now()
WHERE status IN ('promoting', 'completed', 'failed');

ALTER TABLE upload_sessions
    ADD CONSTRAINT upload_sessions_status_valid CHECK (
        status IN ('open', 'verified', 'cancelled')
    ),
    ADD CONSTRAINT upload_sessions_completed_at_state CHECK (
        completed_at IS NULL OR status = 'verified'
    ),
    ADD CONSTRAINT upload_sessions_cancelled_at_state CHECK (
        cancelled_at IS NULL OR status = 'cancelled'
    );

ALTER TABLE upload_sessions
    DROP COLUMN promotion_started_at,
    DROP COLUMN promoted_at,
    DROP COLUMN failed_at,
    DROP COLUMN failure_reason;
