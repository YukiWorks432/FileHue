use file_icons_core::{CompiledTheme, ThemeRules, selection::Selection};
use std::collections::BTreeMap;

#[test]
fn filename_and_pattern_exceptions_keep_unrelated_extension_targets_active() {
    let root = tempfile::tempdir().unwrap();
    let icon = root.path().join("icon.ico");
    std::fs::write(&icon, b"fixture").unwrap();
    let theme = CompiledTheme::new(ThemeRules {
        files: BTreeMap::from([("sample.json".into(), icon.clone())]),
        extensions: BTreeMap::from([("json".into(), icon)]),
        ..Default::default()
    })
    .unwrap();
    let extension = theme
        .targets()
        .into_iter()
        .find(|t| t.id == "extension:json")
        .unwrap();
    let mut selection = Selection::default();
    selection.set("file:sample.json", Some(false)).unwrap();
    assert!(selection.resolve(&theme, "sample.json").is_none());
    assert!(selection.resolve(&theme, "data.json").is_some());
    assert!(selection.target_enabled(&extension));
    assert!(selection.any_enabled(&theme));
    selection.set("pattern:sample.*", Some(false)).unwrap();
    assert!(selection.target_enabled(&extension));
    selection.set("extension:json", Some(false)).unwrap();
    assert!(!selection.any_enabled(&theme));
}

#[test]
fn parent_extension_conditions_precede_general_compound_extensions() {
    let root = tempfile::tempdir().unwrap();
    let icon = root.path().join("icon.ico");
    std::fs::write(&icon, b"fixture").unwrap();
    let theme = CompiledTheme::new(ThemeRules {
        extensions: BTreeMap::from([
            ("gz".into(), icon.clone()),
            ("tar.gz".into(), icon.clone()),
            ("src/gz".into(), icon.clone()),
            ("src/tar.gz".into(), icon),
        ]),
        ..Default::default()
    })
    .unwrap();
    let mut selection = Selection::default();
    selection.set("extension:src/gz", Some(false)).unwrap();
    selection.set("extension:tar.gz", Some(true)).unwrap();
    assert!(selection.resolve(&theme, "C:/SRC/a.TAR.GZ").is_none());
    assert!(selection.resolve(&theme, "other/a.tar.gz").is_some());
    let targets = theme.targets();
    let parent_compound = targets
        .iter()
        .find(|t| t.id == "extension:src/tar.gz")
        .unwrap();
    assert!(!selection.target_enabled(parent_compound));
    selection.set("extension:src/tar.gz", Some(true)).unwrap();
    assert!(selection.resolve(&theme, "src/a.tar.gz").is_some());
    selection.set("extension:src/gz", Some(true)).unwrap();
    selection.set("extension:tar.gz", Some(false)).unwrap();
    selection.set("extension:src/tar.gz", None).unwrap();
    assert!(selection.resolve(&theme, "src/a.tar.gz").is_some());
    assert!(selection.resolve(&theme, "other/a.tar.gz").is_none());
}

