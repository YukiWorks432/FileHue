use std::{fs, path::Path, process::Command, time::Duration};
use xtask::{package, process};

fn generate(root: &Path, output: &Path) -> process::Report {
    let logs = tempfile::tempdir_in(root).unwrap();
    process::run(
        Command::new("pwsh")
            .args(["-NoProfile", "-NonInteractive", "-File"])
            .arg(xtask::root().join("scripts/create-release-notes.ps1"))
            .arg("-Version")
            .arg("9.8.7")
            .arg("-Root")
            .arg(root)
            .arg("-OutputPath")
            .arg(output),
        Duration::from_secs(20),
        "日英リリースノートの生成",
        &logs.path().join("process"),
    )
    .unwrap()
}

#[test]
fn bilingual_notes_pin_relative_links_to_the_requested_tag_and_preserve_sources() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let notes = root.join("docs/releases");
    fs::create_dir_all(&notes).unwrap();
    let japanese =
        "# 日本語の変更\r\n\r\n[英語](9.8.7.en.md)\r\n[使い方](../usage.ja.md#update)\r\n";
    let english = "# Changes\n\n[External](https://example.test/terms)\n";
    fs::write(notes.join("9.8.7.ja.md"), japanese).unwrap();
    fs::write(notes.join("9.8.7.en.md"), english).unwrap();
    let output = root.join("出力 with spaces/notes.md");
    let result = generate(root, &output);
    assert_eq!(result.returncode, 0, "{}", process::detail(&result));
    let body = fs::read_to_string(&output).unwrap();
    assert!(body.starts_with("# 日本語の変更\n"));
    assert!(body.contains("# Changes\n"));
    assert!(body.contains(
        "[英語](https://github.com/YukiWorks432/FileHue/blob/v9.8.7/docs/releases/9.8.7.en.md)"
    ));
    assert!(body.contains(
        "[使い方](https://github.com/YukiWorks432/FileHue/blob/v9.8.7/docs/usage.ja.md#update)"
    ));
    assert!(body.contains("[External](https://example.test/terms)"));
    assert!(!body.contains('\r'));
    assert_eq!(
        fs::read_to_string(notes.join("9.8.7.ja.md")).unwrap(),
        japanese
    );
    assert_eq!(
        fs::read_to_string(notes.join("9.8.7.en.md")).unwrap(),
        english
    );
}

#[test]
fn missing_language_creates_no_body_and_existing_output_is_preserved() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let notes = root.join("docs/releases");
    fs::create_dir_all(&notes).unwrap();
    fs::write(notes.join("9.8.7.ja.md"), "日本語の変更").unwrap();
    let output = root.join("notes.md");
    assert_ne!(generate(root, &output).returncode, 0);
    assert!(!output.exists());
    fs::write(notes.join("9.8.7.en.md"), "Changes").unwrap();
    fs::write(&output, "手動で確認した本文").unwrap();
    assert_ne!(generate(root, &output).returncode, 0);
    assert_eq!(fs::read_to_string(&output).unwrap(), "手動で確認した本文");
}

#[test]
fn payload_collects_future_notes_and_keeps_historical_notes() {
    let temp = tempfile::tempdir().unwrap();
    let notes = temp.path().join("docs/releases");
    fs::create_dir_all(&notes).unwrap();
    for name in ["1.0.0.ja.md", "1.0.0.en.md", "9.8.7.ja.md", "9.8.7.en.md"] {
        fs::write(notes.join(name), "変更内容").unwrap();
    }
    let payload = package::payload_documents(temp.path()).unwrap();
    for name in ["1.0.0.ja.md", "1.0.0.en.md", "9.8.7.ja.md", "9.8.7.en.md"] {
        assert!(payload.contains(&Path::new("docs/releases").join(name)));
    }
}
