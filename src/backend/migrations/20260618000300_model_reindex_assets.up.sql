CREATE TABLE model_reindex_assets (
    reindex_run_id UUID NOT NULL REFERENCES model_reindex_runs(id) ON DELETE CASCADE,
    asset_id UUID NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    status TEXT NOT NULL DEFAULT 'queued' CHECK (
        status IN ('queued', 'done', 'failed')
    ),
    error_message TEXT CHECK (
        error_message IS NULL
        OR length(error_message) BETWEEN 1 AND 1000
    ),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (reindex_run_id, asset_id)
);

CREATE INDEX model_reindex_assets_run_status_idx
    ON model_reindex_assets(reindex_run_id, status);
