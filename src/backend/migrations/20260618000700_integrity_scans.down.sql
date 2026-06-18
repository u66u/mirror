DROP TABLE integrity_scan_runs;

ALTER TABLE derivatives
    DROP COLUMN blake3_hash;
