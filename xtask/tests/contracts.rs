use serde_json::{Value, json};
use std::{
    fs::{self, File},
    io::Write,
    path::Path,
};
use xtask::{documents, package, release};
#[path = "support/package.rs"]
mod public_package;

fn archive(path: &Path, names: &[&str]) {
    let mut zip = zip::ZipWriter::new(File::create(path).unwrap());
    for name in names {
        zip.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"abc").unwrap();
    }
    zip.finish().unwrap();
}

#[test]
fn final_move_preserves_existing_files_and_directories() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let source = root.join("new.zip");
    let target = root.join("existing.zip");
    fs::write(&source, b"new package").unwrap();
    fs::write(&target, b"existing package").unwrap();
    assert!(package::move_new(&source, &target).is_err());
    assert_eq!(fs::read(&target).unwrap(), b"existing package");
    assert_eq!(fs::read(&source).unwrap(), b"new package");
    let stage = root.join("stage");
    fs::create_dir(&stage).unwrap();
    fs::write(stage.join("payload"), b"staged payload").unwrap();
    let destination = root.join("existing-directory");
    fs::create_dir(&destination).unwrap();
    assert!(package::move_new(&stage, &destination).is_err());
    assert!(fs::read_dir(&destination).unwrap().next().is_none());
    assert_eq!(fs::read(stage.join("payload")).unwrap(), b"staged payload");
    fs::write(destination.join("saved"), b"saved payload").unwrap();
    assert!(package::move_new(&stage, &destination).is_err());
    assert_eq!(
        fs::read(destination.join("saved")).unwrap(),
        b"saved payload"
    );
    let fresh = root.join("fresh");
    package::move_new(&stage, &fresh).unwrap();
    assert_eq!(fs::read(fresh.join("payload")).unwrap(), b"staged payload");
    assert!(!stage.exists());
}
fn metadata(root: &Path) -> Value {
    let package = |name: &str, external: bool| json!({"id":name,"name":name,"version":"1","source":if external {Some("registry")}else{None},"manifest_path":root.join(name).join("Cargo.toml"),"license":"MIT","authors":["Fixture Author"],"repository":"https://example.invalid/fixture"});
    let dep = |name: &str, kind: Value| json!({"pkg":name,"dep_kinds":[{"kind":kind}]});
    json!({"packages":[package("file-icons-manager",false),package("file-icons-cli",false),package("file-icons-thumbnail-shell",false),package("library",true),package("build-helper",true),package("transitive",true),package("test-only",true),package("cli-library",true),package("thumbnail-runtime",true)],
    "resolve":{"nodes":[{"id":"file-icons-manager","deps":[dep("library",Value::Null),dep("build-helper",json!("build")),dep("test-only",json!("dev"))]},
    {"id":"file-icons-cli","deps":[dep("cli-library",Value::Null)]},{"id":"file-icons-thumbnail-shell","deps":[dep("thumbnail-runtime",Value::Null)]},{"id":"library","deps":[dep("transitive",Value::Null)]},{"id":"cli-library","deps":[]},{"id":"thumbnail-runtime","deps":[]},{"id":"build-helper","deps":[]},{"id":"transitive","deps":[]},{"id":"test-only","deps":[]}]}})
}
#[test]
fn dependency_closure_keeps_runtime_build_and_transitive_dependencies_but_not_dev() {
    let metadata = metadata(Path::new("unused"));
    let names: std::collections::BTreeSet<_> = package::dependencies(&metadata)
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        std::collections::BTreeSet::from([
            "library",
            "build-helper",
            "transitive",
            "cli-library",
            "thumbnail-runtime"
        ])
    );
}
#[test]
fn manifest_checks_known_hash_and_rejects_modified_extra_missing_and_unsafe_files() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("payload-dir");
    public_package::payload(&path, package::Purpose::Validation);
    fs::write(path.join("payload"), b"abc").unwrap();
    let mut manifest: Value =
        serde_json::from_slice(&fs::read(root.path().join("manifest.json")).unwrap()).unwrap();
    manifest["files"].as_array_mut().unwrap().push(json!({"path":"payload","size":3,"sha256":"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"}));
    fs::write(root.path().join("manifest.json"), manifest.to_string()).unwrap();
    package::verify(&path, root.path(), package::Purpose::Validation).unwrap();
    fs::write(path.join("payload"), b"abd").unwrap();
    assert!(package::verify(&path, root.path(), package::Purpose::Validation).is_err());
    fs::write(path.join("payload"), b"abc").unwrap();
    fs::write(path.join("unexpected"), b"keep out").unwrap();
    assert!(package::verify(&path, root.path(), package::Purpose::Validation).is_err());
    fs::remove_file(path.join("unexpected")).unwrap();
    fs::remove_file(path.join("payload")).unwrap();
    assert!(package::verify(&path, root.path(), package::Purpose::Validation).is_err());
    for name in [
        "../outside",
        "/absolute",
        "C:/outside",
        "one\\two",
        "one/../two",
        "nul.txt",
        "one.",
    ] {
        assert!(package::relative(name).is_err(), "{name}");
    }
}

