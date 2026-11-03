use serde_json::json;
use std::{fs, process::Command};
use xtask::package::{self, Purpose};
#[path = "support/package.rs"]
mod support;

#[test]
fn bilingual_notices_link_to_each_other_and_bundled_license_html() {
    let temp = tempfile::tempdir().unwrap();
    let payload = temp.path().join("payload");
    support::payload(&payload, Purpose::Validation);
    let repository = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    for name in ["THIRD_PARTY_NOTICES.md", "THIRD_PARTY_NOTICES.en.md"] {
        package::copy_distribution_notice(&repository.join(name), &payload.join(name)).unwrap();
    }
    xtask::documents::verify(&payload).unwrap();
    fs::remove_file(payload.join("licenses/THIRD_PARTY_LICENSES.html")).unwrap();
    assert!(xtask::documents::verify(&payload).is_err());
    fs::write(
        payload.join("licenses/THIRD_PARTY_LICENSES.html"),
        "fixture",
    )
    .unwrap();
    fs::remove_file(payload.join("THIRD_PARTY_NOTICES.en.md")).unwrap();
    assert!(xtask::documents::verify(&payload).is_err());
}

#[test]
fn public_payload_requires_complete_clean_unsigned_files_and_no_unresolved_rights() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("payload");
    let original = support::payload(&root, Purpose::PublicCandidate);
    package::verify(&root, temp.path(), Purpose::PublicCandidate).unwrap();
    assert!(package::verify(&root, temp.path(), Purpose::Validation).is_err());
    for (field, bad) in [
        ("purpose", json!("private-validation-only")),
        ("profile", json!("debug")),
        ("product", json!("Other product")),
        ("target", json!("aarch64-pc-windows-msvc")),
        ("source_dirty", json!(true)),
        (
            "source_repository",
            json!("https://example.invalid/private"),
        ),
        ("signature", json!("self-signed")),
        ("publication_approved", json!(true)),
        ("license_review_required", json!(["other@1.0.0"])),
    ] {
        let mut manifest = original.clone();
        manifest[field] = bad;
        fs::write(temp.path().join("manifest.json"), manifest.to_string()).unwrap();
        assert!(
            package::verify(&root, temp.path(), Purpose::PublicCandidate).is_err(),
            "{field}"
        );
    }
    // 一覧を書き換えても、必須の権利文書を落とした候補は受理しない。
    let mut manifest = original.clone();
    manifest["files"]
        .as_array_mut()
        .unwrap()
        .retain(|e| e["path"] != "LICENSE");
    fs::remove_file(root.join("LICENSE")).unwrap();
    fs::write(temp.path().join("manifest.json"), manifest.to_string()).unwrap();
    assert!(package::verify(&root, temp.path(), Purpose::PublicCandidate).is_err());
}

#[test]
fn unsigned_policy_checks_the_pe_even_when_the_manifest_hash_was_updated() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("payload");
    let mut manifest = support::payload(&root, Purpose::Validation);
    let executable = root.join("file-icons-manager.exe");
    let mut pe = fs::read(&executable).unwrap();
    pe[232..236].copy_from_slice(&328u32.to_le_bytes());
    pe[236..240].copy_from_slice(&8u32.to_le_bytes());
    pe.extend([0u8; 8]);
    fs::write(&executable, pe).unwrap();
    let entry = manifest["files"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|e| e["path"] == "file-icons-manager.exe")
        .unwrap();
    entry["size"] = json!(executable.metadata().unwrap().len());
    entry["sha256"] = json!(package::digest(&executable).unwrap());
    fs::write(temp.path().join("manifest.json"), manifest.to_string()).unwrap();
    assert!(
        package::verify(&root, temp.path(), Purpose::Validation)
            .unwrap_err()
            .to_string()
            .contains("埋め込み署名")
    );
}

