CREATE VIEW asset_display_view AS
SELECT
    a.id as asset_id,
    a.public_id as asset_public_id,
    a.owner_id as asset_owner_id,
    a.created_at,
    a.trashed_at,
    a.favorite_at,
    o.blake3_hash,
    o.media_type,
    o.size_bytes,
    s.original_filename,
    t.format as thumbnail_format,
    t.width as thumbnail_width,
    t.height as thumbnail_height,
    p.format as preview_format,
    p.width as preview_width,
    p.height as preview_height
FROM assets a
JOIN originals o ON o.id = a.original_id
LEFT JOIN LATERAL (
    SELECT original_filename
    FROM asset_sources
    WHERE asset_id = a.id
    ORDER BY created_at ASC
    LIMIT 1
) s ON true
LEFT JOIN LATERAL (
    SELECT format, width, height
    FROM derivatives
    WHERE asset_id = a.id
      AND kind = 'thumbnail'
    ORDER BY created_at DESC
    LIMIT 1
) t ON true
LEFT JOIN LATERAL (
    SELECT format, width, height
    FROM derivatives
    WHERE asset_id = a.id
      AND kind = 'preview'
    ORDER BY created_at DESC
    LIMIT 1
) p ON true;