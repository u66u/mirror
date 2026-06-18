CREATE TABLE asset_shares (
    id UUID PRIMARY KEY,
    public_id UUID NOT NULL UNIQUE,
    owner_id SMALLINT NOT NULL REFERENCES owner_accounts(id) ON DELETE CASCADE,
    asset_id UUID NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    token_hash BYTEA NOT NULL UNIQUE CHECK (octet_length(token_hash) >= 32),
    token_hash_alg TEXT NOT NULL DEFAULT 'sha256' CHECK (
        length(token_hash_alg) BETWEEN 1 AND 64
    ),
    allow_original_download BOOLEAN NOT NULL DEFAULT false,
    expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (expires_at > created_at)
);

CREATE INDEX asset_shares_owner_created_idx
    ON asset_shares(owner_id, created_at DESC);

CREATE INDEX asset_shares_active_token_idx
    ON asset_shares(token_hash, expires_at)
    WHERE revoked_at IS NULL;
