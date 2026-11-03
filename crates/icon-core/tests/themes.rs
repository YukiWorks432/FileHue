use file_icons_core::{discover, import};
use std::fs;

fn fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("package.json"), r#"{"name":"test-pack","publisher":"tests","version":"1.0.0","contributes":{"iconThemes":[{"id":"test","label":"Test","path":"theme.json"}]}}"#).unwrap();
    fs::write(root.path().join("LICENSE"), "Test fixture artwork: MIT").unwrap();
    fs::write(root.path().join("generic.svg"), r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="16" height="16" fill="#ff0000"/></svg>"##).unwrap();
    fs::write(root.path().join("special.svg"), r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><circle cx="8" cy="8" r="7" fill="#0000ff"/></svg>"##).unwrap();
    fs::write(root.path().join("theme.json"), r#"{
        // VS Codeのコメント付き定義と末尾カンマを受け付ける。
        "iconDefinitions": {"generic":{"iconPath":"generic.svg"}, "special":{"iconPath":"special.svg"}},
        "fileNames":{"Cargo.toml":"special","package.json":"special","README.md":"special","readme.MD":"special","Dockerfile":"special","xposed/module.prop":"special"},
        "fileExtensions":{"toml":"generic","json":"generic","md":"generic","rs":"generic","d.ts":"special","ts":"generic","system/rs":"special"},
    }"#).unwrap();
    root
}

#[test]
fn deeply_nested_json5_is_rejected_for_packages_and_themes() {
    for separator in ["\n", "\r", "\u{2028}", "\u{2029}"] {
        let root = fixture();
        let cache = tempfile::tempdir().unwrap();
        let nested = format!("// comment{separator}{}0{}", "[".repeat(64), "]".repeat(64));
        for name in ["theme.json", "package.json"] {
            let path = root.path().join(name);
            let original = fs::read_to_string(&path).unwrap();
            let modified = format!(
                "{{ extra: {nested}, {}",
                original.trim_start().strip_prefix('{').unwrap()
            );
            fs::write(&path, &modified).unwrap();
            let error = import(root.path(), "test", cache.path()).unwrap_err();
            assert!(format!("{error:#}").contains("JSON5 nesting limit"));
            assert_eq!(fs::read_to_string(&path).unwrap(), modified);
            assert_eq!(fs::read_dir(cache.path()).unwrap().count(), 0);
            fs::write(&path, original).unwrap();
        }
    }
}

#[test]
fn json5_strings_and_comments_do_not_consume_structural_depth() {
    let root = fixture();
    let cache = tempfile::tempdir().unwrap();
    let path = root.path().join("theme.json");
    let original = fs::read_to_string(&path).unwrap();
    // 最上位のオブジェクトと63段の配列で、許可する64段の境界を検証する。
    let nested = format!("{}0{}", "[".repeat(63), "]".repeat(63));
    let modified = format!(
        "{{ extra: {nested}, notes: 'escaped \\' quote {}', /* {} */ // {}\n{}",
        "{[".repeat(256),
        "{[".repeat(256),
        "{[".repeat(256),
        original.trim_start().strip_prefix('{').unwrap(),
    );
    fs::write(&path, modified).unwrap();
    let pack = import(root.path(), "test", cache.path()).unwrap();
    assert!(pack.theme.resolve("sample.json").is_some());
}

#[test]
fn imports_real_assets_and_resolves_contract_examples() {
    let root = fixture();
    let cache = tempfile::tempdir().unwrap();
    assert_eq!(discover(root.path()).unwrap().len(), 1);
    let pack = import(root.path(), "test", cache.path()).unwrap();
    for (special, generic) in [
        ("Cargo.toml", "other.toml"),
        ("package.json", "other.json"),
        ("README.md", "other.md"),
    ] {
        assert_ne!(pack.theme.resolve(special), pack.theme.resolve(generic));
        assert_eq!(
            pack.theme.resolve(special),
            pack.theme.resolve(&special.to_uppercase())
        );
    }
    assert!(pack.theme.resolve("Dockerfile").is_some());
    assert_eq!(
        pack.theme.resolve("C:\\xposed\\module.prop"),
        pack.theme.resolve("Cargo.toml")
    );
    assert!(pack.theme.resolve("other/module.prop").is_none());
    assert!(pack.theme.resolve("xposed/other/module.prop").is_none());
    assert_eq!(
        pack.theme.resolve("system/code.rs"),
        pack.theme.resolve("Cargo.toml")
    );
    assert_eq!(
        pack.theme.resolve("module.d.ts"),
        pack.theme.resolve("Cargo.toml")
    );
    assert!(pack.theme.resolve("unknown.bin").is_none());
    let path = pack.theme.resolve("code.rs").unwrap();
    let ico = ico::IconDir::read(fs::File::open(path).unwrap()).unwrap();
    for size in [16, 20, 24, 32, 48, 128, 256] {
        let image = ico
            .entries()
            .iter()
            .find(|entry| entry.width() == size)
            .unwrap()
            .decode()
            .unwrap();
        let center = ((size / 2 * size + size / 2) * 4) as usize;
        assert_eq!(&image.rgba_data()[center..center + 4], &[255, 0, 0, 255]);
    }
    let provenance = fs::read_to_string(&pack.provenance).unwrap();
    assert!(provenance.contains("Test fixture artwork: MIT"));
    fs::remove_file(path).unwrap();
    assert!(pack.theme.resolve("code.rs").is_none());
}

#[test]
fn rejects_external_paths_undefined_icons_and_font_only_themes() {
    let root = fixture();
    let cache = tempfile::tempdir().unwrap();
    let outside = tempfile::NamedTempFile::new().unwrap();
    for definition in [
        serde_json::json!({"iconDefinitions":{"x":{"iconPath":outside.path()}},"fileExtensions":{"rs":"x"}}),
        serde_json::json!({"fileNames":{"README.md":"missing"}}),
        serde_json::json!({"fonts":[{}],"fileNames":{"README.md":"font"}}),
        serde_json::json!({"iconDefinitions":{"a":{"iconPath":"generic.svg"},"b":{"iconPath":"special.svg"}},"fileNames":{"README.md":"a","readme.MD":"b"}}),
    ] {
        fs::write(root.path().join("theme.json"), definition.to_string()).unwrap();
        assert!(import(root.path(), "test", cache.path()).is_err());
    }
}

#[test]
fn undefined_rules_warn_without_hiding_valid_extension_fallbacks() {
    let root = fixture();
    let cache = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("theme.json"),
        serde_json::json!({
            "iconDefinitions":{"valid":{"iconPath":"generic.svg"}},
            "fileNames":{"Cargo.toml":"absent-file"},
            "fileExtensions":{"toml":"valid","broken":"absent-extension"},
            "languageIds":{"rust":"absent-language"}
        })
        .to_string(),
    )
    .unwrap();
    let pack = import(root.path(), "test", cache.path()).unwrap();
    assert_eq!(
        pack.theme.resolve("Cargo.toml"),
        pack.theme.resolve("normal.toml")
    );
    assert!(pack.theme.resolve("normal.toml").unwrap().is_file());
    assert!(pack.theme.resolve("file.broken").is_none());
    assert!(pack.theme.resolve("source.rs").is_none());
    let warnings = serde_json::json!([
        {"rule_type":"fileNames","rule":"Cargo.toml","icon_id":"absent-file"},
        {"rule_type":"fileExtensions","rule":"broken","icon_id":"absent-extension"},
        {"rule_type":"languageIds","rule":"rust","icon_id":"absent-language"}
    ]);
    assert_eq!(serde_json::to_value(&pack.warnings).unwrap(), warnings);
    let record: serde_json::Value =
        serde_json::from_slice(&fs::read(&pack.provenance).unwrap()).unwrap();
    assert_eq!(record["warnings"], warnings);
    let again = import(root.path(), "test", cache.path()).unwrap();
    assert_eq!(again.warnings, pack.warnings);
    assert_eq!(again.provenance, pack.provenance);

    // 既存の保存形式には警告欄がなくてもよい。
    let mut legacy = serde_json::to_value(&pack).unwrap();
    legacy.as_object_mut().unwrap().remove("warnings");
    let legacy: file_icons_core::ImportedPack = serde_json::from_value(legacy).unwrap();
    assert!(legacy.warnings.is_empty());
    assert_eq!(
        legacy.theme.resolve("normal.toml"),
        pack.theme.resolve("normal.toml")
    );
    fs::remove_file(root.path().join("generic.svg")).unwrap();
    assert!(import(root.path(), "test", cache.path()).is_err());
}