#[test]
fn external_license_records_are_bound_to_embedded_originals_and_source_archive() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("payload");
    let mut manifest = support::payload(&root, Purpose::Validation);
    let notice = temp.path().join("sources/fixture/source.crate");
    fs::create_dir_all(notice.parent().unwrap()).unwrap();
    fs::write(&notice, b"Original source").unwrap();
    let html = root.join("licenses/THIRD_PARTY_LICENSES.html");
    let source = temp.path().join("collected");
    fs::create_dir_all(source.join("fixture")).unwrap();
    fs::write(source.join("fixture/NOTICE"), "Fixture copyright and terms").unwrap();
    let hash = package::digest(&source.join("fixture/NOTICE")).unwrap();
    let record = json!([{"name":"fixture","version":"1","review_required":false,
        "files":[{"path":"fixture/NOTICE","sha256":hash}],"source_archive":{"path":"sources/fixture/source.crate","url":"https://static.crates.io/crates/fixture/fixture-1.crate","sha256":package::digest(&notice).unwrap()}}]);
    let report = json!({"licenses":[]});
    fs::write(
        &html,
        xtask::notices::html(&report, record.as_array().unwrap(), &source).unwrap(),
    )
    .unwrap();
    let html_entry = manifest["files"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|item| item["path"] == "licenses/THIRD_PARTY_LICENSES.html")
        .unwrap();
    html_entry["size"] = json!(html.metadata().unwrap().len());
    html_entry["sha256"] = json!(package::digest(&html).unwrap());
    fs::write(temp.path().join("dependencies.json"), record.to_string()).unwrap();
    manifest["license_records_sha256"] =
        json!(package::digest(&temp.path().join("dependencies.json")).unwrap());
    fs::write(temp.path().join("manifest.json"), manifest.to_string()).unwrap();
    package::verify(&root, temp.path(), Purpose::Validation).unwrap();

    fs::write(temp.path().join("dependencies.json"), "[]").unwrap();
    assert!(package::verify(&root, temp.path(), Purpose::Validation).is_err());
    fs::write(temp.path().join("dependencies.json"), record.to_string()).unwrap();
    fs::write(temp.path().join("about.json"), "modified").unwrap();
    assert!(package::verify(&root, temp.path(), Purpose::Validation).is_err());
    fs::write(temp.path().join("about.json"), "{\"licenses\":[]}").unwrap();

    let original_html = fs::read(&html).unwrap();
    fs::write(
        &html,
        String::from_utf8(original_html.clone())
            .unwrap()
            .replace("Fixture copyright and terms", "Fixture altered terms"),
    )
    .unwrap();
    let entry = manifest["files"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|item| item["path"] == "licenses/THIRD_PARTY_LICENSES.html")
        .unwrap();
    entry["size"] = json!(html.metadata().unwrap().len());
    entry["sha256"] = json!(package::digest(&html).unwrap());
    fs::write(temp.path().join("manifest.json"), manifest.to_string()).unwrap();
    assert!(package::verify(&root, temp.path(), Purpose::Validation).is_err());
    fs::write(&html, original_html).unwrap();
    let entry = manifest["files"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|item| item["path"] == "licenses/THIRD_PARTY_LICENSES.html")
        .unwrap();
    entry["size"] = json!(html.metadata().unwrap().len());
    entry["sha256"] = json!(package::digest(&html).unwrap());

    fs::remove_file(&notice).unwrap();
    fs::write(temp.path().join("manifest.json"), manifest.to_string()).unwrap();
    assert!(package::verify(&root, temp.path(), Purpose::Validation).is_err());
}

#[test]
fn only_the_public_filehue_repository_and_exact_published_commit_are_accepted() {
    let head = "0123456789abcdef0123456789abcdef01234567";
    let public = json!({"full_name":"YukiWorks432/filehue","private":false});
    let commit = json!({"sha":head});
    package::verify_public_source(&public, &commit, head).unwrap();
    package::verify_public_source(
        &json!({"full_name":"YukiWorks432/FileHue","private":false}),
        &commit,
        head,
    )
    .unwrap();
    assert!(
        package::verify_public_source(
            &json!({"full_name":"YukiWorks432/filehue","private":true}),
            &commit,
            head
        )
        .is_err()
    );
    assert!(
        package::verify_public_source(
            &json!({"full_name":"someone/another","private":false}),
            &commit,
            head
        )
        .is_err()
    );
    assert!(package::verify_public_source(&public, &json!({"sha":"different"}), head).is_err());
}

