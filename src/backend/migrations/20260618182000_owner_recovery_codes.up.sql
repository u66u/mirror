CREATE TABLE owner_recovery_codes (
    id UUID PRIMARY KEY,
    owner_id SMALLINT NOT NULL REFERENCES owner_accounts(id) ON DELETE CASCADE,
    code_hash BYTEA NOT NULL UNIQUE CHECK (octet_length(code_hash) >= 32),
    code_hash_alg TEXT NOT NULL DEFAULT 'sha256' CHECK (
        length(code_hash_alg) BETWEEN 1 AND 64
    ),
    version INTEGER NOT NULL CHECK (version >= 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    used_at TIMESTAMPTZ,
    CHECK (used_at IS NULL OR used_at >= created_at)
);

CREATE INDEX owner_recovery_codes_owner_active_idx
    ON owner_recovery_codes(owner_id, version, created_at)
    WHERE used_at IS NULL;