#[test]
fn resolves_language_ids_with_official_catalog_and_keeps_theme_rules_stronger() {
    let root = fixture();
    let cache = tempfile::tempdir().unwrap();
    fs::write(root.path().join("theme.json"), serde_json::json!({
        "iconDefinitions":{"generic":{"iconPath":"generic.svg"},"special":{"iconPath":"special.svg"}},
        "languageIds":{"rust":"generic","json":"generic","markdown":"generic"},
        "fileExtensions":{"rs":"special"}
    }).to_string()).unwrap();
    let pack = import(root.path(), "test", cache.path()).unwrap();
    assert!(pack.theme.resolve("file.json").is_some());
    assert_eq!(
        pack.theme.resolve("README.md"),
        pack.theme.resolve("file.json")
    );
    assert_ne!(
        pack.theme.resolve("file.rs"),
        pack.theme.resolve("file.json")
    );
    assert!(
        pack.theme
            .registration_extensions()
            .contains(&"json".into())
    );
    assert!(pack.theme.registration_extensions().contains(&"md".into()));
}

#[test]
fn theme_aliases_keep_their_own_identity_and_provenance() {
    let root = fixture();
    let cache = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("package.json"),
        serde_json::json!({
            "name":"test-pack", "publisher":"tests", "version":"1.0.0", "displayName":"Sample pack",
            "contributes":{"iconThemes":[
                {"id":"one", "label":"First theme", "path":"theme.json"},
                {"id":"two", "label":"Second theme", "path":"theme.json"}
            ]}
        })
        .to_string(),
    )
    .unwrap();
    let one = import(root.path(), "one", cache.path()).unwrap();
    let original = fs::read(&one.provenance).unwrap();
    let two = import(root.path(), "two", cache.path()).unwrap();
    assert_ne!(one.id, two.id);
    assert_ne!(one.provenance, two.provenance);
    assert!(one.label.contains("First theme"));
    assert!(two.label.contains("Second theme"));
    assert_eq!(fs::read(&one.provenance).unwrap(), original);
    let metadata: serde_json::Value =
        serde_json::from_slice(&fs::read(&two.provenance).unwrap()).unwrap();
    assert_eq!(metadata["theme_id"], "two");
    assert!(one.theme.resolve("code.rs").unwrap().is_file());
    assert!(two.theme.resolve("code.rs").unwrap().is_file());
}

