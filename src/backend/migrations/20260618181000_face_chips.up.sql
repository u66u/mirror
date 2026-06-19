ALTER TABLE face_occurrences
    ADD COLUMN chip_storage_key TEXT,
    ADD COLUMN chip_width INTEGER CHECK (chip_width IS NULL OR chip_width > 0),
    ADD COLUMN chip_height INTEGER CHECK (chip_height IS NULL OR chip_height > 0),
    ADD COLUMN chip_format TEXT CHECK (chip_format IS NULL OR chip_format IN ('webp'));

ALTER TABLE face_occurrences
    ADD CONSTRAINT face_occurrences_chip_complete_check
    CHECK (
        (chip_storage_key IS NULL AND chip_width IS NULL AND chip_height IS NULL AND chip_format IS NULL)
        OR
        (chip_storage_key IS NOT NULL AND chip_width IS NOT NULL AND chip_height IS NOT NULL AND chip_format IS NOT NULL)
    );