#[test]
fn filename_exceptions_and_compound_extensions_do_not_fall_through() {
    let root = tempfile::tempdir().unwrap();
    let icon = root.path().join("icon.ico");
    std::fs::write(&icon, b"fixture").unwrap();
    let theme = CompiledTheme::new(ThemeRules {
        files: BTreeMap::from([
            ("package.json".into(), icon.clone()),
            ("README.md".into(), icon.clone()),
            ("Dockerfile".into(), icon.clone()),
        ]),
        extensions: BTreeMap::from([
            ("json".into(), icon.clone()),
            ("md".into(), icon.clone()),
            ("gz".into(), icon.clone()),
            ("tar.gz".into(), icon.clone()),
        ]),
        ..Default::default()
    })
    .unwrap();
    let mut selection = Selection::default();
    selection.set("extension:json", Some(false)).unwrap();
    assert!(selection.resolve(&theme, "data.json").is_none());
    assert!(selection.resolve(&theme, "package.json").is_none());
    selection.set("file:package.json", Some(true)).unwrap();
    assert_eq!(
        selection.resolve(&theme, "C:/src/PACKAGE.JSON").unwrap().1,
        "file_name"
    );
    selection.set("file:README.md", Some(false)).unwrap();
    assert!(selection.resolve(&theme, "README.md").is_none());
    assert!(selection.resolve(&theme, "notes.md").is_some());
    selection.set("extension:gz", Some(false)).unwrap();
    selection.set("extension:tar.gz", Some(true)).unwrap();
    assert!(selection.resolve(&theme, "a.tar.gz").is_some());
    assert!(selection.resolve(&theme, "a.gz").is_none());
    selection.set("file:dockerfile", Some(false)).unwrap();
    assert!(selection.resolve(&theme, "Dockerfile").is_none());
    selection.set("file:package.json", None).unwrap();
    assert!(selection.resolve(&theme, "package.json").is_none());
    let saved: Selection =
        serde_json::from_slice(&serde_json::to_vec(&selection).unwrap()).unwrap();
    assert!(saved.resolve(&theme, "README.md").is_none());
    assert!(saved.resolve(&theme, "a.tar.gz").is_some());
    assert!(theme.resolve("package.json").is_some());
}
#[test]
fn pattern_cache_parent_conditions_and_copy_preserve_selection_contract() {
    let theme = CompiledTheme::new(ThemeRules {
        language_patterns: BTreeMap::from([("*.test".into(), "test.ico".into())]),
        extensions: BTreeMap::from([("rs".into(), "rust.ico".into())]),
        ..Default::default()
    })
    .unwrap();
    let mut selection = Selection::default();
    selection.set("pattern:*.test", Some(false)).unwrap();
    assert!(!selection.enabled("A.TEST"));
    selection.set("pattern:*.test", Some(true)).unwrap();
    assert!(selection.enabled("A.TEST"));
    selection.set("extension:rs", Some(false)).unwrap();
    selection.set("file:src/main.rs", Some(true)).unwrap();
    assert!(selection.enabled("src/main.rs"));
    assert!(!selection.enabled("other/main.rs"));
    let destination = CompiledTheme::new(ThemeRules {
        extensions: BTreeMap::from([
            ("rs".into(), "rust.ico".into()),
            ("toml".into(), "toml.ico".into()),
        ]),
        ..Default::default()
    })
    .unwrap();
    let mut destination_selection = Selection::default();
    destination_selection
        .set("extension:toml", Some(false))
        .unwrap();
    destination_selection
        .set("extension:rs", Some(true))
        .unwrap();
    destination_selection
        .set("extension:obsolete", Some(false))
        .unwrap();
    let (copied, unmatched) = selection.copy_to(&theme, &destination, &destination_selection);
    assert!(!copied.enabled("lib.rs"));
    assert!(!copied.enabled("config.toml"));
    assert_eq!(copied.explicit("extension:obsolete"), None);
    assert_eq!(unmatched, vec!["pattern:*.test"]);
    assert!(selection.set("file:../secret", Some(false)).is_err());
    assert!(serde_json::from_str::<Selection>(r#"{"overrides":{"pattern:[":false}}"#).is_err());
}

#[test]
fn prepared_search_preserves_case_dots_and_compound_extension_matching() {
    use file_icons_core::selection::{Target, TargetQuery};
    let extension = Target {
        id: "extension:tar.gz".into(),
        name: "tar.gz".into(),
        kind: "extension",
        category: "archives",
    };
    let file = Target {
        id: "file:readme.md".into(),
        name: "readme.md".into(),
        kind: "file",
        category: "documents",
    };
    for (query, extension_matches, file_matches) in [
        ("", true, true),
        (".", true, true),
        (".TAR", true, false),
        (".gz", true, false),
        ("GZ", true, false),
        (".MD", false, true),
        (".readme", false, false),
        ("readme", false, true),
        ("不存在", false, false),
    ] {
        let search = TargetQuery::new(query);
        assert_eq!(search.matches(&extension), extension_matches, "{query}");
        assert_eq!(search.matches(&file), file_matches, "{query}");
        assert_eq!(extension.matches(query), extension_matches, "{query}");
        assert_eq!(file.matches(query), file_matches, "{query}");
    }
}

#[test]
fn parent_pattern_overrides_round_trip_copy_and_inherit_the_same_scope() {
    let root = tempfile::tempdir().unwrap();
    let icon = root.path().join("icon.ico");
    std::fs::write(&icon, b"fixture").unwrap();
    let theme = CompiledTheme::new(ThemeRules {
        language_patterns: BTreeMap::from([
            ("config/*.wfi-test".into(), icon.clone()),
            ("**/rebase-merge/done".into(), icon.clone()),
        ]),
        extensions: BTreeMap::from([("wfi-test".into(), icon)]),
        ..Default::default()
    })
    .unwrap();
    let mut selection = Selection::default();
    selection
        .set("pattern:CONFIG/*.WFI-TEST", Some(false))
        .unwrap();
    selection
        .set("pattern:**/rebase-merge/done", Some(false))
        .unwrap();
    for name in [
        "config/one.wfi-test",
        "C:\\CONFIG\\ONE.WFI-TEST",
        "rebase-merge/done",
    ] {
        assert!(selection.resolve(&theme, name).is_none());
    }
    for name in [
        "one.wfi-test",
        "other/one.wfi-test",
        "config/other/one.wfi-test",
    ] {
        assert!(selection.resolve(&theme, name).is_some());
    }
    let saved: Selection =
        serde_json::from_slice(&serde_json::to_vec(&selection).unwrap()).unwrap();
    let (mut copied, unmatched) = saved.copy_to(&theme, &theme, &Selection::default());
    assert!(unmatched.is_empty());
    assert!(copied.resolve(&theme, "C:\\config\\one.wfi-test").is_none());
    assert!(!copied.enabled(".git/rebase-merge/done"));
    copied.set("pattern:config/*.wfi-test", Some(true)).unwrap();
    assert!(copied.resolve(&theme, "config/one.wfi-test").is_some());
    copied.set("extension:wfi-test", Some(false)).unwrap();
    copied.set("pattern:config/*.wfi-test", None).unwrap();
    assert!(copied.resolve(&theme, "config/one.wfi-test").is_none());
    copied.set("file:config/one.wfi-test", Some(true)).unwrap();
    assert!(copied.resolve(&theme, "config/one.wfi-test").is_some());
    assert!(copied.resolve(&theme, "config/tool.exe").is_none());
    assert!(copied.resolve(&theme, "config/app.LNK").is_none());
}

#[test]
fn windows_owned_icons_override_names_patterns_and_legacy_selections() {
    let root = tempfile::tempdir().unwrap();
    let icon = root.path().join("icon.ico");
    std::fs::write(&icon, b"fixture").unwrap();
    let theme = CompiledTheme::new(ThemeRules {
        files: BTreeMap::from([
            ("tool.exe".into(), icon.clone()),
            ("desktop/app.lnk".into(), icon.clone()),
        ]),
        extensions: BTreeMap::from([
            ("exe".into(), icon.clone()),
            ("lnk".into(), icon.clone()),
            ("rs".into(), icon.clone()),
        ]),
        language_patterns: BTreeMap::from([("*".into(), icon)]),
        ..Default::default()
    })
    .unwrap();
    let mut selection: Selection = serde_json::from_str(
        r#"{"overrides":{"extension:exe":true,"file:desktop/app.lnk":true,"pattern:*":true}}"#,
    )
    .unwrap();
    for name in [
        "tool.exe",
        "TOOL.EXE",
        "C:\\Desktop\\app.LNK",
        "app.rs.exe",
        "missing.lnk",
    ] {
        assert!(!selection.enabled(name), "{name}");
        assert!(selection.resolve(&theme, name).is_none());
        assert!(theme.resolve(name).is_none());
    }
    for id in [
        "extension:exe",
        "extension:src/exe",
        "extension:src/tar.EXE",
        "extension:lnk",
        "extension:tar.lnk",
        "file:tool.exe",
        "file:desktop/app.lnk",
        "pattern:*.exe",
    ] {
        assert!(selection.set(id, Some(true)).is_err());
    }
    assert!(
        serde_json::from_str::<Selection>(r#"{"overrides":{"unknown:app.exe":true}}"#).is_err()
    );
    assert!(
        serde_json::from_str::<Selection>(r#"{"overrides":{"file:../app.exe":true}}"#).is_err()
    );
    assert!(selection.resolve(&theme, "tool.rs").is_some());
    assert_eq!(theme.registration_extensions(), vec!["rs"]);
    assert!(theme.rules().extensions.contains_key("exe"));
    for target in theme.targets().iter().filter(|t| t.protected()) {
        assert!(!selection.target_enabled(target));
    }
    let protected_only = CompiledTheme::new(ThemeRules {
        extensions: BTreeMap::from([
            ("lnk".into(), "link.ico".into()),
            ("exe".into(), "app.ico".into()),
        ]),
        language_patterns: BTreeMap::from([("*.exe".into(), "app.ico".into())]),
        ..Default::default()
    })
    .unwrap();
    assert!(!Selection::default().any_enabled(&protected_only));
    assert!(!protected_only.needs_unknown_class());
    assert!(protected_only.registration_extensions().is_empty());
}