#[test]
fn changed_artwork_or_licenses_never_mutate_a_previously_imported_pack() {
    let root = fixture();
    let cache = tempfile::tempdir().unwrap();
    let first = import(root.path(), "test", cache.path()).unwrap();
    let first_metadata = fs::read(&first.provenance).unwrap();
    let first_icon = fs::read(first.theme.resolve("code.rs").unwrap()).unwrap();
    let reused = import(root.path(), "test", cache.path()).unwrap();
    assert_eq!(reused.provenance, first.provenance);
    fs::write(
        root.path().join("LICENSE"),
        "Updated fixture artwork notice",
    )
    .unwrap();
    let relabeled = import(root.path(), "test", cache.path()).unwrap();
    assert_ne!(first.provenance, relabeled.provenance);
    assert_eq!(fs::read(&first.provenance).unwrap(), first_metadata);
    fs::write(root.path().join("generic.svg"), r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="16" height="16" fill="#00ff00"/></svg>"##).unwrap();
    let changed = import(root.path(), "test", cache.path()).unwrap();
    assert_ne!(changed.provenance, relabeled.provenance);
    assert_eq!(
        fs::read(first.theme.resolve("code.rs").unwrap()).unwrap(),
        first_icon
    );
    let icon =
        ico::IconDir::read(fs::File::open(changed.theme.resolve("code.rs").unwrap()).unwrap())
            .unwrap();
    let pixels = icon
        .entries()
        .iter()
        .find(|entry| entry.width() == 16)
        .unwrap()
        .decode()
        .unwrap();
    assert_eq!(
        &pixels.rgba_data()[((8 * 16 + 8) * 4)..((8 * 16 + 8) * 4 + 4)],
        &[0, 255, 0, 255]
    );
    fs::write(&changed.provenance, b"damaged provenance").unwrap();
    assert!(import(root.path(), "test", cache.path()).is_err());
    assert_eq!(fs::read(changed.provenance).unwrap(), b"damaged provenance");
}

#[test]
fn cache_reuse_rejects_valid_but_changed_images_without_overwriting_them() {
    let root = fixture();
    let cache = tempfile::tempdir().unwrap();
    let pack = import(root.path(), "test", cache.path()).unwrap();
    let icon = pack.theme.resolve("code.rs").unwrap().to_path_buf();
    let other = fs::read(pack.theme.resolve("Cargo.toml").unwrap()).unwrap();
    fs::write(&icon, &other).unwrap();
    assert!(import(root.path(), "test", cache.path()).is_err());
    assert_eq!(fs::read(icon).unwrap(), other);
}

#[test]
fn cache_reuse_refuses_links_for_images_provenance_and_buckets() {
    let root = fixture();
    let cache = tempfile::tempdir().unwrap();
    let pack = import(root.path(), "test", cache.path()).unwrap();
    let icon = pack.theme.resolve("code.rs").unwrap().to_path_buf();
    let outside = tempfile::tempdir().unwrap();
    let external = outside.path().join("image.ico");
    let bytes = fs::read(&icon).unwrap();
    fs::write(&external, &bytes).unwrap();
    fs::remove_file(&icon).unwrap();
    #[cfg(windows)]
    let linked = std::os::windows::fs::symlink_file(&external, &icon);
    #[cfg(unix)]
    let linked = std::os::unix::fs::symlink(&external, &icon);
    if let Err(error) = linked {
        eprintln!("symlink unavailable: {error}");
        return;
    }
    assert!(import(root.path(), "test", cache.path()).is_err());
    assert_eq!(fs::read(&external).unwrap(), bytes);
    fs::remove_file(&icon).unwrap();
    fs::write(&icon, &bytes).unwrap();
    let record = fs::read(&pack.provenance).unwrap();
    let external_record = outside.path().join("provenance.json");
    fs::write(&external_record, &record).unwrap();
    fs::remove_file(&pack.provenance).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(&external_record, &pack.provenance).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&external_record, &pack.provenance).unwrap();
    assert!(import(root.path(), "test", cache.path()).is_err());
    assert_eq!(fs::read(&external_record).unwrap(), record);
    fs::remove_file(&pack.provenance).unwrap();
    fs::write(&pack.provenance, &record).unwrap();
    let bucket = pack.provenance.parent().unwrap();
    let external_bucket = outside.path().join("bucket");
    fs::create_dir(&external_bucket).unwrap();
    for entry in fs::read_dir(bucket).unwrap() {
        let entry = entry.unwrap();
        fs::copy(entry.path(), external_bucket.join(entry.file_name())).unwrap();
        fs::remove_file(entry.path()).unwrap();
    }
    fs::remove_dir(bucket).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&external_bucket, bucket).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&external_bucket, bucket).unwrap();
    assert!(import(root.path(), "test", cache.path()).is_err());
    assert_eq!(
        fs::read(external_bucket.join("provenance.json")).unwrap(),
        record
    );
}

