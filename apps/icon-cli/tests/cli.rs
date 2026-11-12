//! 実CLIの標準入出力・保存・変更予定を専用プロファイルで検証する。
#[path = "../../../test-support/process.rs"]
mod test_process;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    path::Path,
    process::{Command, Output},
};
use test_process::CommandOutput;
fn run(profile: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_file-icons"))
        .args(args)
        .env("LOCALAPPDATA", profile)
        .test_output()
        .unwrap()
}
fn document(output: &Output, code: i32) -> Value {
    assert_eq!(
        output.status.code(),
        Some(code),
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
fn fixture(path: &Path) {
    fs::create_dir_all(path).unwrap();
    fs::write(path.join("package.json"),r#"{"name":"cli-fixture","version":"1","license":"MIT","contributes":{"iconThemes":[{"id":"one","label":"One","path":"theme.json"},{"id":"two","label":"Two","path":"theme.json"}]}}"#).unwrap();
    fs::write(path.join("theme.json"),r#"{"iconDefinitions":{"x":{"iconPath":"icon.svg"}},"fileNames":{"Cargo.toml":"x"},"fileExtensions":{"wfi-cli-probe":"x"}}"#).unwrap();
    fs::write(path.join("icon.svg"),r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="16" height="16" fill="#00ff00"/></svg>"##).unwrap();
    fs::write(path.join("LICENSE"), "Fixture permission 原文\n").unwrap();
}
fn files(path: &Path) -> BTreeMap<std::path::PathBuf, Vec<u8>> {
    let mut result = BTreeMap::new();
    if path.is_dir() {
        for entry in fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                result.extend(files(&path));
            } else if path.file_name().unwrap() != "manager.lock" {
                result.insert(path.clone(), fs::read(path).unwrap());
            }
        }
    }
    result
}

#[test]
fn parent_patterns_resolve_and_saved_overrides_use_the_same_file_scope() {
    let profile = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    fixture(source.path());
    let package_path = source.path().join("package.json");
    let mut package: Value = serde_json::from_slice(&fs::read(&package_path).unwrap()).unwrap();
    package["contributes"]["languages"] = serde_json::json!([{ "id":"wfi-pattern", "filenamePatterns":["config/*.wfi-test", "**/rebase-merge/done"] }]);
    fs::write(package_path, serde_json::to_vec(&package).unwrap()).unwrap();
    fs::write(
        source.path().join("theme.json"),
        r#"{"iconDefinitions":{"x":{"iconPath":"icon.svg"}},"languageIds":{"wfi-pattern":"x"}}"#,
    )
    .unwrap();
    let imported = document(
        &run(
            profile.path(),
            &[
                "pack",
                "import",
                source.path().to_str().unwrap(),
                "--theme",
                "one",
                "--json",
            ],
        ),
        0,
    );
    let id = imported["data"]["pack_id"].as_str().unwrap();
    let first = document(
        &run(
            profile.path(),
            &["resolve", id, "C:\\sample\\CONFIG\\one.WFI-TEST", "--json"],
        ),
        0,
    );
    assert_eq!(first["data"]["reason"], "language_pattern");
    assert!(Path::new(first["data"]["icon"].as_str().unwrap()).is_file());
    for (path, reason) in [
        ("other/one.wfi-test", "windows_default"),
        ("config/other/one.wfi-test", "windows_default"),
        (".git/rebase-merge/done", "language_pattern"),
    ] {
        assert_eq!(
            document(&run(profile.path(), &["resolve", id, path, "--json"]), 0)["data"]["reason"],
            reason
        );
    }
    let changes = source.path().join("changes.json");
    for (value, expected) in [
        (Some(false), "windows_default"),
        (Some(true), "language_pattern"),
        (None, "language_pattern"),
    ] {
        fs::write(&changes, serde_json::to_vec(&serde_json::json!({"pattern:config/*.wfi-test":value,"pattern:**/rebase-merge/done":value})).unwrap()).unwrap();
        document(
            &run(
                profile.path(),
                &["targets", "set", id, changes.to_str().unwrap(), "--json"],
            ),
            0,
        );
        for path in ["C:\\sample\\config\\one.wfi-test", ".git/rebase-merge/done"] {
            let result = document(&run(profile.path(), &["resolve", id, path, "--json"]), 0);
            assert_eq!(result["data"]["reason"], expected);
            if expected == "windows_default" {
                assert!(result["data"]["icon"].is_null());
            } else {
                assert_eq!(result["data"]["icon"], first["data"]["icon"]);
            }
        }
    }
    let state = document(&run(profile.path(), &["status", "--json"]), 0);
    assert_eq!(state["data"]["registration_state"], "inactive");
}

#[test]
fn protection_setting_is_default_on_and_dry_run_does_not_persist() {
    let profile = tempfile::tempdir().unwrap();
    assert_eq!(
        document(&run(profile.path(), &["protection", "show", "--json"]), 0)["data"]["preserve_existing_icons"],
        true
    );
    let before = files(profile.path());
    assert_eq!(
        document(
            &run(
                profile.path(),
                &["protection", "set", "off", "--dry-run", "--json"]
            ),
            0
        )["data"]["preserve_existing_icons"],
        false
    );
    assert_eq!(files(profile.path()), before);
    document(
        &run(profile.path(), &["protection", "set", "off", "--json"]),
        0,
    );
    assert_eq!(
        document(&run(profile.path(), &["protection", "show", "--json"]), 0)["data"]["preserve_existing_icons"],
        false
    );
    document(
        &run(profile.path(), &["protection", "set", "on", "--json"]),
        0,
    );
    assert_eq!(
        document(&run(profile.path(), &["protection", "show", "--json"]), 0)["data"]["preserve_existing_icons"],
        true
    );
    let before = files(profile.path());
    document(
        &run(profile.path(), &["protection", "set", "inherit", "--json"]),
        2,
    );
    assert_eq!(files(profile.path()), before);
    for (language, label) in [
        ("ja", "既存のアイコンを保護する"),
        ("en", "Protect existing icons"),
    ] {
        let output = run(profile.path(), &["protection", "show", "--lang", language]);
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains(label));
    }
}
#[test]
fn real_process_import_resolve_rights_dry_runs_and_removal_preserve_sources() {
    let profile = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let extension = source.path().join("日本語の テーマ");
    fixture(&extension);
    let theme_path = extension.join("theme.json");
    let mut theme: Value = serde_json::from_slice(&fs::read(&theme_path).unwrap()).unwrap();
    // 実ユーザーの.toml関連付けに依存せず、専用拡張子で名前の一致を確認する。
    theme["fileNames"] = serde_json::json!({"fixture.wfi-cli-probe":"x"});
    theme["fileExtensions"]["broken"] = "undefined".into();
    fs::write(&theme_path, serde_json::to_vec(&theme).unwrap()).unwrap();
    let source_before = files(source.path());
    let path = extension.to_str().unwrap();
    assert_eq!(
        document(
            &run(profile.path(), &["pack", "inspect", path, "--json"]),
            0
        )["data"]["themes"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    document(&run(profile.path(), &["pack", "import", path, "--json"]), 2);
    let result = document(
        &run(
            profile.path(),
            &["pack", "import", path, "--theme", "two", "--json"],
        ),
        0,
    );
    let id = result["data"]["pack_id"].as_str().unwrap();
    assert_eq!(result["data"]["warning_count"], 1);
    assert!(result["warnings"][0].as_str().unwrap().contains("除外"));
    let english = document(
        &run(
            profile.path(),
            &[
                "pack", "import", path, "--theme", "two", "--lang", "en", "--json",
            ],
        ),
        0,
    );
    assert_eq!(english["data"]["warning_count"], 1);
    assert!(
        english["warnings"][0]
            .as_str()
            .unwrap()
            .contains("were skipped")
    );
    assert_eq!(
        document(
            &run(
                profile.path(),
                &["resolve", id, "FIXTURE.WFI-CLI-PROBE", "--json"]
            ),
            0
        )["data"]["reason"],
        "file_name"
    );
    assert_eq!(
        document(
            &run(profile.path(), &["resolve", id, "unknown.zzz", "--json"]),
            0
        )["data"]["reason"],
        "windows_default"
    );
    let rights = document(&run(profile.path(), &["pack", "show", id, "--json"]), 0);
    assert_eq!(rights["data"]["provenance"]["theme_id"], "two");
    assert_eq!(
        rights["data"]["provenance"]["warnings"],
        serde_json::json!([
            {"rule_type":"fileExtensions","rule":"broken","icon_id":"undefined"}
        ])
    );
    assert_eq!(
        rights["data"]["provenance"]["licenses"]["LICENSE"],
        "Fixture permission 原文\n"
    );
    let state = profile.path().join("WindowsFileIconsDev");
    let before = files(&state);
    for args in [
        vec!["apply", id, "--dry-run", "--json"],
        vec!["restore", "--dry-run", "--json"],
        vec!["recover", "--dry-run", "--json"],
        vec!["cleanup", "--dry-run", "--json"],
        vec!["pack", "remove", id, "--dry-run", "--json"],
    ] {
        assert_eq!(
            document(&run(profile.path(), &args), 0)["data"]["dry_run"],
            true
        );
        assert_eq!(files(&state), before);
    }
    document(&run(profile.path(), &["pack", "remove", id, "--json"]), 0);
    assert_eq!(
        document(&run(profile.path(), &["pack", "list", "--json"]), 0)["data"]["packs"],
        serde_json::json!([])
    );
    assert_eq!(files(source.path()), source_before);
}
#[test]
fn output_contract_errors_locking_and_saved_language() {
    let profile = tempfile::tempdir().unwrap();
    let help = document(
        &run(profile.path(), &["--help", "--lang", "en", "--json"]),
        0,
    );
    assert!(
        help["data"]["help"]
            .as_str()
            .unwrap()
            .contains("Manage Windows")
    );
    for args in [
        vec!["no-command", "--json"],
        vec!["status", "unexpected", "--json"],
        vec!["status", "--dry-run", "--json"],
        vec![
            "pack", "import", "folder", "--theme", "one", "--theme", "two", "--json",
        ],
    ] {
        assert_eq!(
            document(&run(profile.path(), &args), 2)["error"]["code"],
            "invalid_arguments"
        );
    }
    let root = profile.path().join("WindowsFileIconsDev");
    fs::create_dir_all(&root).unwrap();
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join("manager.lock"))
        .unwrap();
    lock.try_lock().unwrap();
    assert_eq!(
        document(&run(profile.path(), &["status", "--json"]), 3)["error"]["code"],
        "manager_busy"
    );
    // 独立したカタログ照会はGUIの排他中も使える。
    let available = document(&run(profile.path(), &["catalog", "list", "--json"]), 0);
    assert!(
        available["data"]["catalog"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["id"] == "symbols")
    );
    drop(lock);
    let settings = file_icons_windows::Settings {
        language: "en".into(),
        ..Default::default()
    };
    file_icons_windows::save_to(&settings, &root.join("settings.json")).unwrap();
    let error = document(
        &run(profile.path(), &["pack", "show", "missing", "--json"]),
        4,
    );
    assert_eq!(error["error"]["code"], "not_found");
    assert_eq!(
        error["error"]["message"],
        "The requested identifier was not found."
    );
    let original = files(&root);
    document(
        &run(profile.path(), &["status", "--lang", "ja", "--json"]),
        0,
    );
    assert_eq!(files(&root), original);
    fs::write(root.join("settings.json"), b"broken").unwrap();
    let broken = document(&run(profile.path(), &["status", "--json"]), 7);
    assert_eq!(broken["error"]["code"], "recovery_required");
    assert_eq!(fs::read(root.join("settings.json")).unwrap(), b"broken");
}

#[test]
#[ignore = "公式配布元への通信と全テーマの変換を伴う"]
fn official_downloads_through_real_cli_keep_registration_inactive() {
    let profile = tempfile::tempdir().unwrap();
    for id in [
        "vscode-icons",
        "material-icon-theme",
        "catppuccin-icons",
        "symbols",
        "vscode-great-icons",
    ] {
        let result = document(&run(profile.path(), &["pack", "download", id, "--json"]), 0);
        assert_eq!(result["data"]["applied"], false);
        if id == "symbols" {
            assert_eq!(result["data"]["warning_count"], 2);
            assert!(!result["warnings"].as_array().unwrap().is_empty());
        }
        for pack in result["data"]["pack_ids"].as_array().unwrap() {
            let result = document(
                &run(
                    profile.path(),
                    &["pack", "show", pack.as_str().unwrap(), "--json"],
                ),
                0,
            );
            assert!(
                result["data"]["provenance"]["licenses"]
                    .as_object()
                    .is_some_and(|items| !items.is_empty())
            );
        }
    }
    let state = document(&run(profile.path(), &["status", "--json"]), 0);
    assert_eq!(state["data"]["packs"].as_array().unwrap().len(), 8);
    assert_eq!(state["data"]["registration_state"], "inactive");
    assert!(
        !profile
            .path()
            .join("WindowsFileIconsDev/recovery.json")
            .exists()
    );
}

#[test]
fn target_changes_are_atomic_dry_runnable_and_shared_with_resolve() {
    let profile = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    fixture(source.path());
    let imported = document(
        &run(
            profile.path(),
            &[
                "pack",
                "import",
                source.path().to_str().unwrap(),
                "--theme",
                "one",
                "--json",
            ],
        ),
        0,
    );
    let id = imported["data"]["pack_id"].as_str().unwrap();
    let changes = source.path().join("changes.json");
    fs::write(
        &changes,
        r#"{"file:cargo.toml":false,"extension:wfi-cli-probe":false}"#,
    )
    .unwrap();
    let args = ["targets", "set", id, changes.to_str().unwrap(), "--json"];
    let before = files(profile.path());
    let mut dry = args.to_vec();
    dry.push("--dry-run");
    document(&run(profile.path(), &dry), 0);
    assert_eq!(files(profile.path()), before);
    document(
        &Command::new(env!("CARGO_BIN_EXE_file-icons"))
            .args(["targets", "set", id, "changes.json", "--json"])
            .current_dir(source.path())
            .env("LOCALAPPDATA", profile.path())
            .test_output()
            .unwrap(),
        0,
    );
    assert_eq!(
        document(
            &run(profile.path(), &["resolve", id, "Cargo.toml", "--json"]),
            0
        )["data"]["reason"],
        "windows_default"
    );
    let saved = files(profile.path());
    fs::write(
        &changes,
        r#"{"file:cargo.toml":true,"file:unknown.name":false}"#,
    )
    .unwrap();
    document(&run(profile.path(), &args), 4);
    assert_eq!(files(profile.path()), saved);
    fs::write(
        &changes,
        r#"{"file:cargo.toml":true,"file:unknown.name":null}"#,
    )
    .unwrap();
    document(&run(profile.path(), &args), 4);
    assert_eq!(files(profile.path()), saved);
    fs::write(&changes, r#"{"file:cargo.toml":null}"#).unwrap();
    document(
        &Command::new(env!("CARGO_BIN_EXE_file-icons"))
            .args(["targets", "set", id, "changes.json", "--json"])
            .current_dir(source.path())
            .env("LOCALAPPDATA", profile.path())
            .test_output()
            .unwrap(),
        0,
    );
    assert_eq!(
        document(
            &run(profile.path(), &["resolve", id, "Cargo.toml", "--json"]),
            0
        )["data"]["reason"],
        "file_name"
    );
    assert!(
        document(&run(profile.path(), &["targets", "list", id, "--json"]), 0)["data"]["targets"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["target"]["id"] == "file:cargo.toml")
    );
    let second = document(
        &run(
            profile.path(),
            &[
                "pack",
                "import",
                source.path().to_str().unwrap(),
                "--theme",
                "two",
                "--json",
            ],
        ),
        0,
    );
    let destination = second["data"]["pack_id"].as_str().unwrap();
    document(
        &run(
            profile.path(),
            &["targets", "edit", destination, "all", "on", "--json"],
        ),
        0,
    );
    let original = document(&run(profile.path(), &["targets", "list", id, "--json"]), 0);
    let saved = files(profile.path());
    let dry = document(
        &run(
            profile.path(),
            &["targets", "copy", id, destination, "--dry-run", "--json"],
        ),
        0,
    );
    assert_eq!(files(profile.path()), saved);
    let copied = document(
        &run(
            profile.path(),
            &["targets", "copy", id, destination, "--json"],
        ),
        0,
    );
    assert_eq!(copied["data"]["selection"], dry["data"]["selection"]);
    assert_eq!(
        copied["data"]["selection"]["overrides"],
        serde_json::json!({"extension:wfi-cli-probe":false})
    );
    assert_eq!(
        document(&run(profile.path(), &["targets", "list", id, "--json"]), 0),
        original
    );
    let inherited = document(
        &run(
            profile.path(),
            &[
                "targets",
                "edit",
                destination,
                "query:.WFI-CLI-PROBE",
                "inherit",
                "--json",
            ],
        ),
        0,
    );
    assert_eq!(inherited["data"]["changed_targets"], 1);
    assert_eq!(
        inherited["data"]["selection"]["overrides"],
        serde_json::json!({})
    );
}

#[test]
fn shared_categories_drive_bulk_edits_in_both_languages() {
    let profile = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    fixture(source.path());
    fs::write(source.path().join("theme.json"), r#"{"iconDefinitions":{"x":{"iconPath":"icon.svg"}},"fileNames":{"package.json":"x","tsconfig.json":"x"},"fileExtensions":{"sql":"x","lua":"x","json":"x","zip":"x","tar.gz":"x","unknown":"x"}}"#).unwrap();
    let imported = document(
        &run(
            profile.path(),
            &[
                "pack",
                "import",
                source.path().to_str().unwrap(),
                "--theme",
                "one",
                "--json",
            ],
        ),
        0,
    );
    let id = imported["data"]["pack_id"].as_str().unwrap();
    for language in ["ja", "en"] {
        let listed = document(
            &run(
                profile.path(),
                &["targets", "list", id, "--lang", language, "--json"],
            ),
            0,
        );
        let targets = listed["data"]["targets"].as_array().unwrap();
        for (target, expected) in [
            ("extension:sql", "language"),
            ("extension:lua", "language"),
            ("file:package.json", "build"),
            ("file:tsconfig.json", "data"),
            ("extension:zip", "archives"),
            ("extension:unknown", "other"),
        ] {
            let entry = targets
                .iter()
                .find(|t| t["target"]["id"] == target)
                .unwrap();
            assert_eq!(entry["target"]["category"], expected, "{target}");
        }
        document(
            &run(
                profile.path(),
                &[
                    "targets",
                    "edit",
                    id,
                    "category:archives",
                    "off",
                    "--lang",
                    language,
                    "--json",
                ],
            ),
            0,
        );
    }
    let listed = document(&run(profile.path(), &["targets", "list", id, "--json"]), 0);
    for target in listed["data"]["targets"].as_array().unwrap() {
        let expected = target["target"]["category"] != "archives";
        assert_eq!(target["enabled"], expected, "{}", target["target"]["id"]);
    }
    for (name, enabled) in [
        ("file.zip", false),
        ("file.tar.gz", false),
        ("query.sql", true),
        ("package.json", true),
    ] {
        let resolved = document(&run(profile.path(), &["resolve", id, name, "--json"]), 0);
        assert_eq!(resolved["data"]["selection_enabled"], enabled, "{name}");
        assert_eq!(resolved["data"]["icon"].is_string(), enabled, "{name}");
    }
}

#[test]
fn protected_targets_reject_explicit_enabling_and_survive_bulk_edits() {
    let profile = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    fixture(source.path());
    fs::write(source.path().join("theme.json"), r#"{"iconDefinitions":{"x":{"iconPath":"icon.svg"}},"fileNames":{"tool.exe":"x"},"fileExtensions":{"lnk":"x","exe":"x","rs":"x"}}"#).unwrap();
    let result = document(
        &run(
            profile.path(),
            &[
                "pack",
                "import",
                source.path().to_str().unwrap(),
                "--theme",
                "one",
                "--json",
            ],
        ),
        0,
    );
    let id = result["data"]["pack_id"].as_str().unwrap();
    let listed = document(&run(profile.path(), &["targets", "list", id, "--json"]), 0);
    let targets = listed["data"]["targets"].as_array().unwrap();
    assert!(targets.iter().any(|t| t["target"]["id"] == "extension:lnk"
        && t["protected"] == true
        && t["enabled"] == false));
    let before = files(profile.path());
    document(
        &run(
            profile.path(),
            &["targets", "edit", id, "extension:lnk", "on", "--json"],
        ),
        4,
    );
    let changes = source.path().join("changes.json");
    fs::write(&changes, r#"{"extension:rs":false,"file:tool.exe":true}"#).unwrap();
    document(
        &run(
            profile.path(),
            &["targets", "set", id, changes.to_str().unwrap(), "--json"],
        ),
        4,
    );
    assert_eq!(files(profile.path()), before);
    document(
        &run(
            profile.path(),
            &["targets", "edit", id, "all", "on", "--json"],
        ),
        0,
    );
    for name in ["app.LNK", "tool.EXE"] {
        let output = document(&run(profile.path(), &["resolve", id, name, "--json"]), 0);
        assert_eq!(output["data"]["reason"], "protected_file_type");
        assert_eq!(output["data"]["selection_enabled"], false);
        assert!(output["data"]["icon"].is_null());
    }
    assert!(
        document(
            &run(profile.path(), &["resolve", id, "lib.rs", "--json"]),
            0
        )["data"]["icon"]
            .is_string()
    );
}

#[test]
fn confirmed_recovery_cli_output_and_confirmation_are_noninteractive() {
    let profile = tempfile::tempdir().unwrap();
    let before = files(profile.path());
    let listed = document(
        &run(
            profile.path(),
            &["conflicts", "list", "--json", "--lang", "en"],
        ),
        0,
    );
    assert_eq!(listed["data"]["conflicts"], 0);
    let preview = document(
        &run(
            profile.path(),
            &[
                "recover",
                "--strategy",
                "preserve-external",
                "--dry-run",
                "--json",
            ],
        ),
        0,
    );
    let id = preview["data"]["plan"]["confirmation"].as_str().unwrap();
    assert_eq!(id.len(), 64);
    assert_eq!(files(profile.path()), before);
    for args in [
        vec!["recover", "--strategy", "preserve-external", "--json"],
        vec!["status", "--strategy", "preserve-external", "--json"],
        vec!["recover", "--confirm", id, "--json"],
        vec![
            "recover",
            "--strategy",
            "preserve-external",
            "--dry-run",
            "--confirm",
            id,
            "--json",
        ],
    ] {
        document(&run(profile.path(), &args), 2);
    }
    let wrong = "0".repeat(64);
    let error = document(
        &run(
            profile.path(),
            &[
                "recover",
                "--strategy",
                "preserve-external",
                "--confirm",
                &wrong,
                "--json",
            ],
        ),
        6,
    );
    assert_eq!(error["error"]["code"], "registration_conflict");
    assert_eq!(files(profile.path()), before);
    let result = document(
        &run(
            profile.path(),
            &[
                "recover",
                "--strategy",
                "preserve-external",
                "--confirm",
                id,
                "--json",
            ],
        ),
        0,
    );
    assert_eq!(result["data"]["recovered"], true);
    assert_eq!(result["data"]["plan"]["writes"], 0);
    document(&run(profile.path(), &["restore", "--json"]), 0);
    document(&run(profile.path(), &["recover", "--json"]), 0);
}
