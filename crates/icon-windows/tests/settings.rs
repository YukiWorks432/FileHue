use file_icons_windows::{Settings, load_from, save_to};

#[test]
fn protection_defaults_and_persists_opt_out() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("settings.json");
    let mut settings = load_from(&path).unwrap();
    assert!(settings.preserve_existing_icons);
    save_to(&settings, &path).unwrap();
    settings.preserve_existing_icons = false;
    save_to(&settings, &path).unwrap();
    assert!(!load_from(&path).unwrap().preserve_existing_icons);
    assert!(
        !file_icons_windows::SettingsCache::default()
            .get(&path)
            .unwrap()
            .preserve_existing_icons
    );
}

#[test]
fn persists_recovery_data_and_rejects_corruption_without_overwriting_it() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("settings.json");
    let mut settings = Settings {
        language: "en".into(),
        active: Some("local.pack/theme".into()),
        ..Default::default()
    };
    settings.journal.push(file_icons_core::registry::Change {
        key: "Software\\Classes\\Test\\DefaultIcon".into(),
        name: "".into(),
        before: Some(file_icons_core::registry::Value {
            kind: 2,
            bytes: vec![1, 0, 0, 0],
        }),
        after: file_icons_core::registry::Value {
            kind: 1,
            bytes: vec![2, 0, 0, 0],
        },
    });
    save_to(&settings, &path).unwrap();
    let loaded = load_from(&path).unwrap();
    assert_eq!(loaded.active, settings.active);
    assert_eq!(loaded.journal, settings.journal);
    settings.language = "ja".into();
    save_to(&settings, &path).unwrap();
    assert_eq!(load_from(&path).unwrap().language, "ja");
    std::fs::write(&path, "broken").unwrap();
    assert!(load_from(&path).is_err());
    assert_eq!(std::fs::read_to_string(path).unwrap(), "broken");
}

#[test]
fn ordinary_save_preserves_both_files_when_either_document_is_invalid() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("settings.json");
    let library = path.with_extension("library.json");
    let settings = Settings::default();
    save_to(&settings, &path).unwrap();
    let runtime_bytes = std::fs::read(&path).unwrap();
    let library_bytes = std::fs::read(&library).unwrap();
    for target in [&path, &library] {
        for damaged in [
            b"broken".as_slice(),
            br#"{"format_version":3,"packs":false}"#,
            br#"{"format_version":999}"#,
            br#"{"format_version":1,"language":42,"packs":[],"pending_pack_removals":[]}"#,
        ] {
            std::fs::write(&path, &runtime_bytes).unwrap();
            std::fs::write(&library, &library_bytes).unwrap();
            std::fs::write(target, damaged).unwrap();
            let before = [
                std::fs::read(&path).unwrap(),
                std::fs::read(&library).unwrap(),
            ];
            assert!(save_to(&settings, &path).is_err());
            assert_eq!(
                [
                    std::fs::read(&path).unwrap(),
                    std::fs::read(&library).unwrap()
                ],
                before
            );
        }
    }
}

#[test]
fn cache_observes_replacements_and_uses_original_icon_after_corruption() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("settings.json");
    let mut settings = Settings {
        active: Some("pack/first".into()),
        ..Default::default()
    };
    settings.fallbacks.insert(
        "TestType".into(),
        file_icons_windows::IconLocation {
            path: "original.ico".into(),
            index: 2,
        },
    );
    save_to(&settings, &path).unwrap();
    let mut cache = file_icons_windows::SettingsCache::default();
    assert_eq!(
        cache.get(&path).unwrap().active.as_deref(),
        Some("pack/first")
    );
    settings.active = Some("pack/replacement".into());
    save_to(&settings, &path).unwrap();
    assert_eq!(
        cache.get(&path).unwrap().active.as_deref(),
        Some("pack/replacement")
    );
    std::fs::write(&path, "broken").unwrap();
    let fallback = cache.get(&path).unwrap();
    assert!(fallback.active.is_none());
    assert_eq!(fallback.fallbacks["TestType"].path, "original.ico");
    assert_eq!(fallback.fallback("TESTTYPE").unwrap().path, "original.ico");
    assert!(save_to(&settings, &path).is_err());
    // 外部の修復で正常なファイルが戻ると、キャッシュも回復する。
    std::fs::remove_file(&path).unwrap();
    save_to(&settings, &path).unwrap();
    assert_eq!(
        cache.get(&path).unwrap().active.as_deref(),
        Some("pack/replacement")
    );
    std::fs::remove_file(&path).unwrap();
    let fallback = cache.get(&path).unwrap();
    assert!(fallback.active.is_none());
    assert_eq!(fallback.fallbacks["TestType"].index, 2);
}