#[test]
fn notice_collection_refuses_total_bytes_and_file_count_before_publishing() {
    let root = fixture();
    let cache = tempfile::tempdir().unwrap();
    for index in 0..128 {
        fs::write(
            root.path().join(format!("NOTICE-{index:03}")),
            vec![b'x'; 64 * 1024],
        )
        .unwrap();
    }
    let error = import(root.path(), "test", cache.path()).unwrap_err();
    assert!(format!("{error:#}").contains("notice byte budget"));
    assert_eq!(fs::read_dir(cache.path()).unwrap().count(), 0);
    for index in 0..257 {
        fs::write(root.path().join(format!("NOTICE-{index:03}")), b"").unwrap();
    }
    let error = import(root.path(), "test", cache.path()).unwrap_err();
    assert!(format!("{error:#}").contains("too many notice files"));
    assert_eq!(fs::read_dir(cache.path()).unwrap().count(), 0);
}

#[test]
fn replacing_rules_rebuilds_patterns_and_rejects_invalid_updates_atomically() {
    let root = fixture();
    let cache = tempfile::tempdir().unwrap();
    let mut pack = import(root.path(), "test", cache.path()).unwrap();
    let icon = pack.theme.resolve("code.rs").unwrap().to_path_buf();
    let mut rules = pack.theme.rules().clone();
    rules.language_patterns.insert("*.old".into(), icon.clone());
    pack.theme.replace_rules(rules).unwrap();
    assert_eq!(pack.theme.resolve("sample.old"), Some(icon.as_path()));
    let mut rules = pack.theme.rules().clone();
    rules.language_patterns.clear();
    rules.language_patterns.insert("*.new".into(), icon.clone());
    pack.theme.replace_rules(rules).unwrap();
    assert!(pack.theme.resolve("sample.old").is_none());
    assert_eq!(pack.theme.resolve("sample.new"), Some(icon.as_path()));
    let mut invalid = pack.theme.rules().clone();
    invalid.language_patterns.insert("[".into(), icon.clone());
    assert!(pack.theme.replace_rules(invalid).is_err());
    assert_eq!(pack.theme.resolve("sample.new"), Some(icon.as_path()));
    let serialized = serde_json::to_vec(&pack.theme).unwrap();
    let decoded: file_icons_core::CompiledTheme = serde_json::from_slice(&serialized).unwrap();
    assert_eq!(decoded.resolve("sample.new"), Some(icon.as_path()));
}