#[test]
fn cli_verification_uses_the_same_purpose_and_payload_contract() {
    let temp = tempfile::tempdir().unwrap();
    let payload = temp.path().join("payload");
    support::payload(&payload, Purpose::PublicCandidate);
    let run = |public: bool| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_xtask"));
        command.args(["package", "--verify"]).arg(&payload);
        if public {
            command.arg("--public-candidate");
        }
        command.output().unwrap()
    };
    assert!(run(true).status.success());
    assert!(!run(false).status.success());
    fs::write(payload.join("file-icons.exe"), "changed after packaging").unwrap();
    assert!(!run(true).status.success());
}

#[test]
fn release_entry_rejects_wrong_version_commit_lock_and_archive_checksum_before_execution() {
    use sha2::{Digest, Sha256};
    use std::io::Write;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("source");
    fs::create_dir(&root).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[workspace.package]\nversion='0.1.0'\n",
    )
    .unwrap();
    fs::write(root.join("Cargo.lock"), b"abc").unwrap();
    fs::write(root.join(".gitignore"), "work/\n").unwrap();
    for args in [
        vec!["init", "-q"],
        vec!["add", "."],
        vec![
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-qm",
            "source",
        ],
    ] {
        package::run(&root, "git", &args, 30).unwrap();
    }
    let source = package::source_state(&root).unwrap();
    let payload = temp.path().join("payload");
    let mut original = support::payload(&payload, Purpose::Validation);
    original["source_commit"] = json!(source.head);
    original["source_files_sha256"] = json!(hex::encode(Sha256::digest(
        serde_json::to_vec(&source.files).unwrap()
    )));
    for (index, (field, bad, reason)) in [
        ("version", "0.2.0", "版・構成"),
        (
            "source_commit",
            "ffffffffffffffffffffffffffffffffffffffff",
            "source differ",
        ),
        ("cargo_lock_sha256", "wrong", "source differ"),
        ("source_files_sha256", "wrong", "source differ"),
    ]
    .into_iter()
    .enumerate()
    {
        let mut manifest = original.clone();
        manifest[field] = json!(bad);
        fs::write(temp.path().join("manifest.json"), manifest.to_string()).unwrap();
        let archive = temp.path().join(format!("candidate-{index}.zip"));
        let mut zip = zip::ZipWriter::new(fs::File::create(&archive).unwrap());
        for path in package::files(&payload).unwrap() {
            zip.start_file(
                format!("candidate/{}", path.to_string_lossy().replace('\\', "/")),
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
            zip.write_all(&fs::read(payload.join(path)).unwrap())
                .unwrap();
        }
        zip.finish().unwrap();
        fs::write(
            temp.path().join("SHA256SUMS.txt"),
            format!(
                "{}  candidate-{index}.zip\n{}  manifest.json\n",
                package::digest(&archive).unwrap(),
                package::digest(&temp.path().join("manifest.json")).unwrap()
            ),
        )
        .unwrap();
        let report = temp.path().join(format!("report-{index}.json"));
        let error =
            xtask::release::verify(&root, &archive, &report, Purpose::Validation).unwrap_err();
        assert!(error.to_string().contains(reason), "{field}: {error:#}");
        assert!(!report.exists());
        fs::write(temp.path().join("SHA256SUMS.txt"), "changed checksum").unwrap();
        assert!(
            xtask::release::verify(&root, &archive, &report, Purpose::Validation)
                .unwrap_err()
                .to_string()
                .contains("SHA256SUMS")
        );
    }
    // 非公開の開発チェックアウトでは、ビルド開始前に公開候補の作成を拒否する。
    package::run(
        &root,
        "git",
        &["remote", "add", "origin", "https://example.invalid/private"],
        30,
    )
    .unwrap();
    assert!(
        package::package(&root, "release", false, Purpose::PublicCandidate)
            .unwrap_err()
            .to_string()
            .contains("新しいfilehue")
    );
}
