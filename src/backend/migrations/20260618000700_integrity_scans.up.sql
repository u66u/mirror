ALTER TABLE derivatives
    ADD COLUMN blake3_hash TEXT CHECK (
        blake3_hash IS NULL OR length(blake3_hash) = 64
    );

CREATE TABLE integrity_scan_runs (
    id UUID PRIMARY KEY,
    status TEXT NOT NULL CHECK (
        status IN ('queued', 'running', 'succeeded', 'failed')
    ),
    missing_originals JSONB NOT NULL DEFAULT '[]'::jsonb CHECK (
        jsonb_typeof(missing_originals) = 'array'
    ),
    corrupt_originals JSONB NOT NULL DEFAULT '[]'::jsonb CHECK (
        jsonb_typeof(corrupt_originals) = 'array'
    ),
    missing_derivatives JSONB NOT NULL DEFAULT '[]'::jsonb CHECK (
        jsonb_typeof(missing_derivatives) = 'array'
    ),
    corrupt_derivatives JSONB NOT NULL DEFAULT '[]'::jsonb CHECK (
        jsonb_typeof(corrupt_derivatives) = 'array'
    ),
    error_message TEXT CHECK (
        error_message IS NULL OR length(error_message) BETWEEN 1 AND 1000
    ),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    started_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX integrity_scan_runs_created_at_idx
    ON integrity_scan_runs(created_at DESC, id DESC);