#[test]
fn pattern_aliases_preserve_selection_and_reject_conflicting_updates() {
    use file_icons_core::{CompiledTheme, selection::Selection};
    let root = fixture();
    let cache = tempfile::tempdir().unwrap();
    let pack = import(root.path(), "test", cache.path()).unwrap();
    let first = pack.theme.resolve("code.rs").unwrap().to_path_buf();
    let second = pack.theme.resolve("Cargo.toml").unwrap().to_path_buf();
    let mut rules = pack.theme.rules().clone();
    rules
        .language_patterns
        .insert("*.TeSt".into(), first.clone());
    rules
        .language_patterns
        .insert("*.test".into(), first.clone());
    let mut theme = CompiledTheme::new(rules).unwrap();
    assert_eq!(theme.resolve("SAMPLE.TEST"), Some(first.as_path()));
    assert_eq!(
        theme
            .targets()
            .iter()
            .filter(|t| t.id == "pattern:*.test")
            .count(),
        1
    );
    let mut selection = Selection::default();
    selection.set("pattern:*.TEST", Some(false)).unwrap();
    let saved = serde_json::to_vec(&selection).unwrap();
    let decoded: Selection = serde_json::from_slice(&saved).unwrap();
    assert!(decoded.resolve(&theme, "sample.test").is_none());
    let original = serde_json::to_vec(&theme).unwrap();
    let mut conflicting = theme.rules().clone();
    conflicting
        .language_patterns
        .insert("*.TEST".into(), second.clone());
    assert!(CompiledTheme::new(conflicting.clone()).is_err());
    assert!(theme.replace_rules(conflicting.clone()).is_err());
    assert_eq!(serde_json::to_vec(&theme).unwrap(), original);
    assert_eq!(theme.resolve("sample.test"), Some(first.as_path()));
    assert!(
        serde_json::from_value::<CompiledTheme>(serde_json::to_value(conflicting).unwrap())
            .is_err()
    );
    // 有効な旧記録の混在表記は同じ対象へまとまり、別パターンの既存キー順は保つ。
    let mut legacy = theme.rules().clone();
    legacy
        .language_patterns
        .insert("*.TEST".into(), first.clone());
    legacy.language_patterns.insert("sample.*".into(), second);
    let decoded: CompiledTheme =
        serde_json::from_value(serde_json::to_value(legacy).unwrap()).unwrap();
    assert_eq!(decoded.resolve("sample.test"), Some(first.as_path()));
    assert!(decoded.resolve("sample.exe").is_none());
    assert!(decoded.resolve("sample.LNK").is_none());
}

