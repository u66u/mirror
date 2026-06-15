CREATE TABLE originals (
    id UUID PRIMARY KEY,
    blake3_hash TEXT NOT NULL UNIQUE CHECK (length(blake3_hash) = 64),
    storage_key TEXT NOT NULL UNIQUE CHECK (length(storage_key) BETWEEN 1 AND 512),
    size_bytes BIGINT NOT NULL CHECK (size_bytes > 0),
    media_type TEXT NOT NULL CHECK (
        media_type IN ('image/jpeg', 'image/png', 'image/gif', 'image/webp')
    ),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE assets (
    id UUID PRIMARY KEY,
    public_id UUID NOT NULL UNIQUE,
    owner_id SMALLINT NOT NULL REFERENCES owner_accounts(id) ON DELETE CASCADE,
    original_id UUID NOT NULL REFERENCES originals(id) ON DELETE RESTRICT,
    favorite_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX assets_owner_created_at_idx
    ON assets(owner_id, created_at DESC);

CREATE TABLE asset_sources (
    id UUID PRIMARY KEY,
    asset_id UUID NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    source_kind TEXT NOT NULL CHECK (
        source_kind IN ('upload', 'import', 'android')
    ),
    upload_id UUID UNIQUE REFERENCES upload_sessions(id) ON DELETE SET NULL,
    original_filename TEXT NOT NULL CHECK (
        length(original_filename) BETWEEN 1 AND 255
    ),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE jobs (
    id UUID PRIMARY KEY,
    kind TEXT NOT NULL CHECK (length(kind) BETWEEN 1 AND 120),
    payload JSONB NOT NULL CHECK (jsonb_typeof(payload) = 'object'),
    idempotency_key TEXT NOT NULL UNIQUE CHECK (
        length(idempotency_key) BETWEEN 1 AND 200
    ),
    status TEXT NOT NULL DEFAULT 'queued' CHECK (
        status IN ('queued', 'leased', 'done', 'dead')
    ),
    priority INTEGER NOT NULL DEFAULT 0,
    run_after TIMESTAMPTZ NOT NULL DEFAULT now(),
    lease_owner TEXT,
    leased_at TIMESTAMPTZ,
    heartbeat_at TIMESTAMPTZ,
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    max_attempts INTEGER NOT NULL DEFAULT 5 CHECK (max_attempts > 0),
    last_error JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX jobs_ready_idx
    ON jobs(priority DESC, run_after ASC, created_at ASC)
    WHERE status = 'queued';