#[test]
fn oversized_save_preserves_the_last_readable_settings() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("settings.json");
    let settings = Settings {
        language: "ja".into(),
        ..Default::default()
    };
    save_to(&settings, &path).unwrap();
    let before = std::fs::read(&path).unwrap();
    let oversized = Settings {
        language: "x".repeat(16 * 1024 * 1024),
        ..Default::default()
    };
    assert!(save_to(&oversized, &path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(load_from(&path).unwrap().language, "ja");
}

#[test]
fn cache_recovers_after_a_temporary_read_failure_without_another_file_change() {
    use std::{fs::OpenOptions, os::windows::fs::OpenOptionsExt, time::Duration};
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("settings.json");
    let mut settings = Settings {
        active: Some("pack/first".into()),
        ..Default::default()
    };
    save_to(&settings, &path).unwrap();
    let mut cache = file_icons_windows::SettingsCache::default();
    assert_eq!(cache.get(&path).unwrap().active, settings.active);
    settings.active = Some("pack/after-temporary-lock".into());
    save_to(&settings, &path).unwrap();
    let before = std::fs::metadata(&path).unwrap();
    let lock = OpenOptions::new()
        .read(true)
        .share_mode(6)
        .open(&path)
        .unwrap();
    assert!(cache.get(&path).unwrap().active.is_none());
    drop(lock);
    std::thread::sleep(Duration::from_millis(1100));
    assert_eq!(cache.get(&path).unwrap().active, settings.active);
    let after = std::fs::metadata(&path).unwrap();
    assert_eq!(before.len(), after.len());
    assert_eq!(before.modified().unwrap(), after.modified().unwrap());
}

#[test]
fn library_changes_do_not_rewrite_or_reparse_shell_state() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("settings.json");
    let active = file_icons_core::ImportedPack {
        id: "pack/active".into(),
        version: "1".into(),
        label: "Active".into(),
        theme: Default::default(),
        provenance: root.path().join("active-provenance.json"),
        warnings: Vec::new(),
    };
    let mut settings = Settings {
        language: "ja".into(),
        active: Some(active.id.clone()),
        packs: vec![active.clone()],
        ..Default::default()
    };
    save_to(&settings, &path).unwrap();
    let before = std::fs::metadata(&path).unwrap();
    let mut cache = file_icons_windows::SettingsCache::default();
    let initial = cache.get(&path).unwrap();
    settings.language = "en".into();
    let mut inactive = active.clone();
    inactive.id = "pack/inactive".into();
    settings.packs.push(inactive);
    save_to(&settings, &path).unwrap();
    use std::os::windows::fs::OpenOptionsExt as _;
    let read_denied = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(6)
        .open(&path)
        .unwrap();
    let next = cache.get(&path).unwrap();
    assert_eq!(next.active, initial.active);
    assert_eq!(next.packs.len(), 1);
    assert_eq!(next.packs[0].id, active.id);
    drop(read_denied);
    assert_eq!(load_from(&path).unwrap().packs.len(), 2);
    assert_eq!(
        std::fs::metadata(&path).unwrap().modified().unwrap(),
        before.modified().unwrap()
    );
    assert_eq!(load_from(&path).unwrap().language, "en");
    let document: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(document["format_version"], 4);
    assert_eq!(document["language"], "");
}

#[test]
fn unsupported_settings_formats_remain_untouched() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("settings.json");
    let settings = Settings {
        language: "en".into(),
        ..Default::default()
    };
    save_to(&settings, &path).unwrap();
    let library = path.with_extension("library.json");
    let originals = [
        std::fs::read(&path).unwrap(),
        std::fs::read(&library).unwrap(),
    ];
    for target in [&path, &library] {
        for version in [None, Some(1), Some(2), Some(3), Some(999)] {
            std::fs::write(&path, &originals[0]).unwrap();
            std::fs::write(&library, &originals[1]).unwrap();
            let mut document: serde_json::Value =
                serde_json::from_slice(&std::fs::read(target).unwrap()).unwrap();
            if let Some(version) = version {
                document["format_version"] = version.into();
            } else {
                document.as_object_mut().unwrap().remove("format_version");
            }
            std::fs::write(target, serde_json::to_vec(&document).unwrap()).unwrap();
            let before = [
                std::fs::read(&path).unwrap(),
                std::fs::read(&library).unwrap(),
            ];
            assert!(load_from(&path).is_err());
            assert!(save_to(&settings, &path).is_err());
            assert_eq!(
                [
                    std::fs::read(&path).unwrap(),
                    std::fs::read(&library).unwrap()
                ],
                before
            );
        }
    }
}

#[test]
fn inactive_selection_saves_do_not_change_shell_snapshot() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("settings.json");
    let mut settings = Settings {
        active: Some("one".into()),
        ..Default::default()
    };
    settings
        .active_selection
        .set("extension:json", Some(false))
        .unwrap();
    settings
        .selections
        .insert("one".into(), settings.active_selection.clone());
    save_to(&settings, &path).unwrap();
    let before = std::fs::read(&path).unwrap();
    let mut selection = file_icons_core::selection::Selection::default();
    selection.set("file:cargo.toml", Some(false)).unwrap();
    settings.selections.insert("two".into(), selection);
    save_to(&settings, &path).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), before);
    let mut cache = file_icons_windows::SettingsCache::default();
    let shell = cache.get(&path).unwrap();
    assert!(!shell.active_selection.enabled("data.json"));
    assert!(shell.active_selection.enabled("Cargo.toml"));
    assert!(!load_from(&path).unwrap().selections["two"].enabled("Cargo.toml"));
}
