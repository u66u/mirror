//! Mirror maintenance process.

use std::{collections::BTreeSet, env, io};

use mirror_backend::{
    backups::{
        CreateBackupRunInput, backup_manifest_summary, create_backup_run, durable_storage_manifest,
        mark_backup_failed, mark_backup_succeeded, mark_restore_check, postgres_dump_plan,
        postgres_restore_plan, restic_backup_plan, restic_restore_plan, restic_retention_plan,
        run_postgres_dump_plan, run_postgres_restore_plan, run_restic_backup_plan,
        run_restic_restore_plan, run_restic_retention_plan,
    },
    config::Config,
    db,
    integrity::{remediate_original_orphans, scan_original_storage},
    runtime::io_other,
    storage::{ObjectStorage, StorageKey},
};
use uuid::Uuid;

const USAGE: &str = "\
Usage: maintenance [--delete-orphan KEY ...] [--apply] [--backup-plan PG_DUMP_PATH] [--restore-plan SNAPSHOT_ID RESTORE_TARGET PG_DUMP_PATH] [--retention-plan] [--run-backup PG_DUMP_PATH] [--run-restore SNAPSHOT_ID RESTORE_TARGET PG_DUMP_PATH] [--run-retention] [--repository-hint HINT] [--restore-check BACKUP_RUN_ID]

Scans originals by default without modifying storage.
--delete-orphan KEY  Select a currently reported orphan for remediation.
--apply              Delete selected orphans after fresh database checks.
--backup-plan PATH   Print pg_dump/restic command plans for durable backup inputs.
--restore-plan ID TARGET PATH
                     Print restic/pg_restore command plans for a restore drill.
--retention-plan     Print restic forget/prune retention inputs.
--run-backup PATH    Run pg_dump then restic backup and persist backup_runs metadata.
--run-restore ID TARGET PATH
                     Run restic restore then pg_restore into MIRROR_DATABASE_URL.
--run-retention      Run restic forget --prune with the v1 retention policy.
--repository-hint H  Redacted repository label stored with --run-backup.
--restore-check ID   Scan restored DB/storage and update backup_runs status.
";

#[derive(Debug)]
struct Options {
    apply: bool,
    selected_keys: Vec<StorageKey>,
    backup_plan_dump_path: Option<std::path::PathBuf>,
    restore_plan: Option<RestorePlanOptions>,
    retention_plan: bool,
    run_backup_dump_path: Option<std::path::PathBuf>,
    run_restore: Option<RestorePlanOptions>,
    run_retention: bool,
    repository_hint: Option<String>,
    restore_check_backup_run_id: Option<Uuid>,
}

