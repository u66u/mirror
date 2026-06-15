DROP INDEX upload_sessions_owner_client_key_idx;

ALTER TABLE upload_sessions
DROP COLUMN client_upload_key;
