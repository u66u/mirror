ALTER TABLE face_occurrences
    DROP CONSTRAINT IF EXISTS face_occurrences_chip_complete_check;

ALTER TABLE face_occurrences
    DROP COLUMN IF EXISTS chip_format,
    DROP COLUMN IF EXISTS chip_height,
    DROP COLUMN IF EXISTS chip_width,
    DROP COLUMN IF EXISTS chip_storage_key;
