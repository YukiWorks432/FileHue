use std::{fs, process::Command};

#[test]
fn thumbnail_status_and_dry_run_keep_an_isolated_profile_unchanged() {
    let profile = tempfile::tempdir().unwrap();
    let run = |arguments: &[&str]| {
        let output = Command::new(env!("CARGO_BIN_EXE_file-icons"))
            .env("LOCALAPPDATA", profile.path())
            .args(["--json", "--lang", "en"])
            .args(arguments)
            .output()
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        (output.status.success(), json)
    };
    let (ok, status) = run(&["thumbnails", "show"]);
    assert!(ok);
    assert_eq!(status["data"]["saved_selection"], serde_json::json!([]));
    assert_eq!(status["data"]["recovery_pending"], false);
    let (ok, plan) = run(&["thumbnails", "restore", "--dry-run"]);
    assert!(ok);
    assert_eq!(plan["data"]["dry_run"], true);
    assert!(plan["data"]["recovery"].is_null());
    for value in ["exe", "ai,,pdf", "..ai", "ai,.ai"] {
        let (ok, _) = run(&["thumbnails", "set", value, "--dry-run"]);
        assert!(!ok, "invalid selection accepted: {value}");
    }
    let files = fs::read_dir(profile.path())
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(!files.is_empty());
    let manager = profile.path().join("WindowsFileIconsDev");
    assert!(!manager.join("thumbnail-settings.json").exists());
    assert!(!manager.join("thumbnail-recovery.json").exists());
    let settings = manager.join("thumbnail-settings.json");
    let orphaned = br#"{"format_version":1,"selected":[".svg"]}"#;
    fs::write(&settings, orphaned).unwrap();
    let (ok, status) = run(&["thumbnails", "show"]);
    assert!(ok);
    assert_eq!(status["data"]["recovery_pending"], true);
    for arguments in [
        &["thumbnails", "set", "svg"][..],
        &["thumbnails", "restore"][..],
    ] {
        let (ok, _) = run(arguments);
        assert!(!ok);
        assert_eq!(fs::read(&settings).unwrap(), orphaned);
        assert!(!manager.join("thumbnail-recovery.json").exists());
    }
}
