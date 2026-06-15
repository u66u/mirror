CREATE TABLE asset_metadata (
    asset_id UUID PRIMARY KEY REFERENCES assets(id) ON DELETE CASCADE,
    width INTEGER CHECK (width IS NULL OR width > 0),
    height INTEGER CHECK (height IS NULL OR height > 0),
    raw JSONB NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(raw) = 'object'),
    extracted_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE derivatives (
    id UUID PRIMARY KEY,
    asset_id UUID NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('thumbnail', 'preview')),
    format TEXT NOT NULL CHECK (format IN ('webp')),
    generator_version TEXT NOT NULL CHECK (length(generator_version) BETWEEN 1 AND 120),
    source_blake3 TEXT NOT NULL CHECK (length(source_blake3) = 64),
    storage_key TEXT NOT NULL CHECK (length(storage_key) BETWEEN 1 AND 512),
    width INTEGER NOT NULL CHECK (width > 0),
    height INTEGER NOT NULL CHECK (height > 0),
    size_bytes BIGINT NOT NULL CHECK (size_bytes > 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (asset_id, kind, format, generator_version)
);

CREATE INDEX derivatives_asset_kind_idx
    ON derivatives(asset_id, kind);
