#!/usr/bin/env bash
set -euo pipefail

if [[ "${1:-}" == "--help" || "${1:-}" == "-h" ]]; then
  cat <<'USAGE'
Usage: scripts/backup_restore_drill.sh

Runs a destructive-free local backup/restore drill against the Docker infra
Postgres on 127.0.0.1:54329. It creates temporary source/destination databases,
temporary storage, and a temporary restic repository, then removes them.

Optional env:
  MIRROR_DRILL_BASE_URL  default postgres://mirror:mirror@127.0.0.1:54329
  MIRROR_DRILL_ADMIN_DB  default postgres
USAGE
  exit 0
fi

need() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "missing required command: $1" >&2
    exit 2
  }
}

need cargo
need jq
need pg_dump
need pg_restore
need psql
need restic

base_url="${MIRROR_DRILL_BASE_URL:-postgres://mirror:mirror@127.0.0.1:54329}"
admin_db="${MIRROR_DRILL_ADMIN_DB:-postgres}"
admin_url="${base_url}/${admin_db}"
suffix="$(date +%s)-$$"
source_db="mirror_drill_source_${suffix}"
restore_db="mirror_drill_restore_${suffix}"
work_dir="$(mktemp -d)"

cleanup() {
  psql "$admin_url" -v ON_ERROR_STOP=0 -c "DROP DATABASE IF EXISTS \"$source_db\"" >/dev/null 2>&1 || true
  psql "$admin_url" -v ON_ERROR_STOP=0 -c "DROP DATABASE IF EXISTS \"$restore_db\"" >/dev/null 2>&1 || true
  rm -rf "$work_dir"
}
trap cleanup EXIT

run_sql() {
  local database="$1"
  local sql="$2"
  psql "${base_url}/${database}" -v ON_ERROR_STOP=1 -c "$sql" >/dev/null
}

scalar_sql() {
  local database="$1"
  local sql="$2"
  psql "${base_url}/${database}" -At -v ON_ERROR_STOP=1 -c "$sql"
}