#[test]
fn package_requires_thumbnail_dll_even_if_manifest_omits_it() {
    let root = tempfile::tempdir().unwrap();
    let payload = root.path().join("payload-dir");
    let mut manifest = public_package::payload(&payload, package::Purpose::Validation);
    package::verify(&payload, root.path(), package::Purpose::Validation).unwrap();
    fs::remove_file(payload.join("file_icons_thumbnail_shell.dll")).unwrap();
    manifest["files"]
        .as_array_mut()
        .unwrap()
        .retain(|entry| entry["path"] != "file_icons_thumbnail_shell.dll");
    fs::write(
        root.path().join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    let error = package::verify(&payload, root.path(), package::Purpose::Validation).unwrap_err();
    assert!(error.to_string().contains("file_icons_thumbnail_shell.dll"));
}
#[test]
fn source_changes_are_detected_even_when_dirty_status_is_unchanged() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path();
    fs::write(path.join("payload"), "first").unwrap();
    fs::write(path.join(" leading name"), "tracked whitespace").unwrap();
    for args in [
        vec!["init", "-q"],
        vec!["add", "payload", " leading name"],
        vec![
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-qm",
            "fixture",
        ],
    ] {
        package::run(path, "git", &args, 30).unwrap();
    }
    fs::write(path.join("payload"), "dirty one").unwrap();
    let before = package::source_state(path).unwrap();
    assert!(before.files.contains_key(" leading name"));
    package::assert_source(path, &before).unwrap();
    fs::write(path.join("payload"), "dirty two").unwrap();
    let after = package::source_state(path).unwrap();
    assert_eq!(before.dirty, after.dirty);
    assert!(package::assert_source(path, &before).is_err());
    fs::write(path.join("payload"), "dirty one").unwrap();
    package::assert_source(path, &before).unwrap();
    package::run(
        path,
        "git",
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-qam",
            "new",
        ],
        30,
    )
    .unwrap();
    assert!(package::assert_source(path, &before).is_err());
}
#[test]
fn missing_license_retains_original_metadata_and_never_invents_permission_text() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path();
    let mut metadata = metadata(path);
    metadata["packages"]
        .as_array_mut()
        .unwrap()
        .retain(|p| ["file-icons-manager", "library"].contains(&p["name"].as_str().unwrap()));
    metadata["resolve"]["nodes"] = json!([{"id":"file-icons-manager","deps":[{"pkg":"library","dep_kinds":[{"kind":null}]}]},{"id":"library","deps":[]}]);
    fs::create_dir_all(path.join("licenses/upstream")).unwrap();
    fs::write(path.join("licenses/upstream/manifest.json"), "[]").unwrap();
    fs::write(path.join("Cargo.lock"), "version = 4\npackage=[]").unwrap();
    fs::create_dir(path.join("library")).unwrap();
    fs::write(
        path.join("library/Cargo.toml"),
        "[package]\nname='library'\nlicense='MIT'",
    )
    .unwrap();
    fs::write(path.join("library/README.md"), "Upstream summary only.").unwrap();
    let result = package::licenses(path, &metadata, &path.join("notices")).unwrap();
    assert_eq!(result[0]["review_required"], true);
    assert_eq!(
        fs::read_to_string(path.join("notices/library-1/README.md")).unwrap(),
        "Upstream summary only."
    );
    assert!(!path.join("notices/library-1/LICENSE").exists());
}
#[test]
fn markdown_links_resolve_after_extraction_and_code_examples_are_not_links() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path();
    let file = path.join("test.zip");
    let mut zip = zip::ZipWriter::new(File::create(&file).unwrap());
    for (name, body) in [
        (
            "release/docs/usage.md",
            "[復旧](recovery.md#restore)\n[表示名](<../data/file name.json>)\n[同じ節](#local)\n[Web](https://example.com/missing)\n[参照][details]\n\n[details]: recovery.md\n```markdown\n[コード例](absent-example.md)\n```\n`[inline](absent-inline.md)`\n",
        ),
        ("release/docs/recovery.md", "# Restore"),
        ("release/data/file name.json", "{}"),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(body.as_bytes()).unwrap();
    }
    zip.finish().unwrap();
    let folder = release::extract(&file, &path.join("unpacked")).unwrap();
    assert_eq!(documents::verify(&folder).unwrap(), 3);
    fs::remove_file(folder.join("docs/recovery.md")).unwrap();
    assert!(documents::verify(&folder).is_err());
}
#[test]
fn document_links_must_stay_within_the_payload_and_target_files() {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("release");
    fs::create_dir_all(folder.join("docs")).unwrap();
    fs::write(root.path().join("outside.md"), "outside").unwrap();
    for target in ["../outside.md", "%2e%2e/outside.md", "docs"] {
        fs::write(folder.join("README.md"), format!("[案内]({target})")).unwrap();
        assert!(documents::verify(&folder).is_err(), "{target}");
    }
}
#[test]
fn archives_reject_traversal_mixed_roots_duplicates_links_and_resource_overflow() {
    let root = tempfile::tempdir().unwrap();
    for (index, names) in [
        vec!["../outside"],
        vec!["one/a", "two/b"],
        vec!["one/a", "one/A"],
        vec!["release/dir/../../outside"],
        vec!["release/NUL"],
    ]
    .iter()
    .enumerate()
    {
        let file = root.path().join(format!("{index}.zip"));
        archive(&file, names);
        assert!(release::extract(&file, &root.path().join(format!("out-{index}"))).is_err());
        assert!(!root.path().join("outside").exists());
    }
    let file = root.path().join("symlink.zip");
    let mut zip = zip::ZipWriter::new(File::create(&file).unwrap());
    zip.add_symlink(
        "release/link",
        "../../outside",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    zip.finish().unwrap();
    assert!(release::extract(&file, &root.path().join("symlink-out")).is_err());
    let file = root.path().join("too-many.zip");
    let mut zip = zip::ZipWriter::new(File::create(&file).unwrap());
    for i in 0..5001 {
        zip.start_file(
            format!("release/{i}"),
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
    }
    zip.finish().unwrap();
    assert!(release::extract(&file, &root.path().join("too-many-out")).is_err());
    let file = root.path().join("too-large.zip");
    archive(&file, &["release/payload"]);
    let mut bytes = fs::read(&file).unwrap();
    let central = bytes.windows(4).position(|s| s == b"PK\x01\x02").unwrap();
    bytes[central + 24..central + 28].copy_from_slice(&(512u32 * 1024 * 1024 + 1).to_le_bytes());
    fs::write(&file, bytes).unwrap();
    assert!(release::extract(&file, &root.path().join("too-large-out")).is_err());
}
#[test]
fn valid_archive_and_compiler_messages_choose_exact_probe() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("valid.zip");
    archive(&file, &["release/payload"]);
    let folder = release::extract(&file, &root.path().join("out")).unwrap();
    assert_eq!(fs::read(folder.join("payload")).unwrap(), b"abc");
    assert_eq!(release::probe("{\"reason\":\"compiler-artifact\",\"target\":{\"name\":\"isolated_shell\"},\"executable\":\"probe.exe\"}").unwrap(),Path::new("probe.exe"));
    assert!(release::probe("{\"reason\":\"build-finished\",\"success\":true}").is_err());
    let messages = "{\"reason\":\"compiler-artifact\",\"target\":{\"name\":\"isolated_shell\"},\"executable\":\"icon.exe\"}\n{\"reason\":\"compiler-artifact\",\"target\":{\"name\":\"pdf\"},\"executable\":\"thumbnail.exe\"}";
    assert_eq!(
        release::probe_for(messages, "pdf").unwrap(),
        Path::new("thumbnail.exe")
    );
    assert!(release::probe_for(messages, "unknown").is_err());
}
