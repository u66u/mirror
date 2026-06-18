CREATE EXTENSION IF NOT EXISTS vector;

CREATE TABLE asset_embeddings (
    asset_id UUID NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    model_pack_id UUID NOT NULL REFERENCES model_packs(id) ON DELETE CASCADE,
    embedding vector NOT NULL,
    embedding_dimension INTEGER NOT NULL CHECK (
        embedding_dimension BETWEEN 1 AND 32768
    ),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (asset_id, model_pack_id)
);

CREATE INDEX asset_embeddings_model_pack_idx
    ON asset_embeddings(model_pack_id);
