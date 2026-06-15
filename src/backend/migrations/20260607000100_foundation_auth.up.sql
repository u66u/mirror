CREATE TABLE owner_accounts (
    id SMALLINT PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    public_id UUID NOT NULL UNIQUE,
    display_name TEXT NOT NULL CHECK (
        length(display_name) BETWEEN 1 AND 120
    ),
    password_hash TEXT NOT NULL CHECK (length(password_hash) > 0),
    password_hash_alg TEXT NOT NULL DEFAULT 'argon2id' CHECK (
        length(password_hash_alg) BETWEEN 1 AND 64
    ),
    totp_secret_ciphertext BYTEA,
    totp_enabled_at TIMESTAMPTZ,
    recovery_codes_version INTEGER NOT NULL DEFAULT 0 CHECK (
        recovery_codes_version >= 0
    ),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    disabled_at TIMESTAMPTZ,
    CHECK (
        totp_secret_ciphertext IS NOT NULL
        OR totp_enabled_at IS NULL
    )
);

CREATE TABLE sessions (
    id UUID PRIMARY KEY,
    owner_id SMALLINT NOT NULL REFERENCES owner_accounts(id) ON DELETE CASCADE,
    token_hash BYTEA NOT NULL UNIQUE CHECK (octet_length(token_hash) >= 32),
    token_hash_alg TEXT NOT NULL DEFAULT 'blake3' CHECK (
        length(token_hash_alg) BETWEEN 1 AND 64
    ),
    csrf_token_hash BYTEA CHECK (
        csrf_token_hash IS NULL
        OR octet_length(csrf_token_hash) >= 32
    ),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen_at TIMESTAMPTZ,
    expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ,
    revocation_reason TEXT,
    user_agent TEXT,
    client_ip INET,
    device_name TEXT,
    CHECK (expires_at > created_at)
);

CREATE INDEX sessions_owner_active_idx
    ON sessions(owner_id, expires_at DESC)
    WHERE revoked_at IS NULL;

CREATE TABLE device_tokens (
    id UUID PRIMARY KEY,
    owner_id SMALLINT NOT NULL REFERENCES owner_accounts(id) ON DELETE CASCADE,
    token_hash BYTEA NOT NULL UNIQUE CHECK (octet_length(token_hash) >= 32),
    token_hash_alg TEXT NOT NULL DEFAULT 'blake3' CHECK (
        length(token_hash_alg) BETWEEN 1 AND 64
    ),
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 120),
    created_by_session_id UUID REFERENCES sessions(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_used_at TIMESTAMPTZ,
    expires_at TIMESTAMPTZ,
    revoked_at TIMESTAMPTZ,
    revocation_reason TEXT,
    user_agent TEXT,
    client_ip INET,
    CHECK (expires_at IS NULL OR expires_at > created_at)
);

CREATE INDEX device_tokens_owner_active_idx
    ON device_tokens(owner_id, created_at DESC)
    WHERE revoked_at IS NULL;

CREATE TABLE audit_events (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    actor_kind TEXT NOT NULL CHECK (
        actor_kind IN ('owner', 'device', 'system', 'share', 'anonymous')
    ),
    actor_owner_id SMALLINT REFERENCES owner_accounts(id) ON DELETE SET NULL,
    actor_session_id UUID REFERENCES sessions(id) ON DELETE SET NULL,
    actor_device_token_id UUID REFERENCES device_tokens(id) ON DELETE SET NULL,
    action TEXT NOT NULL CHECK (length(action) BETWEEN 1 AND 120),
    outcome TEXT NOT NULL CHECK (
        outcome IN ('success', 'failure', 'blocked')
    ),
    target_kind TEXT CHECK (
        target_kind IS NULL
        OR length(target_kind) BETWEEN 1 AND 120
    ),
    target_id TEXT,
    client_ip INET,
    user_agent TEXT,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb CHECK (
        jsonb_typeof(metadata) = 'object'
    ),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX audit_events_created_at_idx
    ON audit_events(created_at DESC);

CREATE INDEX audit_events_action_created_at_idx
    ON audit_events(action, created_at DESC);

CREATE TABLE rate_limit_buckets (
    action TEXT NOT NULL CHECK (length(action) BETWEEN 1 AND 120),
    key_hash BYTEA NOT NULL CHECK (octet_length(key_hash) >= 16),
    key_hash_alg TEXT NOT NULL DEFAULT 'hmac-sha256' CHECK (
        length(key_hash_alg) BETWEEN 1 AND 64
    ),
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    window_start_at TIMESTAMPTZ NOT NULL,
    blocked_until TIMESTAMPTZ,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (action, key_hash),
    CHECK (expires_at > window_start_at)
);

CREATE INDEX rate_limit_buckets_expires_at_idx
    ON rate_limit_buckets(expires_at);