#[test]
fn language_declarations_do_not_hide_duplicate_pattern_images() {
    let root = fixture();
    let cache = tempfile::tempdir().unwrap();
    let package_path = root.path().join("package.json");
    let mut package: serde_json::Value =
        serde_json::from_slice(&fs::read(&package_path).unwrap()).unwrap();
    fs::write(
        root.path().join("theme.json"),
        r#"{
        "iconDefinitions":{"first":{"iconPath":"generic.svg"},"second":{"iconPath":"special.svg"}},
        "languageIds":{"wfi-first":"first","wfi-second":"second"}
    }"#,
    )
    .unwrap();
    for second_pattern in ["*.wfi-test", "*.WFI-TEST"] {
        package["contributes"]["languages"] = serde_json::json!([
            {"id":"wfi-first","filenamePatterns":["*.wfi-test"]},
            {"id":"wfi-second","filenamePatterns":[second_pattern]}
        ]);
        fs::write(&package_path, serde_json::to_vec(&package).unwrap()).unwrap();
        let error = import(root.path(), "test", cache.path()).unwrap_err();
        assert!(format!("{error:#}").contains("conflicting case-insensitive pattern"));
    }
    // 識別子が異なっても同じ内容の画像なら、一つの対象として取り込める。
    fs::copy(
        root.path().join("generic.svg"),
        root.path().join("special.svg"),
    )
    .unwrap();
    let pack = import(root.path(), "test", cache.path()).unwrap();
    assert_eq!(
        pack.theme
            .targets()
            .iter()
            .filter(|t| t.kind == "pattern")
            .count(),
        1
    );
    let icon = pack.theme.resolve("sample.WFI-test").unwrap();
    let image = ico::IconDir::read(fs::File::open(icon).unwrap())
        .unwrap()
        .entries()[0]
        .decode()
        .unwrap();
    assert_eq!(&image.rgba_data()[..4], &[255, 0, 0, 255]);
}

#[test]
fn parent_patterns_use_only_the_immediate_parent_and_keep_resolution_priority() {
    let root = fixture();
    let cache = tempfile::tempdir().unwrap();
    let pack = import(root.path(), "test", cache.path()).unwrap();
    let generic = pack.theme.resolve("code.rs").unwrap().to_path_buf();
    let special = pack.theme.resolve("Cargo.toml").unwrap().to_path_buf();
    let theme = file_icons_core::CompiledTheme::new(file_icons_core::ThemeRules {
        files: std::collections::BTreeMap::from([(
            "config/priority.wfi-test".into(),
            generic.clone(),
        )]),
        extensions: std::collections::BTreeMap::from([("wfi-hard".into(), generic.clone())]),
        language_patterns: std::collections::BTreeMap::from([
            ("config/*.wfi-test".into(), special.clone()),
            ("*.plain".into(), generic.clone()),
            ("**/rebase-merge/done".into(), special.clone()),
            ("**/.github/hooks/*.deep".into(), special.clone()),
            ("config/*.wfi-hard".into(), special.clone()),
        ]),
        language_extensions: std::collections::BTreeMap::from([(
            "wfi-test".into(),
            generic.clone(),
        )]),
        ..Default::default()
    })
    .unwrap();
    for name in [
        "config/one.wfi-test",
        "C:\\sample\\CONFIG\\ONE.WFI-TEST",
        "elsewhere/config/one.wfi-test",
    ] {
        assert_eq!(
            theme.resolve_with_reason(name),
            Some((special.as_path(), "language_pattern"))
        );
    }
    for name in [
        "other/one.wfi-test",
        "config/other/one.wfi-test",
        "one.wfi-test",
    ] {
        assert_eq!(
            theme.resolve_with_reason(name),
            Some((generic.as_path(), "language_extension"))
        );
    }
    for name in ["one.plain", "C:\\anywhere\\ONE.PLAIN"] {
        assert_eq!(theme.resolve(name), Some(generic.as_path()));
    }
    for name in ["rebase-merge/done", "C:\\sample\\.git\\REBASE-MERGE\\DONE"] {
        assert_eq!(theme.resolve(name), Some(special.as_path()));
    }
    assert!(theme.resolve("other/done").is_none());
    assert!(theme.resolve(".github/hooks/one.deep").is_none());
    assert_eq!(
        theme.resolve_with_reason("config/priority.wfi-test"),
        Some((generic.as_path(), "file_name"))
    );
    assert_eq!(
        theme.resolve_with_reason("config/one.wfi-hard"),
        Some((generic.as_path(), "file_extension"))
    );
}

