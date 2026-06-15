CREATE TABLE upload_sessions (
    id UUID PRIMARY KEY,
    owner_id SMALLINT NOT NULL REFERENCES owner_accounts(id) ON DELETE CASCADE,
    original_filename TEXT NOT NULL CHECK (
        length(original_filename) BETWEEN 1 AND 255
    ),
    expected_size BIGINT NOT NULL CHECK (expected_size > 0),
    expected_blake3 TEXT NOT NULL CHECK (
        length(expected_blake3) = 64
    ),
    media_type TEXT NOT NULL CHECK (
        media_type IN ('image/jpeg', 'image/png', 'image/gif', 'image/webp')
    ),
    status TEXT NOT NULL DEFAULT 'open' CHECK (
        status IN ('open', 'verified', 'cancelled')
    ),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at TIMESTAMPTZ,
    cancelled_at TIMESTAMPTZ,
    CHECK (completed_at IS NULL OR status = 'verified'),
    CHECK (cancelled_at IS NULL OR status = 'cancelled')
);

CREATE INDEX upload_sessions_owner_status_idx
    ON upload_sessions(owner_id, status, created_at DESC);

CREATE TABLE upload_parts (
    upload_id UUID NOT NULL REFERENCES upload_sessions(id) ON DELETE CASCADE,
    part_index INTEGER NOT NULL CHECK (part_index >= 0),
    size_bytes BIGINT NOT NULL CHECK (size_bytes > 0),
    storage_key TEXT NOT NULL CHECK (length(storage_key) BETWEEN 1 AND 512),
    blake3_hash TEXT NOT NULL CHECK (length(blake3_hash) = 64),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (upload_id, part_index)
);
