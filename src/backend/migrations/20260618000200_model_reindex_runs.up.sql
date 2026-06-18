CREATE TABLE model_reindex_runs (
    id UUID PRIMARY KEY,
    model_pack_id UUID NOT NULL REFERENCES model_packs(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('semantic_image_text', 'face_identity')),
    status TEXT NOT NULL DEFAULT 'queued' CHECK (
        status IN ('queued', 'running', 'succeeded', 'failed', 'canceled')
    ),
    total_assets INTEGER NOT NULL CHECK (total_assets >= 0),
    queued_assets INTEGER NOT NULL CHECK (queued_assets >= 0),
    processed_assets INTEGER NOT NULL DEFAULT 0 CHECK (processed_assets >= 0),
    failed_assets INTEGER NOT NULL DEFAULT 0 CHECK (failed_assets >= 0),
    error_message TEXT CHECK (
        error_message IS NULL
        OR length(error_message) BETWEEN 1 AND 1000
    ),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at TIMESTAMPTZ
);

CREATE INDEX model_reindex_runs_pack_status_idx
    ON model_reindex_runs(model_pack_id, status, created_at DESC);
