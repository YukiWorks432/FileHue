use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{Read, Seek, SeekFrom, Write},
    os::windows::fs::MetadataExt,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

pub const TARGET: &str = "x86_64-pc-windows-msvc";
pub const PRODUCT: &str = "FileHue";
pub const REPOSITORY: &str = "https://github.com/YukiWorks432/filehue";

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Purpose {
    #[serde(rename = "private-validation-only")]
    Validation,
    #[serde(rename = "public-candidate")]
    PublicCandidate,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Entry {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub format: u32,
    pub product: String,
    pub purpose: Purpose,
    pub version: String,
    pub target: String,
    pub profile: String,
    pub signature: String,
    pub source_repository: Option<String>,
    pub source_commit: String,
    pub source_dirty: bool,
    pub source_files_sha256: String,
    pub cargo_lock_sha256: String,
    pub license_records_sha256: String,
    pub about_sha256: String,
    pub rustc: String,
    pub license_review_required: Vec<String>,
    pub publication_approved: bool,
    pub files: Vec<Entry>,
}

#[derive(Debug, Serialize)]
pub struct Artifacts {
    pub folder: PathBuf,
    pub archive: PathBuf,
    pub checksums: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
struct LicenseAcceptance {
    decided_on: String,
    decided_by: String,
    reason: String,
    crate_checksum: String,
    supplements_sha256: String,
}

#[derive(Deserialize)]
struct LicenseRecord {
    name: String,
    version: String,
    review_required: bool,
    source_archive: Option<LicenseFile>,
}

#[derive(Deserialize)]
struct LicenseFile {
    path: String,
    sha256: String,
}
pub const PAYLOAD: &[&str] = &[
    "LICENSE",
    "THIRD_PARTY_NOTICES.md",
    "THIRD_PARTY_NOTICES.en.md",
    "docs/usage.ja.md",
    "docs/usage.en.md",
    "docs/releases/0.1.0.ja.md",
    "docs/releases/0.1.0.en.md",
    "docs/cli.ja.md",
    "docs/cli.en.md",
    "docs/target-selection.ja.md",
    "docs/conflict-recovery.ja.md",
    "docs/pack-downloads.ja.md",
];
pub const BINARIES: &[&str] = &[
    "file-icons-manager.exe",
    "file-icons.exe",
    "file_icons_shell.dll",
];

pub fn digest(path: &Path) -> Result<String> {
    let mut hash = Sha256::new();
    let mut file = File::open(path)?;
    let mut buf = [0; 65536];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
    }
    Ok(hex::encode(hash.finalize()))
}

/// 再解析ポイントをたどらず、管理対象の通常ファイルだけを列挙する。
pub fn files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in walkdir::WalkDir::new(root).min_depth(1).follow_links(false) {
        let entry = entry?;
        let metadata = fs::symlink_metadata(entry.path())?;
        ensure!(
            metadata.file_attributes() & 0x400 == 0,
            "リンク・再解析ポイントは対象外です: {}",
            entry.path().display()
        );
        if metadata.is_file() {
            paths.push(entry.path().strip_prefix(root)?.to_path_buf())
        } else {
            ensure!(metadata.is_dir(), "通常ファイルではありません")
        }
    }
    paths.sort();
    Ok(paths)
}
pub fn relative(name: &str) -> Result<PathBuf> {
    ensure!(
        !name.is_empty() && !name.starts_with('/') && !name.contains(['\\', ':', '\0']),
        "unsafe manifest path: {name}"
    );
    for part in name.split('/') {
        ensure!(
            !part.is_empty()
                && part != "."
                && part != ".."
                && !part.ends_with([' ', '.'])
                && !part.chars().any(|c| c < ' '),
            "unsafe manifest path: {name}"
        );
        let stem = part.split('.').next().unwrap().to_uppercase();
        ensure!(
            !["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
                && !(stem.len() == 4
                    && (stem.starts_with("COM") || stem.starts_with("LPT"))
                    && matches!(stem.as_bytes()[3], b'1'..=b'9')),
            "予約名は使用できません: {name}"
        );
    }
    Ok(name.split('/').collect())
}
fn slash(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
fn output(root: &Path, program: &str, args: &[&str], timeout: u64) -> Result<Vec<u8>> {
    fs::create_dir_all(root.join("work/processes"))?;
    let logs = tempfile::Builder::new()
        .prefix("xtask-")
        .tempdir_in(root.join("work/processes"))?
        .keep();
    crate::process::checked(
        Command::new(program).args(args).current_dir(root),
        Duration::from_secs(timeout),
        program,
        &logs,
    )
}
pub fn run(root: &Path, program: &str, args: &[&str], timeout: u64) -> Result<String> {
    Ok(String::from_utf8(output(root, program, args, timeout)?)?
        .trim()
        .to_owned())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Source {
    pub head: String,
    pub dirty: bool,
    pub files: BTreeMap<String, String>,
}
pub fn source_state(root: &Path) -> Result<Source> {
    let head = run(root, "git", &["rev-parse", "HEAD"], 30)?;
    let dirty = !run(root, "git", &["status", "--porcelain"], 30)?.is_empty();
    let names = String::from_utf8(output(root, "git", &["ls-files", "-z"], 30)?)?;
    let mut files = BTreeMap::new();
    for name in names.split('\0').filter(|s| !s.is_empty()) {
        files.insert(name.to_owned(), digest(&root.join(relative(name)?))?);
    }
    Ok(Source { head, dirty, files })
}
pub fn assert_source(root: &Path, expected: &Source) -> Result<()> {
    ensure!(
        &source_state(root)? == expected,
        "source changed during packaging; no package was published"
    );
    Ok(())
}

/// 公開先に実在するコミットだけから候補を作る。旧開発履歴の成果物を転用しない。
pub fn public_source(root: &Path, source: &Source) -> Result<()> {
    let remote = run(root, "git", &["remote", "get-url", "origin"], 30)?;
    ensure!(
        [
            REPOSITORY.to_owned(),
            format!("{REPOSITORY}.git"),
            "git@github.com:YukiWorks432/filehue.git".to_owned()
        ]
        .contains(&remote),
        "公開候補は新しいfilehueリポジトリのチェックアウトから作成してください"
    );
    let repository: Value = serde_json::from_str(&run(
        root,
        "gh",
        &["api", "repos/YukiWorks432/filehue"],
        30,
    )?)?;
    let commit: Value = serde_json::from_str(&run(
        root,
        "gh",
        &[
            "api",
            &format!("repos/YukiWorks432/filehue/commits/{}", source.head),
        ],
        30,
    )?)?;
    verify_public_source(&repository, &commit, &source.head)
}

pub fn verify_public_source(repository: &Value, commit: &Value, head: &str) -> Result<()> {
    ensure!(
        repository["full_name"] == "YukiWorks432/filehue" && repository["private"] == false,
        "公開先のリポジトリを確認できません"
    );
    ensure!(commit["sha"] == head, "公開先に対象コミットがありません");
    Ok(())
}

pub fn dependencies(metadata: &Value) -> Result<Vec<&Value>> {
    let packages: BTreeMap<_, _> = metadata["packages"]
        .as_array()
        .context("packagesがありません")?
        .iter()
        .map(|p| (p["id"].as_str().unwrap(), p))
        .collect();
    let nodes: BTreeMap<_, _> = metadata["resolve"]["nodes"]
        .as_array()
        .context("nodesがありません")?
        .iter()
        .map(|p| (p["id"].as_str().unwrap(), p))
        .collect();
    let mut todo: Vec<_> = packages
        .iter()
        .filter(|(_, p)| {
            ["file-icons-manager", "file-icons-shell", "file-icons-cli"]
                .contains(&p["name"].as_str().unwrap_or(""))
        })
        .map(|(id, _)| *id)
        .collect();
    let mut seen = BTreeSet::new();
    while let Some(id) = todo.pop() {
        if !seen.insert(id) {
            continue;
        }
        for dep in nodes.get(id).context("依存ノードがありません")?["deps"]
            .as_array()
            .context("depsがありません")?
        {
            if dep["dep_kinds"]
                .as_array()
                .context("kindがありません")?
                .iter()
                .any(|k| k["kind"] != "dev")
            {
                todo.push(dep["pkg"].as_str().context("pkgがありません")?)
            }
        }
    }
    let mut result: Vec<_> = seen
        .into_iter()
        .map(|id| packages[id])
        .filter(|p| !p["source"].is_null())
        .collect();
    result.sort_by_key(|p| (p["name"].as_str(), p["version"].as_str()));
    Ok(result)
}

fn copy(source: &Path, target: &Path) -> Result<()> {
    fs::create_dir_all(target.parent().unwrap())?;
    fs::copy(source, target)?;
    Ok(())
}

/// ソースでは生成先を案内し、配布物では同梱HTMLへのリンクにする。
pub fn copy_distribution_notice(source: &Path, target: &Path) -> Result<()> {
    let text = fs::read_to_string(source)?;
    let marker = "`licenses/THIRD_PARTY_LICENSES.html`";
    ensure!(
        text.matches(marker).count() == 1,
        "権利表示のHTML案内が不正です: {}",
        source.display()
    );
    let link = "[licenses/THIRD_PARTY_LICENSES.html](licenses/THIRD_PARTY_LICENSES.html)";
    fs::create_dir_all(target.parent().context("権利表示の保存先が不正です")?)?;
    fs::write(target, text.replace(marker, link))?;
    Ok(())
}

/// 準備済みの成果物を、宛先の置換を許さずに移す。
pub fn move_new(source: &Path, target: &Path) -> Result<()> {
    use windows::{
        Win32::Storage::FileSystem::{MOVE_FILE_FLAGS, MoveFileExW},
        core::HSTRING,
    };
    unsafe {
        MoveFileExW(
            &HSTRING::from(source.as_os_str()),
            &HSTRING::from(target.as_os_str()),
            MOVE_FILE_FLAGS(0),
        )
    }
    .with_context(|| format!("既存の宛先は置換できません: {}", target.display()))
}
pub fn licenses(root: &Path, metadata: &Value, destination: &Path) -> Result<Vec<Value>> {
    let extras: Vec<Value> =
        serde_json::from_slice(&fs::read(root.join("licenses/upstream/manifest.json"))?)?;
    let lock: toml::Value = toml::from_str(&fs::read_to_string(root.join("Cargo.lock"))?)?;
    let mut records = Vec::new();
    for package in dependencies(metadata)? {
        let name = package["name"].as_str().context("nameがありません")?;
        let version = package["version"].as_str().context("versionがありません")?;
        let key = format!("{name}-{version}");
        let package_root = Path::new(
            package["manifest_path"]
                .as_str()
                .context("manifest_pathがありません")?,
        )
        .parent()
        .unwrap();
        let output = destination.join(&key);
        fs::create_dir_all(&output)?;
        let mut notices = Vec::new();
        for path in files(package_root)? {
            let filename = path.file_name().unwrap().to_string_lossy().to_uppercase();
            if ["LICENSE", "LICENCE", "NOTICE", "COPYING", "COPYRIGHT"]
                .iter()
                .any(|prefix| filename.starts_with(prefix))
            {
                copy(&package_root.join(&path), &output.join(&path))?;
                notices.push(json!({"path":format!("{key}/{}",slash(&path)),"sha256":digest(&output.join(path))?}));
            }
        }
        let original_notices = !notices.is_empty();
        let matching: Vec<_> = extras
            .iter()
            .filter(|p| p["package"] == name && p["version"] == version)
            .collect();
        let mut supplement_targets = BTreeSet::new();
        for extra in &matching {
            let source = root.join("licenses/upstream").join(relative(
                extra["file"].as_str().context("補完元がありません")?,
            )?);
            ensure!(
                digest(&source)? == extra["sha256"].as_str().unwrap_or(""),
                "changed license supplement: {key}"
            );
            let target_name = extra["destination"]
                .as_str()
                .unwrap_or("UPSTREAM-LICENSE.txt");
            let target_path = relative(target_name)?;
            ensure!(
                supplement_targets.insert(target_name.to_uppercase())
                    && !output.join(&target_path).exists(),
                "duplicate license supplement: {key}/{target_name}"
            );
            let target = output.join(target_path);
            copy(&source, &target)?;
            notices.push(json!({"path":format!("{key}/{target_name}"),"sha256":digest(&target)?,"url":extra["url"],"kind":extra["kind"].as_str().unwrap_or("license")}));
        }
        let missing = !original_notices
            && !matching
                .iter()
                .any(|e| e["kind"].as_str().unwrap_or("license") == "license");
        if missing || !matching.is_empty() {
            for file in ["Cargo.toml.orig", "Cargo.toml", "README.md"] {
                let source = package_root.join(file);
                if source.is_file() {
                    copy(&source, &output.join(file))?;
                    notices.push(json!({"path":format!("{key}/{file}"),"sha256":digest(&output.join(file))?}));
                }
            }
        }
        let mut record = json!({"name":name,"version":version,"license":package["license"],"authors":package["authors"],"repository":package["repository"],"files":notices,"review_required":missing});
        for extra in &matching {
            if let Some(selected) = extra["selected_license"].as_str() {
                ensure!(
                    record["selected_license"].is_null() || record["selected_license"] == selected,
                    "conflicting license selection: {key}"
                );
                record["selected_license"] = json!(selected);
            }
            if let Some(reason) = extra["review_reason"].as_str() {
                record["review_required"] = json!(true);
                record["review_reason"] = json!(reason);
            }
        }
        if package["license"]
            .as_str()
            .unwrap_or("")
            .contains("MPL-2.0")
            || matching.iter().any(|e| e["crate_checksum"].is_string())
        {
            ensure!(
                package["source"].as_str()
                    == Some("registry+https://github.com/rust-lang/crates.io-index"),
                "提供ソースの取得先を確認できない依存です: {key}"
            );
            let registry = package_root.parent().context("registryがありません")?;
            let source = registry
                .parent()
                .and_then(Path::parent)
                .context("crate cacheがありません")?
                .join("cache")
                .join(registry.file_name().unwrap())
                .join(format!("{key}.crate"));
            ensure!(source.is_file(), "source archive required for {key}");
            let locked = lock["package"]
                .as_array()
                .unwrap()
                .iter()
                .find(|p| {
                    p["name"].as_str() == Some(name) && p["version"].as_str() == Some(version)
                })
                .context("Cargo.lockの照合先がありません")?;
            ensure!(
                Some(digest(&source)?.as_str()) == locked["checksum"].as_str(),
                "source archive checksum mismatch: {key}"
            );
            for extra in &matching {
                if let Some(checksum) = extra["crate_checksum"].as_str() {
                    ensure!(
                        locked["checksum"].as_str() == Some(checksum),
                        "license evidence does not match Cargo.lock: {key}"
                    );
                }
            }
            let target = output.join(source.file_name().unwrap());
            copy(&source, &target)?;
            record["source_archive"] = json!({"path":format!("{key}/{}",target.file_name().unwrap().to_string_lossy()),"sha256":digest(&target)?});
        }
        let accepted: Vec<_> = matching
            .iter()
            .filter_map(|entry| entry.get("acceptance"))
            .collect();
        ensure!(accepted.len() <= 1, "採用判断が重複しています: {key}");
        if let Some(value) = accepted.first() {
            let acceptance: LicenseAcceptance = serde_json::from_value((*value).clone())?;
            let evidence: Vec<_> = matching
                .iter()
                .map(|entry| {
                    let mut entry = (*entry).clone();
                    entry.as_object_mut().unwrap().remove("acceptance");
                    entry
                })
                .collect();
            ensure!(
                !missing
                    && record["source_archive"]["sha256"] == acceptance.crate_checksum
                    && hex::encode(Sha256::digest(serde_json::to_vec(&evidence)?))
                        == acceptance.supplements_sha256
                    && !acceptance.decided_on.is_empty()
                    && !acceptance.decided_by.is_empty()
                    && !acceptance.reason.is_empty(),
                "採用判断の原本・補完内容・判断者が一致しません: {key}"
            );
            record["review_required"] = json!(false);
            record["acceptance"] = serde_json::to_value(acceptance)?;
        }
        records.push(record);
    }
    Ok(records)
}

pub fn verify(folder: &Path, metadata: &Path, purpose: Purpose) -> Result<Manifest> {
    let root = folder.canonicalize()?;
    let manifest: Manifest = serde_json::from_slice(&fs::read(metadata.join("manifest.json"))?)?;
    ensure!(
        manifest.format == 5
            && manifest.product == PRODUCT
            && manifest.purpose == purpose
            && manifest.target == TARGET
            && ["release", "debug"].contains(&manifest.profile.as_str())
            && manifest.signature == "unsigned"
            && !manifest.publication_approved,
        "配布形式・用途・対象・未署名方針が一致しません"
    );
    ensure!(
        manifest.source_commit.len() == 40
            && manifest
                .source_commit
                .bytes()
                .all(|b| b.is_ascii_hexdigit()),
        "ソースのコミットが不正です"
    );
    if purpose == Purpose::PublicCandidate {
        ensure!(
            !manifest.source_dirty
                && manifest.profile == "release"
                && manifest.source_repository.as_deref() == Some(REPOSITORY)
                && manifest.license_review_required.is_empty(),
            "公開候補の条件を満たしていません"
        );
    }
    let mut expected = BTreeSet::new();
    let mut names = BTreeSet::new();
    for item in &manifest.files {
        let name = &item.path;
        let relative = relative(name)?;
        ensure!(
            names.insert(name.to_uppercase()) && expected.insert(relative.clone()),
            "manifestのファイルが重複しています"
        );
        let path = root.join(relative);
        ensure!(
            path.canonicalize()?.starts_with(&root)
                && fs::symlink_metadata(&path)?.file_attributes() & 0x400 == 0,
            "unsafe manifest path"
        );
        ensure!(
            path.metadata()?.len() == item.size && digest(&path)? == item.sha256,
            "package checksum mismatch: {name}"
        );
    }
    ensure!(
        files(&root)?.into_iter().collect::<BTreeSet<_>>() == expected,
        "package file list differs from manifest"
    );
    for name in BINARIES.iter().chain(PAYLOAD).chain(&[
        "licenses/THIRD_PARTY_LICENSES.html",
        "licenses/rust/COPYRIGHT-library.html",
        "licenses/VSCODE-LICENSE.txt",
    ]) {
        ensure!(
            expected.contains(&PathBuf::from(name)),
            "必要な配布ファイルがありません: {name}"
        );
    }
    let notice_links = crate::notices::verify_html(&root.join("licenses"))?;
    ensure!(
        notice_links.contains("VSCODE-LICENSE.txt")
            && notice_links.contains("rust/COPYRIGHT-library.html"),
        "必要な権利表示へのリンクがありません"
    );
    ensure!(
        digest(&metadata.join("dependencies.json"))? == manifest.license_records_sha256
            && digest(&metadata.join("about.json"))? == manifest.about_sha256,
        "権利確認記録のハッシュが一致しません"
    );
    let raw_records: Vec<Value> =
        serde_json::from_slice(&fs::read(metadata.join("dependencies.json"))?)?;
    let report: Value = serde_json::from_slice(&fs::read(metadata.join("about.json"))?)?;
    crate::notices::verify_texts(&root.join("licenses"), &raw_records, &report)?;
    let records: Vec<LicenseRecord> = serde_json::from_value(json!(raw_records))?;
    let mut unresolved = Vec::new();
    for record in records {
        if record.review_required {
            unresolved.push(format!("{}@{}", record.name, record.version));
        }
        for notice in record.source_archive.iter() {
            let relative = relative(&notice.path)?;
            ensure!(
                relative.starts_with("sources"),
                "提供ソースの保存先が不正です"
            );
            let path = metadata.join(&relative);
            ensure!(
                path.canonicalize()?.starts_with(metadata.canonicalize()?)
                    && path.is_file()
                    && digest(&path)? == notice.sha256,
                "提供ソースと記録が一致しません: {}",
                notice.path
            );
        }
    }
    ensure!(
        unresolved == manifest.license_review_required,
        "権利確認の一覧が一致しません"
    );
    for name in BINARIES {
        unsigned_pe(&root.join(name))?;
    }
    Ok(manifest)
}

/// PEの証明書テーブルを検査する。自己署名を含め、埋め込み署名を持つ候補は拒否する。
pub fn unsigned_pe(path: &Path) -> Result<()> {
    let mut file = File::open(path)?;
    let mut dos = [0; 64];
    file.read_exact(&mut dos)?;
    ensure!(&dos[..2] == b"MZ", "Windows PEではありません");
    let pe = u64::from(u32::from_le_bytes(dos[60..64].try_into()?));
    file.seek(SeekFrom::Start(pe))?;
    let mut headers = [0; 26];
    file.read_exact(&mut headers)?;
    ensure!(
        &headers[..4] == b"PE\0\0"
            && headers[4..6] == [0x64, 0x86]
            && u16::from_le_bytes(headers[20..22].try_into()?) >= 152
            && headers[24..26] == [0x0b, 0x02],
        "Windows x64のPE32+ではありません"
    );
    file.seek(SeekFrom::Start(pe + 24 + 108))?;
    let mut count = [0; 4];
    file.read_exact(&mut count)?;
    ensure!(
        u32::from_le_bytes(count) >= 5,
        "PEのデータディレクトリが不足しています"
    );
    file.seek(SeekFrom::Start(pe + 24 + 112 + 4 * 8))?;
    let mut security = [0; 8];
    file.read_exact(&mut security)?;
    ensure!(
        security == [0; 8],
        "未署名方針に反する埋め込み署名があります"
    );
    Ok(())
}

pub fn package(
    root: &Path,
    profile: &str,
    allow_dirty: bool,
    purpose: Purpose,
) -> Result<Artifacts> {
    ensure!(
        ["debug", "release"].contains(&profile),
        "構成はdebugまたはreleaseです"
    );
    let source = source_state(root)?;
    ensure!(
        !source.dirty || allow_dirty,
        "commit source changes before packaging, or explicitly use --allow-dirty"
    );
    if purpose == Purpose::PublicCandidate {
        ensure!(
            profile == "release" && !allow_dirty && !source.dirty,
            "公開候補は変更のないソースのrelease構成だけで作成できます"
        );
        public_source(root, &source)?;
    }
    let config: toml::Value = toml::from_str(&fs::read_to_string(root.join("Cargo.toml"))?)?;
    let version = config["workspace"]["package"]["version"]
        .as_str()
        .context("版がありません")?;
    let metadata: Value = serde_json::from_str(&run(
        root,
        "cargo",
        &[
            "metadata",
            "--format-version",
            "1",
            "--locked",
            "--offline",
            "--filter-platform",
            TARGET,
        ],
        180,
    )?)?;
    let mut args = vec![
        "build",
        "--workspace",
        "--exclude",
        "xtask",
        "--locked",
        "--offline",
        "--target",
        TARGET,
    ];
    if profile == "release" {
        args.push("--release");
    }
    let sysroot = run(root, "rustc", &["--print", "sysroot"], 30)?;
    fs::create_dir_all(root.join("work/processes"))?;
    let logs = tempfile::Builder::new()
        .prefix("sanitized-build-")
        .tempdir_in(root.join("work/processes"))?
        .keep();
    crate::process::checked(
        &mut crate::privacy::build_command(root, Path::new(&sysroot), &args)?,
        Duration::from_secs(1800),
        "配布用ビルド",
        &logs,
    )?;
    assert_source(root, &source)?;
    let dist = root.join("dist");
    fs::create_dir_all(&dist)?;
    let name = format!(
        "filehue-{version}-windows-x64-{}-{profile}-{}",
        if purpose == Purpose::PublicCandidate {
            "candidate"
        } else {
            "validation"
        },
        &source.head[..12]
    );
    let output = dist.join(&name);
    ensure!(
        !output.exists(),
        "package destination already exists; preserve it and use another source commit"
    );
    let stage = tempfile::Builder::new()
        .prefix("package-")
        .tempdir_in(&dist)?;
    let payload = stage.path().join("payload");
    fs::create_dir(&payload)?;
    let build = Path::new(
        metadata["target_directory"]
            .as_str()
            .context("target_directoryがありません")?,
    )
    .join(TARGET)
    .join(profile);
    for binary in BINARIES {
        let source = build.join(binary);
        unsigned_pe(&source)?;
        copy(&source, &payload.join(binary))?;
    }
    crate::privacy::verify(&payload, root, Path::new(&sysroot))?;
    for path in PAYLOAD {
        if matches!(
            *path,
            "THIRD_PARTY_NOTICES.md" | "THIRD_PARTY_NOTICES.en.md"
        ) {
            copy_distribution_notice(&root.join(path), &payload.join(path))?;
        } else {
            copy(&root.join(path), &payload.join(path))?;
        }
    }
    let records = crate::notices::generate(root, &metadata, &payload.join("licenses"))?;
    if payload.join("licenses/sources").exists() {
        fs::rename(
            payload.join("licenses/sources"),
            stage.path().join("sources"),
        )?;
    }
    for name in ["about.json", "dependencies.json"] {
        fs::rename(payload.join("licenses").join(name), stage.path().join(name))?;
    }
    let unresolved: Vec<_> = records
        .iter()
        .filter(|p| p["review_required"] == true)
        .map(|p| {
            format!(
                "{}@{}",
                p["name"].as_str().unwrap(),
                p["version"].as_str().unwrap()
            )
        })
        .collect();
    ensure!(
        purpose != Purpose::PublicCandidate || unresolved.is_empty(),
        "未解決の権利確認があります: {unresolved:?}"
    );
    let mut entries = Vec::new();
    for relative in files(&payload)? {
        let path = payload.join(&relative);
        entries.push(Entry {
            path: slash(&relative),
            size: path.metadata()?.len(),
            sha256: digest(&path)?,
        });
    }
    let manifest = Manifest {
        format: 5,
        product: PRODUCT.into(),
        purpose,
        version: version.into(),
        target: TARGET.into(),
        profile: profile.into(),
        signature: "unsigned".into(),
        source_repository: (purpose == Purpose::PublicCandidate).then(|| REPOSITORY.into()),
        source_commit: source.head.clone(),
        source_dirty: source.dirty,
        source_files_sha256: hex::encode(Sha256::digest(serde_json::to_vec(&source.files)?)),
        rustc: run(root, "rustc", &["--version"], 30)?,
        cargo_lock_sha256: digest(&root.join("Cargo.lock"))?,
        license_records_sha256: digest(&stage.path().join("dependencies.json"))?,
        about_sha256: digest(&stage.path().join("about.json"))?,
        license_review_required: unresolved,
        publication_approved: false,
        files: entries,
    };
    fs::write(
        stage.path().join("manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    verify(&payload, stage.path(), purpose)?;
    assert_source(root, &source)?;
    let zipped = stage.path().join(format!("{name}.zip"));
    let mut zip = zip::ZipWriter::new(File::create(&zipped)?);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .compression_level(Some(6));
    for path in files(&payload)? {
        zip.start_file(format!("{name}/{}", slash(&path)), options)?;
        std::io::copy(&mut File::open(payload.join(path))?, &mut zip)?;
    }
    zip.finish()?.flush()?;
    fs::write(
        stage.path().join("SHA256SUMS.txt"),
        format!(
            "{}  {name}.zip\n{}  manifest.json\n",
            digest(&zipped)?,
            digest(&stage.path().join("manifest.json"))?
        ),
    )?;
    assert_source(root, &source)?;
    // 移動の時点でも既存の宛先を拒否する。
    move_new(stage.path(), &output)?;
    let artifacts = Artifacts {
        folder: output.join("payload"),
        archive: output.join(format!("{name}.zip")),
        checksums: output.join("SHA256SUMS.txt"),
    };
    println!(
        "{}",
        json!({"artifacts":artifacts,"sha256":digest(&artifacts.archive)?,"license_review_required":manifest.license_review_required,"purpose":purpose,"signature":"unsigned","publication_approved":false})
    );
    Ok(artifacts)
}
