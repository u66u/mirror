ALTER TABLE asset_embeddings
    ADD COLUMN owner_id SMALLINT,
    ADD COLUMN asset_public_id UUID,
    ADD COLUMN asset_created_at TIMESTAMPTZ,
    ADD COLUMN asset_trashed_at TIMESTAMPTZ;

UPDATE asset_embeddings ae
SET
    owner_id = a.owner_id,
    asset_public_id = a.public_id,
    asset_created_at = a.created_at,
    asset_trashed_at = a.trashed_at
FROM assets a
WHERE a.id = ae.asset_id;

ALTER TABLE asset_embeddings
    ALTER COLUMN owner_id SET NOT NULL,
    ALTER COLUMN asset_public_id SET NOT NULL,
    ALTER COLUMN asset_created_at SET NOT NULL;

ALTER TABLE asset_embeddings
    ADD CONSTRAINT asset_embeddings_owner_id_fkey
    FOREIGN KEY (owner_id) REFERENCES owner_accounts(id) ON DELETE CASCADE;

ALTER TABLE asset_embeddings
    ADD CONSTRAINT asset_embeddings_dimension_matches_chk
    CHECK (vector_dims(embedding) = embedding_dimension);

CREATE INDEX asset_embeddings_owner_model_active_idx
    ON asset_embeddings(model_pack_id, owner_id, asset_created_at DESC, asset_public_id)
    WHERE asset_trashed_at IS NULL;

CREATE OR REPLACE FUNCTION sync_asset_embeddings_asset_fields()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    UPDATE asset_embeddings
    SET
        owner_id = NEW.owner_id,
        asset_public_id = NEW.public_id,
        asset_created_at = NEW.created_at,
        asset_trashed_at = NEW.trashed_at,
        updated_at = now()
    WHERE asset_id = NEW.id;

    RETURN NEW;
END;
$$;

CREATE TRIGGER sync_asset_embeddings_asset_fields_trg
AFTER UPDATE OF owner_id, public_id, created_at, trashed_at ON assets
FOR EACH ROW
EXECUTE FUNCTION sync_asset_embeddings_asset_fields();

-- pgvector ANN indexes for variable-dimension model packs should be created
-- per active model pack/dimension/metric, for example:
--
-- CREATE INDEX CONCURRENTLY asset_embeddings_<model>_cos_hnsw_idx
-- ON asset_embeddings
-- USING hnsw ((embedding::vector(768)) vector_cosine_ops)
-- WHERE model_pack_id = '<model_pack_uuid>';
--
-- Then the query must use the same expression:
-- ORDER BY embedding::vector(768) <=> $query
--
-- This static migration cannot safely create that index because model pack IDs
-- and dimensions are runtime data.