#[test]
fn discovery_and_import_refuse_empty_and_duplicate_theme_ids() {
    let root = fixture();
    let cache = tempfile::tempdir().unwrap();
    for ids in [vec![""], vec!["   "], vec!["same", "same"]] {
        let choices = ids.iter().enumerate().map(|(index,id)| serde_json::json!({"id":id,"label":format!("Choice {index}"),"path":"theme.json"})).collect::<Vec<_>>();
        fs::write(root.path().join("package.json"), serde_json::to_vec(&serde_json::json!({"name":"test","version":"1","contributes":{"iconThemes":choices}})).unwrap()).unwrap();
        assert!(discover(root.path()).is_err());
        assert!(import(root.path(), ids[0], cache.path()).is_err());
        assert_eq!(fs::read_dir(cache.path()).unwrap().count(), 0);
    }
}

#[test]
fn notice_named_artwork_does_not_replace_license_text() {
    let root = fixture();
    let cache = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("preview")).unwrap();
    image::RgbaImage::from_pixel(16, 16, image::Rgba([255, 0, 0, 255]))
        .save(root.path().join("preview/LICENSE.png"))
        .unwrap();
    fs::copy(
        root.path().join("generic.svg"),
        root.path().join("license.svg"),
    )
    .unwrap();
    let pack = import(root.path(), "test", cache.path()).unwrap();
    assert!(pack.theme.resolve("code.rs").unwrap().is_file());
    let record: serde_json::Value =
        serde_json::from_slice(&fs::read(pack.provenance).unwrap()).unwrap();
    assert_eq!(
        record["licenses"],
        serde_json::json!({"LICENSE": "Test fixture artwork: MIT"})
    );

    let manifest = root.path().join("package.json");
    let mut package: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    package["license"] = "SEE LICENSE IN license.svg".into();
    fs::write(&manifest, serde_json::to_vec(&package).unwrap()).unwrap();
    let explicit = import(root.path(), "test", cache.path()).unwrap();
    let record: serde_json::Value =
        serde_json::from_slice(&fs::read(explicit.provenance).unwrap()).unwrap();
    assert_eq!(
        record["licenses"]["license.svg"],
        fs::read_to_string(root.path().join("license.svg")).unwrap()
    );

    // 明示参照は拡張子で省略せず、読めない許諾文は取り込みを失敗させる。
    package["license"] = "SEE LICENSE IN preview/LICENSE.png".into();
    fs::write(&manifest, serde_json::to_vec(&package).unwrap()).unwrap();
    assert!(import(root.path(), "test", cache.path()).is_err());
}

#[test]
fn provenance_keeps_nested_notices_and_explicit_license_references() {
    let root = fixture();
    let cache = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("docs")).unwrap();
    fs::create_dir(root.path().join("licenses")).unwrap();
    for (name, text) in [
        ("docs/terms.txt", "Explicit permission 原文"),
        ("docs/NOTICE.md", "Nested notice"),
        ("licenses/vendor.txt", "Vendor permission"),
        ("licenses/vendor.svg", "Permission in a license directory"),
        ("COPYRIGHT.txt", "Copyright holder"),
        ("THIRD_PARTY_NOTICES.txt", "Additional authors"),
    ] {
        fs::write(root.path().join(name), text).unwrap();
    }
    let manifest = root.path().join("package.json");
    let mut package: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    package["license"] = "SEE LICENSE IN docs/terms.txt".into();
    fs::write(&manifest, serde_json::to_vec(&package).unwrap()).unwrap();
    let pack = import(root.path(), "test", cache.path()).unwrap();
    let record: serde_json::Value =
        serde_json::from_slice(&fs::read(pack.provenance).unwrap()).unwrap();
    assert_eq!(
        record["licenses"]["docs/terms.txt"],
        "Explicit permission 原文"
    );
    assert_eq!(record["licenses"]["docs/NOTICE.md"], "Nested notice");
    assert_eq!(
        record["licenses"]["licenses/vendor.txt"],
        "Vendor permission"
    );
    assert_eq!(record["licenses"]["COPYRIGHT.txt"], "Copyright holder");
    assert_eq!(
        record["licenses"]["licenses/vendor.svg"],
        "Permission in a license directory"
    );
    assert_eq!(
        record["licenses"]["THIRD_PARTY_NOTICES.txt"],
        "Additional authors"
    );
    package["license"] = "SEE LICENSE IN ../outside.txt".into();
    fs::write(&manifest, serde_json::to_vec(&package).unwrap()).unwrap();
    assert!(import(root.path(), "test", cache.path()).is_err());
}
