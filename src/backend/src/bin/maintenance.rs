//! Mirror maintenance process.

use std::{collections::BTreeSet, env, io};

use mirror_backend::{
    config::Config,
    db,
    integrity::{remediate_original_orphans, scan_original_storage},
    runtime::io_other,
    storage::{ObjectStorage, StorageKey},
};

const USAGE: &str = "\
Usage: maintenance [--delete-orphan KEY ...] [--apply]

Scans originals by default without modifying storage.
--delete-orphan KEY  Select a currently reported orphan for remediation.
--apply              Delete selected orphans after fresh database checks.
";

#[derive(Debug)]
struct Options {
    apply: bool,
    selected_keys: Vec<StorageKey>,
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
    let database_url = config.database_url.as_deref().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "MIRROR_DATABASE_URL is required for maintenance",
        )
    })?;
    let pool = db::connect(database_url).await.map_err(io_other)?;
    std::fs::create_dir_all(&config.storage_root)?;
    let storage = ObjectStorage::local(&config.storage_root).map_err(io_other)?;

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
    }))
}

#[derive(Debug)]
enum CliError {
    ApplyWithoutSelection,
    InvalidOriginalObjectKey(String),
    InvalidStorageKey(String),
    MissingOrphanKey,
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
            Self::UnknownArgument(argument) => write!(formatter, "unknown argument: {argument}"),
        }
    }
}

impl std::error::Error for CliError {}
