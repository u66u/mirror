DROP TABLE IF EXISTS person_faces;
DROP TABLE IF EXISTS face_embeddings;
DROP TABLE IF EXISTS face_occurrences;
DROP TABLE IF EXISTS people;

ALTER TABLE assets
    DROP CONSTRAINT IF EXISTS assets_id_owner_id_unique;

ALTER TABLE model_reindex_runs
    DROP CONSTRAINT model_reindex_runs_kind_check;

ALTER TABLE model_reindex_runs
    ADD CONSTRAINT model_reindex_runs_kind_check
    CHECK (kind IN ('semantic_image_text', 'face_identity'));

ALTER TABLE model_packs
    DROP CONSTRAINT model_packs_kind_check;

ALTER TABLE model_packs
    ADD CONSTRAINT model_packs_kind_check
    CHECK (kind IN ('semantic_image_text', 'face_identity'));
