ALTER TABLE upload_sessions
ADD COLUMN client_upload_key UUID;

CREATE UNIQUE INDEX upload_sessions_owner_client_key_idx
ON upload_sessions(owner_id, client_upload_key)
WHERE client_upload_key IS NOT NULL;
