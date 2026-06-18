ALTER TABLE upload_sessions
    DROP CONSTRAINT upload_sessions_media_type_check,
    ADD CONSTRAINT upload_sessions_media_type_check CHECK (
        media_type IN ('image/jpeg', 'image/png', 'image/gif', 'image/webp', 'video/mp4')
    );

ALTER TABLE originals
    DROP CONSTRAINT originals_media_type_check,
    ADD CONSTRAINT originals_media_type_check CHECK (
        media_type IN ('image/jpeg', 'image/png', 'image/gif', 'image/webp', 'video/mp4')
    );