apply_migrations() {
  local database="$1"
  for migration in src/backend/migrations/*.up.sql; do
    psql "${base_url}/${database}" -v ON_ERROR_STOP=1 -f "$migration" >/dev/null
  done
}

write_file() {
  local root="$1"
  local key="$2"
  local body="$3"
  mkdir -p "$root/$(dirname "$key")"
  printf '%s' "$body" > "$root/$key"
}

psql "$admin_url" -v ON_ERROR_STOP=1 -c "CREATE DATABASE \"$source_db\"" >/dev/null
psql "$admin_url" -v ON_ERROR_STOP=1 -c "CREATE DATABASE \"$restore_db\"" >/dev/null
apply_migrations "$source_db"
apply_migrations "$restore_db"

source_storage="$work_dir/source-storage"
restic_repo="$work_dir/restic-repo"
restore_target="$work_dir/restore-target"
password_file="$work_dir/restic-password"
dump_path="$work_dir/postgres.dump"
mkdir -p \
  "$source_storage/originals/blake3" \
  "$source_storage/derivatives" \
  "$source_storage/model-packs" \
  "$source_storage/staging" \
  "$source_storage/tmp" \
  "$source_storage/logs" \
  "$source_storage/scratch" \
  "$restic_repo" \
  "$restore_target"
printf 'mirror-drill-password' > "$password_file"
RESTIC_REPOSITORY="$restic_repo" RESTIC_PASSWORD_FILE="$password_file" restic init >/dev/null

hash_a="aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
hash_b="bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
original_a="originals/blake3/aa/aa/$hash_a"
original_b="originals/blake3/bb/bb/$hash_b"
derivative_a="derivatives/media-v1-image-webp-1/thumbnail/webp/aa/aa/$hash_a.webp"
model_pack_id="018f4e50-0000-7000-8000-000000000050"
model_file="model-packs/$model_pack_id/models/image_encoder.onnx"
staging_junk="staging/uploads/018f4e50-0000-7000-8000-000000000099/part-0"
tmp_junk="tmp/decoder.bin"
logs_junk="logs/worker.log"
scratch_junk="scratch/transient.bin"

write_file "$source_storage" "$original_a" "original-a"
write_file "$source_storage" "$original_b" "original-b"
write_file "$source_storage" "$derivative_a" "derivative-a"
write_file "$source_storage" "$model_file" "model-file"
write_file "$source_storage" "$staging_junk" "must-not-restore"
write_file "$source_storage" "$tmp_junk" "must-not-restore"
write_file "$source_storage" "$logs_junk" "must-not-restore"
write_file "$source_storage" "$scratch_junk" "must-not-restore"

run_sql "$source_db" "
INSERT INTO owner_accounts (id, public_id, display_name, password_hash)
VALUES (1, '018f4e50-0000-7000-8000-000000000001', 'Drill Owner', 'not-used');
INSERT INTO originals (id, blake3_hash, storage_key, size_bytes, media_type)
VALUES
  ('018f4e50-0000-7000-8000-000000000010', '$hash_a', '$original_a', 10, 'image/jpeg'),
  ('018f4e50-0000-7000-8000-000000000020', '$hash_b', '$original_b', 10, 'image/png');
INSERT INTO assets (id, public_id, owner_id, original_id)
VALUES
  ('018f4e50-0000-7000-8000-000000000011', '018f4e50-0000-7000-8000-000000000012', 1, '018f4e50-0000-7000-8000-000000000010'),
  ('018f4e50-0000-7000-8000-000000000021', '018f4e50-0000-7000-8000-000000000022', 1, '018f4e50-0000-7000-8000-000000000020');
INSERT INTO derivatives (id, asset_id, kind, format, generator_version, source_blake3, storage_key, width, height, size_bytes)
VALUES ('018f4e50-0000-7000-8000-000000000013', '018f4e50-0000-7000-8000-000000000011', 'thumbnail', 'webp', 'media-v1-image-webp-1', '$hash_a', '$derivative_a', 16, 16, 12);
INSERT INTO model_packs (id, kind, runtime, model_key, model_revision, license, embedding_dimension, distance_metric, manifest, status, self_test_status)
VALUES ('$model_pack_id', 'semantic_image_text', 'onnx', 'drill-model', 'rev1', 'test-only', 3, 'cosine', '{\"kind\":\"semantic_image_text\"}'::jsonb, 'installed', 'pending');
INSERT INTO model_pack_files (model_pack_id, path, sha256, size_bytes)
VALUES ('$model_pack_id', 'models/image_encoder.onnx', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 10);
"

backup_output="$(
  MIRROR_DATABASE_URL="${base_url}/${source_db}" \
  MIRROR_STORAGE_ROOT="$source_storage" \
  RESTIC_REPOSITORY="$restic_repo" \
  RESTIC_PASSWORD_FILE="$password_file" \
  cargo run -q -p mirror-backend --bin maintenance -- \
    --run-backup "$dump_path" \
    --repository-hint drill-repo
)"
snapshot="$(RESTIC_REPOSITORY="$restic_repo" RESTIC_PASSWORD_FILE="$password_file" restic snapshots --json | jq -r '.[0].short_id')"

if [[ -z "$snapshot" || "$snapshot" == "null" ]]; then
  echo "backup produced no restic snapshot" >&2
  exit 1
fi
if [[ "$backup_output" == *"mirror-drill-password"* || "$backup_output" == *"$base_url"* ]]; then
  echo "backup output leaked secret material" >&2
  exit 1
fi
retention_output="$(
  RESTIC_REPOSITORY="$restic_repo" \
  RESTIC_PASSWORD_FILE="$password_file" \
  cargo run -q -p mirror-backend --bin maintenance -- --run-retention
)"
if [[ "$retention_output" != *$'retention_run\tsucceeded'* ]]; then
  echo "retention command did not report success" >&2
  exit 1
fi
if [[ "$retention_output" == *"mirror-drill-password"* || "$retention_output" == *"$base_url"* ]]; then
  echo "retention output leaked secret material" >&2
  exit 1
fi

MIRROR_DATABASE_URL="${base_url}/${restore_db}" \
MIRROR_STORAGE_ROOT="$work_dir/unused-storage" \
RESTIC_REPOSITORY="$restic_repo" \
RESTIC_PASSWORD_FILE="$password_file" \
cargo run -q -p mirror-backend --bin maintenance -- \
  --run-restore "$snapshot" "$restore_target" "$restore_target/$dump_path" >/dev/null

restored_storage="$restore_target$source_storage"
backup_run_id="$(scalar_sql "$restore_db" "SELECT id FROM backup_runs ORDER BY created_at DESC LIMIT 1")"
MIRROR_DATABASE_URL="${base_url}/${restore_db}" \
MIRROR_STORAGE_ROOT="$restored_storage" \
cargo run -q -p mirror-backend --bin maintenance -- --restore-check "$backup_run_id" >/dev/null

asset_count="$(scalar_sql "$restore_db" "SELECT count(*) FROM assets")"
original_count="$(scalar_sql "$restore_db" "SELECT count(*) FROM originals")"
derivative_count="$(scalar_sql "$restore_db" "SELECT count(*) FROM derivatives")"
model_file_count="$(scalar_sql "$restore_db" "SELECT count(*) FROM model_pack_files")"
restore_status="$(scalar_sql "$restore_db" "SELECT status FROM backup_runs WHERE id = '$backup_run_id'")"

[[ "$asset_count" == "2" ]] || { echo "asset count mismatch: $asset_count" >&2; exit 1; }
[[ "$original_count" == "2" ]] || { echo "original count mismatch: $original_count" >&2; exit 1; }
[[ "$derivative_count" == "1" ]] || { echo "derivative count mismatch: $derivative_count" >&2; exit 1; }
[[ "$model_file_count" == "1" ]] || { echo "model file count mismatch: $model_file_count" >&2; exit 1; }
[[ "$restore_status" == "restore_check_succeeded" ]] || { echo "restore status mismatch: $restore_status" >&2; exit 1; }

for key in "$original_a" "$original_b" "$derivative_a" "$model_file"; do
  [[ -f "$restored_storage/$key" ]] || { echo "missing restored durable file: $key" >&2; exit 1; }
done
for key in "$staging_junk" "$tmp_junk" "$logs_junk" "$scratch_junk"; do
  [[ ! -e "$restored_storage/$key" ]] || { echo "restored excluded file: $key" >&2; exit 1; }
done

printf 'PASS backup_restore_drill snapshot=%s assets=%s originals=%s derivatives=%s model_files=%s status=%s\n' \
  "$snapshot" "$asset_count" "$original_count" "$derivative_count" "$model_file_count" "$restore_status"
