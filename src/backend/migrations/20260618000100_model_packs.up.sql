CREATE TABLE model_packs (
    id UUID PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('semantic_image_text', 'face_identity')),
    runtime TEXT NOT NULL CHECK (runtime IN ('onnx')),
    model_key TEXT NOT NULL CHECK (length(model_key) BETWEEN 1 AND 120),
    model_revision TEXT NOT NULL CHECK (length(model_revision) BETWEEN 1 AND 200),
    license TEXT NOT NULL CHECK (length(license) BETWEEN 1 AND 200),
    embedding_dimension INTEGER NOT NULL CHECK (
        embedding_dimension BETWEEN 1 AND 32768
    ),
    distance_metric TEXT NOT NULL CHECK (distance_metric IN ('cosine', 'dot', 'l2')),
    manifest JSONB NOT NULL CHECK (jsonb_typeof(manifest) = 'object'),
    status TEXT NOT NULL DEFAULT 'installed' CHECK (
        status IN ('installed', 'active', 'disabled')
    ),
    self_test_status TEXT NOT NULL DEFAULT 'pending' CHECK (
        self_test_status IN ('pending', 'passed', 'failed')
    ),
    self_test_error TEXT CHECK (
        self_test_error IS NULL
        OR length(self_test_error) BETWEEN 1 AND 1000
    ),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    activated_at TIMESTAMPTZ,
    UNIQUE (kind, model_key, model_revision)
);

CREATE UNIQUE INDEX model_packs_one_active_per_kind_idx
    ON model_packs(kind)
    WHERE status = 'active';

CREATE TABLE model_pack_files (
    model_pack_id UUID NOT NULL REFERENCES model_packs(id) ON DELETE CASCADE,
    path TEXT NOT NULL CHECK (
        length(path) BETWEEN 1 AND 300
        AND path NOT LIKE '/%'
        AND path NOT LIKE '%\%'
        AND path NOT LIKE '%..%'
    ),
    sha256 TEXT NOT NULL CHECK (sha256 ~ '^[0-9a-fA-F]{64}$'),
    size_bytes BIGINT NOT NULL CHECK (size_bytes > 0),
    PRIMARY KEY (model_pack_id, path)
);

CREATE INDEX model_packs_kind_status_idx
    ON model_packs(kind, status, updated_at DESC);
