//! 同じ本番入口で、確認の失効・外部値の保持・保存失敗からの再開を検証する。
use super::*;
use crate::test_process::CommandOutput;
use std::os::windows::fs::OpenOptionsExt;

#[test]
fn confirmed_recovery_resumes_after_save_failure() {
    if std::env::var_os("WFI_CONFIRMED_RECOVERY_PROBE").is_none() {
        let directory = tempfile::tempdir().unwrap();
        let executable = std::env::current_exe().unwrap();
        let copied = directory.path().join("recovery-tests.exe");
        fs::copy(&executable, &copied).unwrap();
        fs::copy(
            executable
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("file_icons_shell.dll"),
            directory.path().join("file_icons_shell.dll"),
        )
        .unwrap();
        let output = Command::new(copied)
            .args([
                "--exact",
                "tests::conflicts::confirmed_recovery_resumes_after_save_failure",
                "--nocapture",
            ])
            .env("WFI_CONFIRMED_RECOVERY_PROBE", "1")
            .env("LOCALAPPDATA", directory.path().join("profile"))
            .test_output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let (_redirect, classes) = Redirect::new();
    let progress = Arc::new(Progress::default());
    let _console = initialize(progress.clone()).unwrap();
    let class = classes.create_subkey("Wfi.Recovery").unwrap().0;
    classes
        .create_subkey(".wfi-recovery")
        .unwrap()
        .0
        .set_value("", &"Wfi.Recovery")
        .unwrap();
    let icon = class.create_subkey("DefaultIcon").unwrap().0;
    icon.set_value("", &"before.ico,4").unwrap();
    class
        .create_subkey(r"shell\open\command")
        .unwrap()
        .0
        .set_value("", &"notepad.exe %1")
        .unwrap();
    for (ext, id) in [("lnk", "lnkfile"), ("exe", "exefile")] {
        classes
            .create_subkey(format!(".{ext}"))
            .unwrap()
            .0
            .set_value("", &id)
            .unwrap();
        classes
            .create_subkey(format!("{id}\\DefaultIcon"))
            .unwrap()
            .0
            .set_value("", &"protected.ico,9")
            .unwrap();
    }
    let source = tempfile::tempdir().unwrap();
    fs::write(source.path().join("package.json"), r#"{"name":"confirmed-recovery","version":"1","contributes":{"iconThemes":[{"id":"one","label":"Recovery pack","path":"theme.json"}]}}"#).unwrap();
    fs::write(source.path().join("theme.json"), r#"{"iconDefinitions":{"x":{"iconPath":"icon.svg"}},"fileExtensions":{"wfi-recovery":"x","lnk":"x","exe":"x"}}"#).unwrap();
    fs::write(source.path().join("icon.svg"), r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="16" height="16" fill="#ff0000"/></svg>"##).unwrap();
    let manager = Manager::acquire().unwrap();
    let id = manager.import(source.path(), "one", &progress).unwrap();
    // 外部登録の上書きと復旧を試すため、保護は明示的に無効にする。
    dispatch(&manager, &["protection", "set", "off"]).unwrap();
    dispatch(&manager, &["apply", &id]).unwrap();
    // 通常設定だけが解除済みでも、残存する独立記録のパックを再適用できる。
    let mut inactive = manager.settings().unwrap();
    inactive.active = None;
    file_icons_windows::save(&inactive).unwrap();
    assert!(manager.settings().unwrap().active.is_none());
    // 旧版による保護対象の登録が残っていても、再適用では復元だけを行う。
    let legacy_key = classes
        .open_subkey_with_flags(r"lnkfile\DefaultIcon", KEY_ALL_ACCESS)
        .unwrap();
    let before_legacy = legacy_key.get_raw_value("").unwrap();
    legacy_key.set_value("", &"%1").unwrap();
    let after_legacy = legacy_key.get_raw_value("").unwrap();
    let legacy_change = file_icons_core::registry::Change {
        key: r"Software\Classes\lnkfile\DefaultIcon".into(),
        name: String::new(),
        before: Some(file_icons_core::registry::Value {
            kind: before_legacy.vtype as u32,
            bytes: before_legacy.bytes.into_owned(),
        }),
        after: file_icons_core::registry::Value {
            kind: after_legacy.vtype as u32,
            bytes: after_legacy.bytes.into_owned(),
        },
    };
    let legacy_path = file_icons_windows::home().unwrap().join("recovery.json");
    let mut record: Value = serde_json::from_slice(&fs::read(&legacy_path).unwrap()).unwrap();
    record["journal"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::to_value(legacy_change).unwrap());
    fs::write(legacy_path, serde_json::to_vec(&record).unwrap()).unwrap();
    icon.set_value("", &"external-one.ico,7").unwrap();
    let root = file_icons_windows::home().unwrap();
    let listed = dispatch(&manager, &["conflicts", "list"]).unwrap();
    assert_eq!(listed["conflicts"], 1);
    assert!(
        listed["entries"].as_array().unwrap().iter().any(
            |e| e["state"] == "external" && e["key"].as_str().unwrap().ends_with("DefaultIcon")
        )
    );
    let snapshot = fs::read(root.join("settings.json")).unwrap();
    let preview = dispatch(
        &manager,
        &["recover", "--strategy", "preserve-external", "--dry-run"],
    )
    .unwrap();
    let confirmation = preview["plan"]["confirmation"].as_str().unwrap().to_owned();
    assert_eq!(fs::read(root.join("settings.json")).unwrap(), snapshot);
    assert!(!root.join("conflict-backups").exists());
    let wrong = dispatch(
        &manager,
        &[
            "recover",
            "--strategy",
            "reapply-theme",
            "--confirm",
            &confirmation,
        ],
    )
    .unwrap_err();
    assert_eq!(classify(&wrong), ("registration_conflict", 6));
    icon.set_value("", &"external-two.ico,8").unwrap();
    let stale = dispatch(
        &manager,
        &[
            "recover",
            "--strategy",
            "preserve-external",
            "--confirm",
            &confirmation,
        ],
    )
    .unwrap_err();
    assert_eq!(classify(&stale), ("registration_conflict", 6));
    assert_eq!(
        icon.get_value::<String, _>("").unwrap(),
        "external-two.ico,8"
    );
    let preview = dispatch(
        &manager,
        &["recover", "--strategy", "preserve-external", "--dry-run"],
    )
    .unwrap();
    let confirmation = preview["plan"]["confirmation"].as_str().unwrap().to_owned();
    fs::write(root.join("conflict-backups"), b"block backup directory").unwrap();
    assert!(
        dispatch(
            &manager,
            &[
                "recover",
                "--strategy",
                "preserve-external",
                "--confirm",
                &confirmation
            ]
        )
        .is_err()
    );
    assert_eq!(fs::read(root.join("settings.json")).unwrap(), snapshot);
    assert_eq!(
        icon.get_value::<String, _>("").unwrap(),
        "external-two.ico,8"
    );
    fs::remove_file(root.join("conflict-backups")).unwrap();

    let preview = dispatch(
        &manager,
        &["recover", "--strategy", "reapply-theme", "--dry-run"],
    )
    .unwrap();
    let confirmation = preview["plan"]["confirmation"].as_str().unwrap().to_owned();
    // 計画後の素材消失で再適用を止め、解除経路は利用可能にする。
    let asset = manager
        .settings()
        .unwrap()
        .packs
        .iter()
        .find(|p| p.id == id)
        .unwrap()
        .theme
        .rules()
        .extensions["wfi-recovery"]
        .clone();
    let asset_bytes = fs::read(&asset).unwrap();
    fs::remove_file(&asset).unwrap();
    assert!(
        dispatch(
            &manager,
            &["recover", "--strategy", "reapply-theme", "--dry-run"]
        )
        .is_err()
    );
    assert!(
        dispatch(
            &manager,
            &[
                "recover",
                "--strategy",
                "reapply-theme",
                "--confirm",
                &confirmation
            ]
        )
        .is_err()
    );
    assert_eq!(fs::read(root.join("settings.json")).unwrap(), snapshot);
    assert_eq!(
        icon.get_value::<String, _>("").unwrap(),
        "external-two.ico,8"
    );
    assert!(!root.join("conflict-operation.json").exists());
    assert!(
        dispatch(
            &manager,
            &["recover", "--strategy", "preserve-external", "--dry-run"]
        )
        .is_ok()
    );
    fs::write(&asset, asset_bytes).unwrap();
    // 登録は成功しても設定の確定だけを共有違反で失敗させる。
    let sharing_lock = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(root.join("settings.json"))
        .unwrap();
    assert!(
        dispatch(
            &manager,
            &[
                "recover",
                "--strategy",
                "reapply-theme",
                "--confirm",
                &confirmation
            ]
        )
        .is_err()
    );
    drop(sharing_lock);
    assert!(root.join("conflict-operation.json").is_file());
    assert_eq!(icon.get_value::<String, _>("").unwrap(), "%1");
    assert!(manager.settings().is_err());
    drop(manager);
    let manager = Manager::acquire().unwrap();
    let preview = dispatch(
        &manager,
        &["recover", "--strategy", "reapply-theme", "--dry-run"],
    )
    .unwrap();
    assert_eq!(preview["plan"]["resuming"], true);
    let confirmation = preview["plan"]["confirmation"].as_str().unwrap().to_owned();
    dispatch(
        &manager,
        &[
            "recover",
            "--strategy",
            "reapply-theme",
            "--confirm",
            &confirmation,
        ],
    )
    .unwrap();
    assert!(!root.join("conflict-operation.json").exists());
    assert_eq!(
        manager.settings().unwrap().active.as_deref(),
        Some(id.as_str())
    );
    for protected in ["lnkfile", "exefile"] {
        assert_eq!(
            classes
                .open_subkey(format!("{protected}\\DefaultIcon"))
                .unwrap()
                .get_value::<String, _>("")
                .unwrap(),
            "protected.ico,9"
        );
    }
    dispatch(&manager, &["restore"]).unwrap();
    assert_eq!(
        icon.get_value::<String, _>("").unwrap(),
        "external-two.ico,8"
    );

    dispatch(&manager, &["apply", &id]).unwrap();
    icon.set_value("", &"external-three.ico,2").unwrap();
    // 別アプリへの関連付け変更は、記録済みアイコンの解除を妨げない。
    let application = r"Applications\Illustrator.exe";
    let association = classes
        .open_subkey_with_flags(".wfi-recovery", KEY_ALL_ACCESS)
        .unwrap();
    association.set_value("", &application).unwrap();
    let application_key = classes.create_subkey(application).unwrap().0;
    application_key
        .create_subkey(r"shell\open\command")
        .unwrap()
        .0
        .set_value("", &"illustrator.exe %1")
        .unwrap();
    let application_icon = application_key.create_subkey("DefaultIcon").unwrap().0;
    application_icon
        .set_value("", &"illustrator.ico,3")
        .unwrap();
    assert!(
        file_icons_windows::association("wfi-recovery").is_err(),
        "書き込み対象の検査は維持する"
    );
    let listed = dispatch(&manager, &["conflicts", "list"]).unwrap();
    assert_eq!(listed["conflicts"], 1);
    fs::remove_file(&asset).unwrap();
    let preview = dispatch(
        &manager,
        &["recover", "--strategy", "preserve-external", "--dry-run"],
    )
    .unwrap();
    let stale_id = preview["plan"]["confirmation"].as_str().unwrap().to_owned();
    manager.language("en".into()).unwrap();
    assert!(
        dispatch(
            &manager,
            &[
                "recover",
                "--strategy",
                "preserve-external",
                "--confirm",
                &stale_id
            ]
        )
        .is_err()
    );
    let preview = dispatch(
        &manager,
        &["recover", "--strategy", "preserve-external", "--dry-run"],
    )
    .unwrap();
    let confirmation = preview["plan"]["confirmation"].as_str().unwrap().to_owned();
    let backup = std::path::PathBuf::from(preview["plan"]["backup"].as_str().unwrap());
    dispatch(
        &manager,
        &[
            "recover",
            "--strategy",
            "preserve-external",
            "--confirm",
            &confirmation,
        ],
    )
    .unwrap();
    assert!(backup.join("settings.json").is_file() && backup.join("plan.json").is_file());
    assert_eq!(
        icon.get_value::<String, _>("").unwrap(),
        "external-three.ico,2"
    );
    let settings = manager.settings().unwrap();
    assert!(settings.active.is_none() && settings.journal.is_empty());
    assert_eq!(settings.packs.len(), 1);
    assert_eq!(
        class
            .open_subkey(r"shell\open\command")
            .unwrap()
            .get_value::<String, _>("")
            .unwrap(),
        "notepad.exe %1"
    );
    assert!(!root.join("recovery.json").exists());
    assert_eq!(association.get_value::<String, _>("").unwrap(), application);
    assert_eq!(
        application_icon.get_value::<String, _>("").unwrap(),
        "illustrator.ico,3"
    );
    // 不正な記録による起動設定の書き換えと、未知形式の読み飛ばしは許可しない。
    let mut invalid = settings.clone();
    invalid.journal.push(file_icons_core::registry::Change {
        key: r"Software\Classes\Wfi.Recovery\shell\open\command".into(),
        name: String::new(),
        before: None,
        after: file_icons_core::registry::Value {
            kind: 1,
            bytes: vec![],
        },
    });
    file_icons_windows::save(&invalid).unwrap();
    assert!(dispatch(&manager, &["conflicts", "list"]).is_err());
    fs::write(root.join("settings.json"), br#"{"format_version":999}"#).unwrap();
    assert!(dispatch(&manager, &["conflicts", "list"]).is_err());
}
