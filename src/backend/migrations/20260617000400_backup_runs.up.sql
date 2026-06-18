CREATE TABLE backup_runs (
    id UUID PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('restic')),
    status TEXT NOT NULL CHECK (
        status IN ('planned', 'running', 'succeeded', 'failed', 'restore_check_succeeded', 'restore_check_failed')
    ),
    snapshot_id TEXT CHECK (
        snapshot_id IS NULL
        OR length(snapshot_id) BETWEEN 1 AND 200
    ),
    repository_hint TEXT CHECK (
        repository_hint IS NULL
        OR length(repository_hint) BETWEEN 1 AND 500
    ),
    error_message TEXT CHECK (
        error_message IS NULL
        OR length(error_message) BETWEEN 1 AND 1000
    ),
    manifest JSONB NOT NULL DEFAULT '{}'::jsonb CHECK (
        jsonb_typeof(manifest) = 'object'
    ),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at TIMESTAMPTZ
);

CREATE INDEX backup_runs_created_at_idx
    ON backup_runs(created_at DESC);

CREATE INDEX backup_runs_status_created_at_idx
    ON backup_runs(status, created_at DESC);
