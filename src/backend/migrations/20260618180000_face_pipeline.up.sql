ALTER TABLE model_packs
    DROP CONSTRAINT model_packs_kind_check;

ALTER TABLE model_packs
    ADD CONSTRAINT model_packs_kind_check
    CHECK (kind IN ('semantic_image_text', 'face_identity', 'face_detection', 'face_embedding'));

ALTER TABLE model_reindex_runs
    DROP CONSTRAINT model_reindex_runs_kind_check;

ALTER TABLE model_reindex_runs
    ADD CONSTRAINT model_reindex_runs_kind_check
    CHECK (kind IN ('semantic_image_text', 'face_identity', 'face_detection', 'face_embedding'));

ALTER TABLE assets
    ADD CONSTRAINT assets_id_owner_id_unique UNIQUE (id, owner_id);

CREATE TABLE people (
    id UUID PRIMARY KEY,
    owner_id SMALLINT NOT NULL REFERENCES owner_accounts(id) ON DELETE CASCADE,
    display_name TEXT CHECK (
        display_name IS NULL
        OR length(display_name) BETWEEN 1 AND 120
    ),
    review_status TEXT NOT NULL DEFAULT 'unreviewed' CHECK (
        review_status IN ('unreviewed', 'reviewed', 'hidden')
    ),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (id, owner_id)
);

CREATE UNIQUE INDEX people_owner_display_name_idx
    ON people(owner_id, lower(display_name))
    WHERE display_name IS NOT NULL AND review_status <> 'hidden';

CREATE TABLE face_occurrences (
    id UUID PRIMARY KEY,
    asset_id UUID NOT NULL,
    owner_id SMALLINT NOT NULL,
    detection_model_pack_id UUID REFERENCES model_packs(id) ON DELETE SET NULL,
    bbox_left REAL NOT NULL CHECK (bbox_left >= 0 AND bbox_left <= 1),
    bbox_top REAL NOT NULL CHECK (bbox_top >= 0 AND bbox_top <= 1),
    bbox_width REAL NOT NULL CHECK (bbox_width > 0 AND bbox_width <= 1),
    bbox_height REAL NOT NULL CHECK (bbox_height > 0 AND bbox_height <= 1),
    quality REAL CHECK (quality IS NULL OR (quality >= 0 AND quality <= 1)),
    review_state TEXT NOT NULL DEFAULT 'unassigned' CHECK (
        review_state IN ('unassigned', 'assigned', 'hidden')
    ),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (id, owner_id),
    CHECK (bbox_left + bbox_width <= 1.0001),
    CHECK (bbox_top + bbox_height <= 1.0001),
    FOREIGN KEY (asset_id, owner_id)
        REFERENCES assets(id, owner_id) ON DELETE CASCADE
);

CREATE INDEX face_occurrences_owner_asset_idx
    ON face_occurrences(owner_id, asset_id, created_at DESC);

CREATE TABLE face_embeddings (
    face_occurrence_id UUID NOT NULL,
    owner_id SMALLINT NOT NULL,
    model_pack_id UUID NOT NULL REFERENCES model_packs(id) ON DELETE CASCADE,
    embedding vector NOT NULL,
    embedding_dimension INTEGER NOT NULL CHECK (
        embedding_dimension BETWEEN 1 AND 32768
    ),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (face_occurrence_id, model_pack_id),
    CHECK (vector_dims(embedding) = embedding_dimension),
    FOREIGN KEY (face_occurrence_id, owner_id)
        REFERENCES face_occurrences(id, owner_id) ON DELETE CASCADE
);

CREATE INDEX face_embeddings_owner_model_idx
    ON face_embeddings(owner_id, model_pack_id);

CREATE TABLE person_faces (
    person_id UUID NOT NULL,
    face_occurrence_id UUID NOT NULL UNIQUE,
    owner_id SMALLINT NOT NULL,
    review_state TEXT NOT NULL DEFAULT 'assigned' CHECK (
        review_state IN ('assigned', 'hidden')
    ),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (person_id, face_occurrence_id),
    FOREIGN KEY (person_id, owner_id)
        REFERENCES people(id, owner_id) ON DELETE CASCADE,
    FOREIGN KEY (face_occurrence_id, owner_id)
        REFERENCES face_occurrences(id, owner_id) ON DELETE CASCADE
);

CREATE INDEX person_faces_owner_person_idx
    ON person_faces(owner_id, person_id, created_at DESC);
