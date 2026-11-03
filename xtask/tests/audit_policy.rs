//! 更新候補をコンパイルせずに行う例外検証と、cargo-denyへの設定の受け渡しを確認する。
use serde_json::{Value, json};
use std::{fs, path::PathBuf, process::Command, time::Duration};
use xtask::process;

struct Fixture {
    directory: tempfile::TempDir,
    exceptions: Value,
    packages: Value,
}

impl Fixture {
    fn new() -> Self {
        Self {
            directory: tempfile::tempdir().unwrap(),
            exceptions: json!([{
                "id": "RUSTSEC-2026-0192", "package": "example@1.0.0",
                "reason": "外部入力の確認結果", "path": "application -> example",
                "review_by": "9999-12-31"
            }]),
            packages: json!([{"name": "example", "version": "1.0.0"}]),
        }
    }

    fn output(&self) -> PathBuf {
        self.directory.path().join("監査設定 with spaces.toml")
    }

    fn prepare(&self) -> process::Report {
        let root = self.directory.path();
        fs::write(root.join("exceptions.json"), self.exceptions.to_string()).unwrap();
        fs::write(
            root.join("metadata.json"),
            json!({"packages": self.packages}).to_string(),
        )
        .unwrap();
        fs::write(
            root.join("deny.toml"),
            "[advisories]\nyanked = 'deny'\n[bans]\nmultiple-versions = 'warn'\n",
        )
        .unwrap();
        process::run(
            Command::new("pwsh")
                .args(["-NoProfile", "-NonInteractive", "-File"])
                .arg(xtask::root().join("scripts/prepare-dependency-audit.ps1"))
                .arg("-ExceptionsPath")
                .arg(root.join("exceptions.json"))
                .arg("-ConfigPath")
                .arg(root.join("deny.toml"))
                .arg("-MetadataPath")
                .arg(root.join("metadata.json"))
                .arg("-OutputPath")
                .arg(self.output()),
            Duration::from_secs(20),
            "監査例外の検証",
            &root.join("logs"),
        )
        .unwrap()
    }

    fn rejected(&self) {
        let result = self.prepare();
        assert!(!result.timed_out, "{}", process::detail(&result));
        assert_ne!(result.returncode, 0, "{}", process::detail(&result));
        assert!(!self.output().exists(), "拒否した例外の設定を公開しました");
    }
}

#[test]
fn exceptions_reach_cargo_deny_without_losing_reasons_or_standard_settings() {
    let mut fixture = Fixture::new();
    let reason = "引用符\"・改行\n・パス C:\\data・絵文字🛡を含む確認結果";
    fixture.exceptions[0]["reason"] = json!(reason);
    let result = fixture.prepare();
    assert_eq!(result.returncode, 0, "{}", process::detail(&result));
    let config: toml::Value =
        toml::from_str(&fs::read_to_string(fixture.output()).unwrap()).unwrap();
    assert_eq!(config["advisories"]["yanked"].as_str(), Some("deny"));
    assert_eq!(config["bans"]["multiple-versions"].as_str(), Some("warn"));
    let ignores = config["advisories"]["ignore"].as_array().unwrap();
    assert_eq!(ignores.len(), 1);
    assert_eq!(ignores[0]["id"].as_str(), Some("RUSTSEC-2026-0192"));
    assert_eq!(ignores[0]["reason"].as_str(), Some(reason));
    assert_eq!(ignores[0].as_table().unwrap().len(), 2);
}

#[test]
fn expired_invalid_and_duplicate_exceptions_never_publish_configuration() {
    let logs = tempfile::tempdir().unwrap();
    let today = process::checked(
        Command::new("pwsh").args([
            "-NoProfile",
            "-Command",
            "[DateTime]::UtcNow.ToString('yyyy-MM-dd')",
        ]),
        Duration::from_secs(20),
        "UTCの日付",
        logs.path(),
    )
    .unwrap();
    let today = String::from_utf8(today).unwrap();
    for (field, value) in [
        ("review_by", today.trim()),
        ("review_by", "2000-01-01"),
        ("review_by", "2026-02-30"),
        ("id", "not-an-advisory"),
        ("reason", " "),
        ("path", ""),
        ("package", "example"),
    ] {
        let mut fixture = Fixture::new();
        fixture.exceptions[0][field] = json!(value);
        fixture.rejected();
    }
    let mut fixture = Fixture::new();
    fixture.exceptions = json!([fixture.exceptions[0], fixture.exceptions[0]]);
    fixture.rejected();
}

#[test]
fn unapproved_coexisting_replacement_and_missing_versions_are_rejected() {
    for packages in [
        json!([{"name": "example", "version": "1.0.0"}, {"name": "example", "version": "0.9.0"}]),
        json!([{"name": "example", "version": "0.9.0"}]),
        json!([]),
    ] {
        let mut fixture = Fixture::new();
        fixture.packages = packages;
        fixture.rejected();
    }
    let mut fixture = Fixture::new();
    fixture.packages =
        json!([{"name": "example", "version": "1.0.0"}, {"name": "other", "version": "2.0.0"}]);
    let result = fixture.prepare();
    assert_eq!(result.returncode, 0, "{}", process::detail(&result));
}

#[test]
fn existing_output_is_preserved_and_reported_as_failure() {
    let fixture = Fixture::new();
    fs::write(fixture.output(), "利用者のファイル").unwrap();
    let result = fixture.prepare();
    assert_ne!(result.returncode, 0, "{}", process::detail(&result));
    assert_eq!(
        fs::read_to_string(fixture.output()).unwrap(),
        "利用者のファイル"
    );
}

#[test]
fn removing_all_exceptions_keeps_standard_checks_enabled() {
    let mut fixture = Fixture::new();
    fixture.exceptions = json!([]);
    let result = fixture.prepare();
    assert_eq!(result.returncode, 0, "{}", process::detail(&result));
    let config: toml::Value =
        toml::from_str(&fs::read_to_string(fixture.output()).unwrap()).unwrap();
    assert_eq!(config["advisories"]["yanked"].as_str(), Some("deny"));
    assert!(config["advisories"].get("ignore").is_none());
}
