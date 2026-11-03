use file_icons_core::{CompiledTheme, ThemeRules, selection::Selection};
use std::collections::BTreeMap;

fn targets(cases: &[(&str, &str)]) -> CompiledTheme {
    let mut rules = ThemeRules::default();
    for &(id, _) in cases {
        let (kind, name) = id.split_once(':').unwrap();
        let map = match kind {
            "file" => &mut rules.files,
            "pattern" => &mut rules.language_patterns,
            _ => &mut rules.extensions,
        };
        map.insert(name.into(), "fixture.ico".into());
    }
    CompiledTheme::new(rules).unwrap()
}

#[test]
fn purpose_and_specific_names_determine_the_category() {
    let cases = [
        ("extension:rs", "language"),
        ("extension:sql", "language"),
        ("extension:lua", "language"),
        ("extension:dart", "language"),
        ("extension:fs", "language"),
        ("extension:zig", "language"),
        ("extension:d.ts", "web"),
        ("extension:mjs", "web"),
        ("extension:vue", "web"),
        ("extension:json", "data"),
        ("extension:toml", "data"),
        ("extension:md", "documents"),
        ("extension:psd", "media"),
        ("extension:flac", "media"),
        ("extension:zip", "archives"),
        ("extension:tar.gz", "archives"),
        ("file:cargo.toml", "build"),
        ("file:package.json", "build"),
        ("file:pnpm-lock.yaml", "build"),
        ("file:requirements.txt", "build"),
        ("file:build.gradle.kts", "build"),
        ("file:tsconfig.json", "data"),
        ("file:src/tsconfig.json", "data"),
        ("file:src/package.json", "build"),
        ("file:readme.md", "documents"),
        ("file:license", "documents"),
        ("file:dockerfile", "build"),
        ("file:dockerfile.dev", "build"),
        ("file:compose.prod.yml", "build"),
        ("file:.env.production", "data"),
        ("file:.gitignore", "tools"),
        ("file:agents.md", "tools"),
        ("file:sample.tar.gz", "archives"),
        // 同じ文字列でも、拡張子の規則を固有名として解釈しない。
        ("extension:package.json", "data"),
        ("extension:agents.md", "documents"),
        ("extension:dockerfile", "build"),
    ];
    let theme = targets(&cases);
    let actual: BTreeMap<_, _> = theme
        .targets()
        .into_iter()
        .map(|t| (t.id, t.category))
        .collect();
    for (id, expected) in cases {
        assert_eq!(actual[id], expected, "{id}");
    }
}

#[test]
fn patterns_use_known_definitions_and_unknown_targets_remain_unclassified() {
    let cases = [
        ("pattern:*.sql", "language"),
        ("pattern:src/*.tar.gz", "archives"),
        ("pattern:dockerfile.*", "build"),
        ("pattern:.env.*", "data"),
        ("pattern:*", "other"),
        ("pattern:*.{rs,json}", "other"),
        ("pattern:*.custom.json", "data"),
        ("pattern:custom*.json", "other"),
        ("file:.git-unrelated", "other"),
        ("file:readme.unknown", "other"),
        ("extension:unknown", "other"),
    ];
    let actual: BTreeMap<_, _> = targets(&cases)
        .targets()
        .into_iter()
        .map(|t| (t.id, t.category))
        .collect();
    for (id, expected) in cases {
        assert_eq!(actual[id], expected, "{id}");
    }
}

#[test]
fn categories_are_independent_of_pack_icons_and_rule_sources() {
    let first = CompiledTheme::new(ThemeRules {
        files: BTreeMap::from([("PACKAGE.JSON".into(), "first.ico".into())]),
        extensions: BTreeMap::from([("SQL".into(), "first.ico".into())]),
        ..Default::default()
    })
    .unwrap();
    let second = CompiledTheme::new(ThemeRules {
        language_files: BTreeMap::from([("package.json".into(), "second.ico".into())]),
        language_extensions: BTreeMap::from([("sql".into(), "second.ico".into())]),
        ..Default::default()
    })
    .unwrap();
    let categories = |theme: &CompiledTheme| {
        theme
            .targets()
            .into_iter()
            .map(|t| (t.id, t.category))
            .collect::<BTreeMap<_, _>>()
    };
    assert_eq!(categories(&first), categories(&second));
    assert_eq!(categories(&first)["file:package.json"], "build");
    assert_eq!(categories(&first)["extension:sql"], "language");
}

#[test]
fn regrouping_keeps_saved_target_choices_and_bulk_edits_use_current_membership() {
    let theme = targets(&[
        ("extension:sql", "language"),
        ("extension:json", "data"),
        ("file:package.json", "build"),
        ("extension:zip", "archives"),
        ("extension:exe", "other"),
    ]);
    let mut saved: Selection = serde_json::from_str(
        r#"{"overrides":{"extension:sql":false,"file:package.json":true,"extension:zip":false}}"#,
    )
    .unwrap();
    saved.validate_for(&theme).unwrap();
    let targets = theme.targets();
    assert!(!saved.enabled("query.sql"));
    assert!(!saved.enabled("archive.zip"));
    assert!(saved.enabled("package.json"));
    let sql = targets.iter().find(|t| t.id == "extension:sql").unwrap();
    assert_eq!(sql.category, "language");
    assert!(!saved.target_enabled(sql));
    for target in targets.iter().filter(|t| t.category == "language") {
        saved.set(&target.id, Some(true)).unwrap();
    }
    assert!(saved.enabled("query.sql"));
    assert!(!saved.enabled("archive.zip"));
    assert!(saved.enabled("package.json"));
    assert!(!saved.enabled("program.exe"));
    assert_eq!(saved.explicit("extension:json"), None);
}
