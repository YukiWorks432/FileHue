use crate::{package, process};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use sha2::Digest;
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

/// 全項目を検査してから展開する。展開先は呼び出し元が作った専用領域に限る。
pub fn extract(archive: &Path, destination: &Path) -> Result<PathBuf> {
    ensure!(
        !destination.exists(),
        "展開先は新しい専用領域を指定してください"
    );
    let mut zip = zip::ZipArchive::new(File::open(archive)?)?;
    ensure!(
        !zip.is_empty() && zip.len() <= 5000,
        "release archive limits exceeded"
    );
    let mut roots = BTreeSet::new();
    let mut names = BTreeSet::new();
    let mut total = 0u64;
    for i in 0..zip.len() {
        let entry = zip.by_index(i)?;
        let name = entry.name()?.into_owned();
        let path = package::relative(name.trim_end_matches('/'))?;
        ensure!(
            names.insert(name.trim_end_matches('/').to_uppercase()),
            "duplicate release archive path"
        );
        ensure!(
            !entry.encrypted()
                && matches!(
                    entry.compression(),
                    zip::CompressionMethod::Stored | zip::CompressionMethod::Deflated
                ),
            "unsupported release archive entry"
        );
        if let Some(mode) = entry.unix_mode() {
            let kind = mode & 0o170000;
            ensure!(
                kind == 0 || kind == 0o100000 || kind == 0o040000,
                "release archive contains a link or special file"
            );
        }
        total = total
            .checked_add(entry.size())
            .context("release archive size overflow")?;
        ensure!(
            total <= 512 * 1024 * 1024,
            "release archive limits exceeded"
        );
        roots.insert(path.components().next().unwrap().as_os_str().to_owned());
    }
    ensure!(roots.len() == 1, "release archive must have one root");
    fs::create_dir_all(destination)?;
    let mut written = 0u64;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let name = entry.name()?.into_owned();
        let target = destination.join(package::relative(name.trim_end_matches('/'))?);
        if entry.is_dir() {
            fs::create_dir_all(&target)?;
            continue;
        }
        fs::create_dir_all(target.parent().unwrap())?;
        let declared = entry.size();
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)?;
        let bytes = std::io::copy(&mut Read::by_ref(&mut entry).take(declared + 1), &mut file)?;
        written = written.checked_add(bytes).context("展開量が大きすぎます")?;
        ensure!(
            bytes == declared && written <= 512 * 1024 * 1024,
            "release archive limits exceeded"
        );
    }
    Ok(destination.join(roots.into_iter().next().unwrap()))
}

pub fn probe(messages: &str) -> Result<PathBuf> {
    probe_for(messages, "isolated_shell")
}

pub fn probe_for(messages: &str, target: &str) -> Result<PathBuf> {
    let mut paths = Vec::new();
    for line in messages.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if value["reason"] == "compiler-artifact"
            && value["target"]["name"] == target
            && let Some(path) = value["executable"].as_str()
        {
            paths.push(PathBuf::from(path));
        }
    }
    ensure!(
        paths.len() == 1,
        "cannot identify isolated Shell probe: {target}"
    );
    Ok(paths.remove(0))
}

pub fn smoke(executable: &Path, profile: &Path, logs: &Path) -> Result<()> {
    fs::create_dir_all(profile)?;
    process::checked(
        Command::new(executable)
            .arg("--smoke-test")
            .env("LOCALAPPDATA", profile),
        Duration::from_secs(30),
        "GUI起動確認",
        logs,
    )?;
    println!("GUI起動・ウィンドウ初期化・自動終了: 成功（描画と操作の受け入れは別途必要）");
    Ok(())
}