#[actix_web::main]
async fn main() -> io::Result<()> {
    let Some(options) = parse_args()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))?
    else {
        print!("{USAGE}");
        return Ok(());
    };
    let config = Config::from_env();
    if let Some(dump_path) = options.backup_plan_dump_path.as_ref() {
        let dump = postgres_dump_plan(dump_path).map_err(io_other)?;
        let plan = restic_backup_plan(&config.storage_root, dump_path).map_err(io_other)?;
        println!("dump_program\t{}", dump.program);
        println!("dump_args\t{}", dump.args.join("\t"));
        println!("dump_required_env\t{}", dump.required_env.join("\t"));
        println!("backup_program\t{}", plan.program);
        println!("backup_args\t{}", plan.args.join("\t"));
        println!("backup_required_env\t{}", plan.required_env.join("\t"));
        return Ok(());
    }
    if let Some(restore) = options.restore_plan.as_ref() {
        let restic =
            restic_restore_plan(&restore.snapshot_id, &restore.restore_target).map_err(io_other)?;
        let postgres = postgres_restore_plan(&restore.dump_path).map_err(io_other)?;
        println!("restore_program\t{}", restic.program);
        println!("restore_args\t{}", restic.args.join("\t"));
        println!("restore_required_env\t{}", restic.required_env.join("\t"));
        println!("db_restore_program\t{}", postgres.program);
        println!("db_restore_args\t{}", postgres.args.join("\t"));
        println!(
            "db_restore_required_env\t{}",
            postgres.required_env.join("\t")
        );
        return Ok(());
    }
    if options.retention_plan {
        let plan = restic_retention_plan();
        println!("retention_program\t{}", plan.program);
        println!("retention_args\t{}", plan.args.join("\t"));
        println!("retention_required_env\t{}", plan.required_env.join("\t"));
        return Ok(());
    }
    if options.run_retention {
        run_retention()?;
        return Ok(());
    }

    let database_url = config.database_url.as_deref().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "MIRROR_DATABASE_URL is required for maintenance",
        )
    })?;
    let pool = db::connect(database_url).await.map_err(io_other)?;
    std::fs::create_dir_all(&config.storage_root)?;
    ensure_backup_input_dirs(&config.storage_root)?;
    let storage = ObjectStorage::local(&config.storage_root).map_err(io_other)?;

    if let Some(dump_path) = options.run_backup_dump_path.as_ref() {
        run_backup(
            &pool,
            &storage,
            &config,
            database_url,
            dump_path,
            options.repository_hint,
        )
        .await?;
        return Ok(());
    }
    if let Some(backup_run_id) = options.restore_check_backup_run_id {
        run_restore_check(&pool, &storage, backup_run_id).await?;
        return Ok(());
    }
    if let Some(restore) = options.run_restore.as_ref() {
        run_restore(database_url, restore).await?;
        return Ok(());
    }

    let report = scan_original_storage(&pool, &storage)
        .await
        .map_err(io_other)?;
    for missing in &report.missing_objects {
        println!("missing\t{}\t{}", missing.blake3_hash, missing.storage_key);
    }
    for orphan in &report.orphan_objects {
        println!("orphan\t{}", orphan.as_str());
    }
    println!(
        "summary\tmissing={}\torphan={}",
        report.missing_objects.len(),
        report.orphan_objects.len()
    );

    let current_orphans = report
        .orphan_objects
        .iter()
        .map(StorageKey::as_str)
        .collect::<BTreeSet<_>>();
    let selected_orphans = options
        .selected_keys
        .into_iter()
        .filter(|key| {
            if current_orphans.contains(key.as_str()) {
                true
            } else {
                println!("not_current_orphan\t{}", key.as_str());
                false
            }
        })
        .collect::<Vec<_>>();

    if !options.apply {
        for key in selected_orphans {
            println!("would_delete\t{}", key.as_str());
        }
        return Ok(());
    }

    let remediation = remediate_original_orphans(&pool, &storage, &selected_orphans)
        .await
        .map_err(io_other)?;
    for key in remediation.deleted_objects {
        println!("deleted\t{}", key.as_str());
    }
    for key in remediation.retained_db_backed_objects {
        println!("retained_db_backed\t{}", key.as_str());
    }
    for key in remediation.already_missing_objects {
        println!("already_missing\t{}", key.as_str());
    }

    Ok(())
}

