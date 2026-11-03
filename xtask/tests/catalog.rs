use std::{path::Path, process::Command, time::Duration};

#[test]
fn paired_language_catalog_update_rolls_back_and_recovers_without_network() {
    let root = tempfile::tempdir().unwrap();
    xtask::process::checked(
        Command::new("pwsh")
            .args(["-NoProfile", "-NonInteractive", "-File"])
            .arg(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../scripts/tests/catalog-update-contract.ps1"),
            )
            .arg("-OutputDirectory")
            .arg(root.path().join("catalog")),
        Duration::from_secs(30),
        "言語表と許諾文の更新",
        &root.path().join("logs"),
    )
    .unwrap();
}
