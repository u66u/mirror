DROP TRIGGER IF EXISTS sync_asset_embeddings_asset_fields_trg ON assets;
DROP FUNCTION IF EXISTS sync_asset_embeddings_asset_fields();

DROP INDEX IF EXISTS asset_embeddings_owner_model_active_idx;

ALTER TABLE asset_embeddings
    DROP CONSTRAINT IF EXISTS asset_embeddings_owner_id_fkey,
    DROP CONSTRAINT IF EXISTS asset_embeddings_dimension_matches_chk;

ALTER TABLE asset_embeddings
    DROP COLUMN IF EXISTS asset_trashed_at,
    DROP COLUMN IF EXISTS asset_created_at,
    DROP COLUMN IF EXISTS asset_public_id,
    DROP COLUMN IF EXISTS owner_id;