fn parse_args() -> Result<Option<Options>, CliError> {
    let mut apply = false;
    let mut selected_keys = Vec::new();
    let mut backup_plan_dump_path = None;
    let mut restore_plan = None;
    let mut retention_plan = false;
    let mut run_backup_dump_path = None;
    let mut run_restore = None;
    let mut run_retention = false;
    let mut repository_hint = None;
    let mut restore_check_backup_run_id = None;
    let mut args = env::args().skip(1);

    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--apply" => apply = true,
            "--delete-orphan" => {
                let raw_key = args.next().ok_or(CliError::MissingOrphanKey)?;
                let key = StorageKey::new(raw_key.clone())
                    .map_err(|_| CliError::InvalidStorageKey(raw_key.clone()))?;
                if !key.is_original_blake3_object() {
                    return Err(CliError::InvalidOriginalObjectKey(raw_key));
                }
                selected_keys.push(key);
            }
            "--backup-plan" => {
                let raw_path = args.next().ok_or(CliError::MissingBackupDumpPath)?;
                backup_plan_dump_path = Some(std::path::PathBuf::from(raw_path));
            }
            "--restore-plan" => {
                let snapshot_id = args.next().ok_or(CliError::MissingRestorePlanArgument)?;
                let restore_target = args.next().ok_or(CliError::MissingRestorePlanArgument)?;
                let dump_path = args.next().ok_or(CliError::MissingRestorePlanArgument)?;
                restore_plan = Some(RestorePlanOptions {
                    snapshot_id,
                    restore_target: std::path::PathBuf::from(restore_target),
                    dump_path: std::path::PathBuf::from(dump_path),
                });
            }
            "--retention-plan" => retention_plan = true,
            "--run-backup" => {
                let raw_path = args.next().ok_or(CliError::MissingBackupDumpPath)?;
                run_backup_dump_path = Some(std::path::PathBuf::from(raw_path));
            }
            "--run-restore" => {
                let snapshot_id = args.next().ok_or(CliError::MissingRestorePlanArgument)?;
                let restore_target = args.next().ok_or(CliError::MissingRestorePlanArgument)?;
                let dump_path = args.next().ok_or(CliError::MissingRestorePlanArgument)?;
                run_restore = Some(RestorePlanOptions {
                    snapshot_id,
                    restore_target: std::path::PathBuf::from(restore_target),
                    dump_path: std::path::PathBuf::from(dump_path),
                });
            }
            "--run-retention" => run_retention = true,
            "--repository-hint" => {
                repository_hint = Some(args.next().ok_or(CliError::MissingRepositoryHint)?);
            }
            "--restore-check" => {
                let raw_id = args.next().ok_or(CliError::MissingRestoreCheckId)?;
                restore_check_backup_run_id = Some(
                    Uuid::parse_str(&raw_id)
                        .map_err(|_| CliError::InvalidRestoreCheckId(raw_id))?,
                );
            }
            "--help" | "-h" => return Ok(None),
            _ => return Err(CliError::UnknownArgument(argument)),
        }
    }

    if apply && selected_keys.is_empty() {
        return Err(CliError::ApplyWithoutSelection);
    }

    Ok(Some(Options {
        apply,
        selected_keys,
        backup_plan_dump_path,
        restore_plan,
        retention_plan,
        run_backup_dump_path,
        run_restore,
        run_retention,
        repository_hint,
        restore_check_backup_run_id,
    }))
}

#[derive(Debug)]
struct RestorePlanOptions {
    snapshot_id: String,
    restore_target: std::path::PathBuf,
    dump_path: std::path::PathBuf,
}

#[derive(Debug)]
enum CliError {
    ApplyWithoutSelection,
    InvalidOriginalObjectKey(String),
    InvalidStorageKey(String),
    MissingOrphanKey,
    MissingBackupDumpPath,
    MissingRestorePlanArgument,
    MissingRepositoryHint,
    MissingRestoreCheckId,
    InvalidRestoreCheckId(String),
    UnknownArgument(String),
}

impl std::fmt::Display for CliError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ApplyWithoutSelection => {
                formatter.write_str("--apply requires at least one --delete-orphan KEY")
            }
            Self::InvalidOriginalObjectKey(key) => {
                write!(formatter, "not a content-addressed original key: {key}")
            }
            Self::InvalidStorageKey(key) => write!(formatter, "invalid storage key: {key}"),
            Self::MissingOrphanKey => formatter.write_str("--delete-orphan requires KEY"),
            Self::MissingBackupDumpPath => formatter.write_str("--backup-plan requires PATH"),
            Self::MissingRestorePlanArgument => formatter
                .write_str("--restore-plan requires SNAPSHOT_ID RESTORE_TARGET PG_DUMP_PATH"),
            Self::MissingRepositoryHint => formatter.write_str("--repository-hint requires HINT"),
            Self::MissingRestoreCheckId => formatter.write_str("--restore-check requires ID"),
            Self::InvalidRestoreCheckId(id) => write!(formatter, "invalid backup run ID: {id}"),
            Self::UnknownArgument(argument) => write!(formatter, "unknown argument: {argument}"),
        }
    }
}