pub fn verify(
    root: &Path,
    archive: &Path,
    report: &Path,
    purpose: package::Purpose,
    verify_shell: bool,
) -> Result<Value> {
    let source = package::source_state(root)?;
    ensure!(!source.dirty, "commit source before validating release");
    fs::create_dir_all(report.parent().context("報告書の親がありません")?)?;
    ensure!(!report.exists(), "preserve the existing validation report");
    let logs = tempfile::Builder::new()
        .prefix("release-logs-")
        .tempdir_in(report.parent().unwrap())?
        .keep();
    let work = tempfile::Builder::new()
        .prefix("release-validation-")
        .tempdir_in(root.join("work"))?;
    let archive_hash = package::digest(archive)?;
    let archive_name = archive
        .file_name()
        .and_then(|n| n.to_str())
        .context("ZIP名が不正です")?;
    let sums = archive.parent().unwrap().join("SHA256SUMS.txt");
    let metadata = archive.parent().unwrap();
    let manifest_hash = package::digest(&metadata.join("manifest.json"))?;
    ensure!(
        fs::read_to_string(&sums)?
            == format!("{archive_hash}  {archive_name}\n{manifest_hash}  manifest.json\n"),
        "SHA256SUMS.txtとZIP・manifestが一致しません"
    );
    let folder = extract(archive, &work.path().join("unpacked"))?;
    let manifest = package::verify(&folder, metadata, purpose)?;
    let links = crate::documents::verify(&folder)?;
    ensure!(
        manifest.source_commit == source.head
            && !manifest.source_dirty
            && manifest.source_files_sha256
                == hex::encode(sha2::Sha256::digest(serde_json::to_vec(&source.files)?))
            && manifest.cargo_lock_sha256 == package::digest(&root.join("Cargo.lock"))?,
        "release and probe source differ"
    );
    let config: toml::Value = toml::from_str(&fs::read_to_string(root.join("Cargo.toml"))?)?;
    ensure!(
        Some(manifest.version.as_str()) == config["workspace"]["package"]["version"].as_str()
            && manifest.profile == "release",
        "配布物の版・構成がソースと一致しません"
    );
    if purpose == package::Purpose::PublicCandidate {
        package::public_source(root, &source)?;
    }
    let sysroot = package::run(root, "rustc", &["--print", "sysroot"], 30)?;
    crate::privacy::verify(&folder, root, Path::new(&sysroot))?;
    let (probe_sha256, thumbnail_probe_sha256) = if verify_shell {
        let messages = process::checked(
            Command::new("cargo")
                .args([
                    "test",
                    "-p",
                    "file-icons-shell",
                    "--test",
                    "isolated_shell",
                    "--features",
                    "shell-integration",
                    "--no-run",
                    "--locked",
                    "--message-format=json",
                ])
                .current_dir(root),
            Duration::from_secs(1800),
            "配布検証の試験ビルド",
            &logs.join("build"),
        )?;
        let probe = probe(std::str::from_utf8(&messages)?)?;
        let copied = work.path().join("probe");
        fs::create_dir(&copied)?;
        fs::copy(&probe, copied.join("isolated_shell.exe"))?;
        fs::copy(
            folder.join("file_icons_shell.dll"),
            copied.join("file_icons_shell.dll"),
        )?;
        process::checked(
            Command::new(copied.join("isolated_shell.exe"))
                .args([
                    "--exact",
                    "isolated_shell_registration_probe",
                    "--nocapture",
                ])
                .current_dir(root),
            Duration::from_secs(420),
            "配布DLLのSTA/MTA",
            &logs.join("shell"),
        )?;
        let thumbnail_messages = process::checked(
            Command::new("cargo")
                .args([
                    "test",
                    "-p",
                    "file-icons-thumbnail-shell",
                    "--test",
                    "pdf",
                    "--no-run",
                    "--locked",
                    "--message-format=json",
                ])
                .current_dir(root),
            Duration::from_secs(1800),
            "配布検証のサムネイル試験ビルド",
            &logs.join("thumbnail-build"),
        )?;
        let thumbnail_probe = probe_for(std::str::from_utf8(&thumbnail_messages)?, "pdf")?;
        fs::copy(&thumbnail_probe, copied.join("thumbnail_pdf.exe"))?;
        fs::copy(
            folder.join("file_icons_thumbnail_shell.dll"),
            copied.join("file_icons_thumbnail_shell.dll"),
        )?;
        process::checked(
            Command::new(copied.join("thumbnail_pdf.exe"))
                .args([
                    "--exact",
                    "first_page_is_rendered_in_sta_and_mta",
                    "--nocapture",
                ])
                .current_dir(root),
            Duration::from_secs(180),
            "配布サムネイルDLLのSTA/MTA",
            &logs.join("thumbnail-shell"),
        )?;
        (
            Some(package::digest(&probe)?),
            Some(package::digest(&thumbnail_probe)?),
        )
    } else {
        (None, None)
    };
    smoke(
        &folder.join("file-icons-manager.exe"),
        &logs.join("smoke/profile"),
        &logs.join("gui"),
    )?;
    for (i, args) in [
        vec!["--help", "--json"],
        vec!["catalog", "list", "--json"],
        vec!["status", "--json"],
        vec!["--version", "--json"],
    ]
    .iter()
    .enumerate()
    {
        let bytes = process::checked(
            Command::new(folder.join("file-icons.exe"))
                .args(args)
                .env("LOCALAPPDATA", work.path().join("cli-profile"))
                .current_dir(root),
            Duration::from_secs(30),
            "配布CLI",
            &logs.join(format!("cli-{i}")),
        )?;
        let response: Value = serde_json::from_slice(&bytes)?;
        ensure!(
            response["schema_version"] == 1 && response["ok"] == true,
            "bundled CLI contract failed"
        );
        if args[0] == "--version" {
            ensure!(
                response["data"]["version"] == manifest.version,
                "同梱CLIの版が一致しません"
            );
        }
    }
    package::assert_source(root, &source)?;
    ensure!(
        package::digest(archive)? == archive_hash
            && package::digest(&metadata.join("manifest.json"))? == manifest_hash
            && package::digest(&metadata.join("dependencies.json"))?
                == manifest.license_records_sha256
            && package::digest(&metadata.join("about.json"))? == manifest.about_sha256,
        "検証中にZIPまたは検証記録が変更されました"
    );
    let mut result = json!({"product":package::PRODUCT,"purpose":purpose,"source_commit":source.head,"version":manifest.version,"target":manifest.target,
        "source_repository":manifest.source_repository,"signature":"unsigned","archive":archive_name,"archive_sha256":archive_hash,"files":manifest.files.len(),
        "dll_sha256":package::digest(&folder.join("file_icons_shell.dll"))?,"thumbnail_dll_sha256":package::digest(&folder.join("file_icons_thumbnail_shell.dll"))?,"gui_sha256":package::digest(&folder.join("file-icons-manager.exe"))?,"probe_sha256":probe_sha256,"thumbnail_probe_sha256":thumbnail_probe_sha256,
        "manifest":"passed","document_links":"passed","document_links_checked":links,"shell_sta_mta":if verify_shell { "passed" } else { "not-run" },"thumbnail_sta_mta":if verify_shell { "passed" } else { "not-run" },"gui_startup":"passed","cli_smoke":"passed","cli_sha256":package::digest(&folder.join("file-icons.exe"))?,"actual_user_registration_modified":false,
        "license_review_required":manifest.license_review_required,"publication_approved":false});
    // 生ログ・ローカルパスは検証者だけが保持し、公開候補の報告には含めない。
    if purpose == package::Purpose::Validation {
        result["process_logs"] = json!(logs);
        result["archive"] = json!(archive);
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(report)?;
    file.write_all(&serde_json::to_vec_pretty(&result)?)?;
    file.write_all(b"\n")?;
    Ok(result)
}
