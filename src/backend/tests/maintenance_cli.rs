use std::process::Command;

mod support;
use support::TestResult;

#[test]
fn maintenance_apply_requires_explicit_orphan_selection() -> TestResult {
    let output = Command::new(env!("CARGO_BIN_EXE_maintenance"))
        .arg("--apply")
        .output()?;

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("--apply requires at least one --delete-orphan KEY")
    );
    Ok(())
}

#[test]
fn maintenance_rejects_noncanonical_original_key_before_database_access() -> TestResult {
    let output = Command::new(env!("CARGO_BIN_EXE_maintenance"))
        .args([
            "--delete-orphan",
            "originals/blake3/arbitrary/object",
            "--apply",
        ])
        .output()?;

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("not a content-addressed original key")
    );
    Ok(())
}