impl std::error::Error for CliError {}

async fn run_backup(
    pool: &sqlx::PgPool,
    storage: &ObjectStorage,
    config: &Config,
    database_url: &str,
    dump_path: &std::path::Path,
    repository_hint: Option<String>,
) -> io::Result<()> {
    let dump_plan = postgres_dump_plan(dump_path).map_err(io_other)?;
    let backup_plan = restic_backup_plan(&config.storage_root, dump_path).map_err(io_other)?;
    let manifest = durable_storage_manifest(storage).await.map_err(io_other)?;
    let run = create_backup_run(
        pool,
        CreateBackupRunInput {
            repository_hint,
            manifest: backup_manifest_summary(manifest.objects.len()),
        },
    )
    .await
    .map_err(io_other)?;

    if let Err(error) = run_postgres_dump_plan(&dump_plan, database_url) {
        let _ = mark_backup_failed(pool, run.backup_run_id, &error.to_string()).await;
        return Err(io_other(error));
    }

    match run_restic_backup_plan(&backup_plan) {
        Ok(output) => {
            let updated = mark_backup_succeeded(pool, run.backup_run_id, &output.snapshot_id)
                .await
                .map_err(io_other)?;
            println!(
                "backup_run\t{}\t{}\t{}",
                updated.backup_run_id,
                updated.status,
                updated.snapshot_id.unwrap_or_default()
            );
            Ok(())
        }
        Err(error) => {
            let _ = mark_backup_failed(pool, run.backup_run_id, &error.to_string()).await;
            Err(io_other(error))
        }
    }
}

async fn run_restore(database_url: &str, restore: &RestorePlanOptions) -> io::Result<()> {
    let restic =
        restic_restore_plan(&restore.snapshot_id, &restore.restore_target).map_err(io_other)?;
    let postgres = postgres_restore_plan(&restore.dump_path).map_err(io_other)?;

    run_restic_restore_plan(&restic).map_err(io_other)?;
    run_postgres_restore_plan(&postgres, database_url).map_err(io_other)?;
    println!(
        "restore_run\t{}\t{}",
        restore.snapshot_id,
        restore.restore_target.display()
    );
    Ok(())
}

fn ensure_backup_input_dirs(storage_root: &std::path::Path) -> io::Result<()> {
    std::fs::create_dir_all(storage_root.join("originals").join("blake3"))?;
    std::fs::create_dir_all(storage_root.join("derivatives"))?;
    std::fs::create_dir_all(storage_root.join("model-packs"))?;
    Ok(())
}

fn run_retention() -> io::Result<()> {
    let plan = restic_retention_plan();
    run_restic_retention_plan(&plan).map_err(io_other)?;
    println!("retention_run\tsucceeded");
    Ok(())
}

async fn run_restore_check(
    pool: &sqlx::PgPool,
    storage: &ObjectStorage,
    backup_run_id: Uuid,
) -> io::Result<()> {
    let report = scan_original_storage(pool, storage)
        .await
        .map_err(io_other)?;
    let succeeded = report.orphan_objects.is_empty() && report.missing_objects.is_empty();
    let error_message = if succeeded {
        None
    } else {
        Some(format!(
            "restore check found missing={} orphan={}",
            report.missing_objects.len(),
            report.orphan_objects.len()
        ))
    };
    let updated = mark_restore_check(pool, backup_run_id, succeeded, error_message.as_deref())
        .await
        .map_err(io_other)?;
    println!(
        "restore_check\t{}\t{}",
        updated.backup_run_id, updated.status
    );
    if succeeded {
        Ok(())
    } else {
        Err(io::Error::other(
            error_message.unwrap_or_else(|| "restore check failed".to_owned()),
        ))
    }
}
