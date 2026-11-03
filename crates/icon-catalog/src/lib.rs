//! 確認済み配布物だけを取得し、実行せずテーマと権利表示を読み込む。
use anyhow::{Context, Result, ensure};
use file_icons_core::ImportedPack;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

pub const MAX_DOWNLOAD: u64 = 16 * 1024 * 1024;
const MAX_EXPANDED: u64 = 128 * 1024 * 1024;
const MAX_FILE: u64 = 8 * 1024 * 1024;
const MAX_FILES: usize = 12_000;
const DEADLINE: Duration = Duration::from_secs(180);

pub enum Archive {
    Vsix(&'static str),
    GitHubSource {
        commit: &'static str,
        standard_svg_doctypes: &'static [&'static str],
    },
}

pub struct CatalogEntry {
    pub id: &'static str,
    pub name: &'static str,
    pub author: &'static str,
    pub version: &'static str,
    pub repository: &'static str,
    pub release_tag: &'static str,
    pub archive: Archive,
    pub sha256: &'static str,
    pub publisher: &'static str,
    pub package: &'static str,
    pub themes: &'static [&'static str],
    pub license: &'static str,
}

// 版の更新には配布物・ライセンス・互換性の再確認と、この一覧の更新が必要。
pub static CATALOG: &[CatalogEntry] = &[
    CatalogEntry {
        id: "vscode-icons",
        name: "vscode-icons",
        author: "vscode-icons Contributors",
        version: "13.0.0",
        repository: "vscode-icons/vscode-icons",
        release_tag: "v13.0.0",
        archive: Archive::Vsix("vscode-icons-13.0.0.vsix"),
        sha256: "0f38ea41a7a9395d79d2ca712b5930009416fa609b9a77f9eb90247125258983",
        publisher: "vscode-icons-team",
        package: "vscode-icons",
        themes: &["vscode-icons"],
        license: "Code: MIT · Icons: CC BY-SA 4.0 · Branded icons: respective owners' terms",
    },
    CatalogEntry {
        id: "material-icon-theme",
        name: "Material Icon Theme",
        author: "Material Extensions",
        version: "5.39.0",
        repository: "material-extensions/vscode-material-icon-theme",
        release_tag: "v5.39.0",
        archive: Archive::Vsix("material-icon-theme-5.39.0.vsix"),
        sha256: "f3fda4e1d6095d942c92467c1ad6f322a74c62160f7d29741058e4d726922fbd",
        publisher: "PKief",
        package: "material-icon-theme",
        themes: &["material-icon-theme"],
        license: "MIT",
    },
    CatalogEntry {
        id: "catppuccin-icons",
        name: "Catppuccin Icons",
        author: "Catppuccin",
        version: "1.26.0",
        repository: "catppuccin/vscode-icons",
        release_tag: "v1.26.0",
        archive: Archive::Vsix("catppuccin-vsc-icons-1.26.0.vsix"),
        sha256: "57566136d0a8ba8d040eb6e5be866c88ccbb04f3d219d3a7e52a19978900ff3d",
        publisher: "Catppuccin",
        package: "catppuccin-vsc-icons",
        themes: &[
            "catppuccin-mocha",
            "catppuccin-latte",
            "catppuccin-frappe",
            "catppuccin-macchiato",
        ],
        license: "MIT",
    },
    CatalogEntry {
        id: "symbols",
        name: "Symbols",
        author: "Miguel Solorio",
        version: "0.0.7",
        repository: "miguelsolorio/vscode-symbols",
        release_tag: "0.0.7",
        archive: Archive::Vsix("symbols-0.0.7.vsix"),
        sha256: "42bfe2fe9fd5d95132d7ff9cc4e28675804e6b871f06a0e3d3dad3272020f358",
        publisher: "miguelsolorio",
        package: "symbols",
        themes: &["symbols"],
        license: "MIT",
    },
    CatalogEntry {
        id: "vscode-great-icons",
        name: "VSCode Great Icons",
        author: "Emmanuel Béziat",
        version: "2.1.116",
        repository: "EmmanuelBeziat/vscode-great-icons",
        release_tag: "v2.1.116",
        archive: Archive::GitHubSource {
            commit: "71b1906d885ad0b7d65c0ad908fab43d9a61ea17",
            standard_svg_doctypes: &["icons/pnpm.svg", "icons/singularity.svg"],
        },
        sha256: "5d7648d02dcf3fa54d83d3f517c135d2e4b49cd033d009984e405d36f17fabb6",
        publisher: "emmanuelbeziat",
        package: "vscode-great-icons",
        themes: &["vscode-great-icons"],
        license: "MIT",
    },
];

impl CatalogEntry {
    pub fn url(&self) -> String {
        match self.archive {
            Archive::Vsix(asset) => format!(
                "https://github.com/{}/releases/download/{}/{asset}",
                self.repository, self.release_tag
            ),
            Archive::GitHubSource { commit, .. } => {
                format!(
                    "https://github.com/{}/archive/{commit}.zip",
                    self.repository
                )
            }
        }
    }
    fn archive_root(&self) -> String {
        match self.archive {
            Archive::Vsix(_) => "extension".into(),
            Archive::GitHubSource { commit, .. } => {
                format!("{}-{commit}", self.repository.rsplit('/').next().unwrap())
            }
        }
    }
    pub fn source_url(&self) -> String {
        let revision = match self.archive {
            Archive::Vsix(_) => self.release_tag,
            Archive::GitHubSource { commit, .. } => commit,
        };
        format!("https://github.com/{}/tree/{revision}", self.repository)
    }
    pub fn pack_id(&self, theme: &str) -> String {
        format!(
            "{}.{}/{theme}@{}",
            self.publisher, self.package, self.version
        )
    }
}

#[derive(Debug)]
pub struct Cancelled;
impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("download cancelled")
    }
}
impl std::error::Error for Cancelled {}

pub struct Progress {
    cancelled: AtomicBool,
    pub bytes: AtomicU64,
    /// 0: 通信、1: 検証・展開、2: 変換。
    pub stage: AtomicU8,
    conversion: std::sync::Mutex<ConversionProgress>,
    began: Instant,
}
#[derive(Clone, Copy, Default)]
pub struct ConversionProgress {
    pub theme: usize,
    pub themes: usize,
    pub completed: usize,
    pub total: usize,
}
impl Default for Progress {
    fn default() -> Self {
        Self {
            cancelled: AtomicBool::new(false),
            bytes: AtomicU64::new(0),
            stage: AtomicU8::new(0),
            conversion: Default::default(),
            began: Instant::now(),
        }
    }
}
impl Progress {
    pub fn begin_theme(&self, theme: usize, themes: usize) {
        *self.conversion.lock().unwrap() = ConversionProgress {
            theme,
            themes,
            ..Default::default()
        };
    }
    pub fn converted(&self, completed: usize, total: usize) {
        let mut conversion = self.conversion.lock().unwrap();
        conversion.completed = completed;
        conversion.total = total;
    }
    pub fn conversion(&self) -> ConversionProgress {
        *self.conversion.lock().unwrap()
    }
    pub fn elapsed(&self) -> std::time::Duration {
        self.began.elapsed()
    }
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
    pub fn check(&self) -> Result<()> {
        if self.cancelled.load(Ordering::Relaxed) {
            return Err(Cancelled.into());
        }
        Ok(())
    }
}
fn check(progress: &Progress, began: Instant) -> Result<()> {
    progress.check()?;
    ensure!(
        began.elapsed() < DEADLINE,
        "pack preparation deadline exceeded"
    );
    Ok(())
}

fn approved_url(url: &reqwest::Url) -> bool {
    url.scheme() == "https"
        && url.port_or_known_default() == Some(443)
        && url.username().is_empty()
        && url.password().is_none()
        && matches!(
            url.host_str(),
            Some(
                "github.com"
                    | "release-assets.githubusercontent.com"
                    | "objects.githubusercontent.com"
                    | "codeload.github.com"
            )
        )
}

fn client() -> Result<reqwest::blocking::Client> {
    // reqwest 0.13のOS既定検証へ切り替えず、更新済みの証明書集とringを明示する。
    // OSの証明書更新状況に依存せず、漏洩した認証局の信頼除去を反映する。
    let roots = rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let tls = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()?
    .with_root_certificates(roots)
    .with_no_client_auth();
    Ok(reqwest::blocking::Client::builder()
        .tls_backend_preconfigured(tls)
        .https_only(true)
        .no_proxy()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(60))
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd()
        .user_agent(concat!("file-icons-manager/", env!("CARGO_PKG_VERSION")))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 3 || !approved_url(attempt.url()) {
                attempt.error("unapproved download redirect")
            } else {
                attempt.follow()
            }
        }))
        .build()?)
}

/// Content-Lengthを信用せず、実際に読んだ長さで上限を守る。
fn bounded_copy(
    mut reader: impl Read,
    mut writer: impl Write,
    limit: u64,
    progress: &Progress,
    began: Instant,
    report: bool,
) -> Result<u64> {
    let mut total = 0u64;
    let mut buffer = [0u8; 32 * 1024];
    loop {
        check(progress, began)?;
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        total = total.checked_add(count as u64).context("size overflow")?;
        ensure!(total <= limit, "pack exceeds size limit");
        writer.write_all(&buffer[..count])?;
        if report {
            progress.bytes.store(total, Ordering::Relaxed);
        }
    }
    Ok(total)
}

// ZIPライブラリが全項目を確保する前に、中央ディレクトリと項目数を制限する。
// ZIP64、分割ZIP、末尾の追加データは受け付けない。
fn preflight(bytes: &[u8]) -> Result<usize> {
    ensure!(
        bytes.len() as u64 <= MAX_DOWNLOAD && bytes.len() >= 22,
        "invalid archive size"
    );
    let end = (bytes.len().saturating_sub(65_557)..=bytes.len() - 22)
        .rev()
        .find(|&i| {
            bytes[i..i + 4] == *b"PK\x05\x06"
                && i + 22 + u16::from_le_bytes([bytes[i + 20], bytes[i + 21]]) as usize
                    == bytes.len()
        })
        .context("missing ZIP end record")?;
    let u16at = |i| u16::from_le_bytes([bytes[end + i], bytes[end + i + 1]]);
    let u32at = |i| u32::from_le_bytes(bytes[end + i..end + i + 4].try_into().unwrap()) as usize;
    let count = u16at(10) as usize;
    ensure!(
        u16at(4) == 0 && u16at(6) == 0 && u16at(8) as usize == count,
        "split ZIP unsupported"
    );
    ensure!(
        count > 0 && count <= MAX_FILES && u32at(12) <= 4 * 1024 * 1024,
        "too many archive entries"
    );
    ensure!(
        u32at(16).checked_add(u32at(12)) == Some(end),
        "ZIP64 or invalid directory"
    );
    let mut offset = u32at(16);
    let mut actual = 0;
    let mut expanded = 0u64;
    while offset < end {
        ensure!(
            offset + 46 <= end && bytes[offset..offset + 4] == *b"PK\x01\x02",
            "invalid central directory"
        );
        let short = |i| u16::from_le_bytes([bytes[offset + i], bytes[offset + i + 1]]) as usize;
        let long =
            |i| u32::from_le_bytes(bytes[offset + i..offset + i + 4].try_into().unwrap()) as u64;
        ensure!(
            short(8) & 1 == 0 && matches!(short(10), 0 | 8),
            "encrypted or unsupported ZIP compression"
        );
        ensure!(long(24) <= MAX_FILE, "archive file too large");
        expanded += long(24);
        ensure!(expanded <= MAX_EXPANDED, "expanded pack too large");
        offset += 46 + short(28) + short(30) + short(32);
        actual += 1;
        ensure!(actual <= count, "archive entry count mismatch");
    }
    ensure!(
        offset == end && actual == count,
        "archive entry count mismatch"
    );
    Ok(count)
}

fn safe_path(name: &str) -> Result<PathBuf> {
    ensure!(
        name.len() <= 240 && !name.contains(['\\', ':', '\0']),
        "unsafe archive path"
    );
    let path = name.strip_suffix('/').unwrap_or(name);
    let mut output = PathBuf::new();
    for part in path.split('/') {
        let stem = part
            .split('.')
            .next()
            .unwrap_or("")
            .trim_end_matches([' ', '.'])
            .to_ascii_uppercase();
        ensure!(
            !part.is_empty()
                && part != "."
                && part != ".."
                && !part.ends_with(['.', ' '])
                && !part
                    .chars()
                    .any(|c| c.is_control() || ['<', '>', '"', '|', '?', '*'].contains(&c))
                && !matches!(
                    stem.as_str(),
                    "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
                )
                && !((stem.starts_with("COM") || stem.starts_with("LPT"))
                    && stem[3..].chars().any(|c| c.is_numeric())),
            "unsafe archive path"
        );
        output.push(part);
    }
    Ok(output)
}

fn extract(bytes: &[u8], target: &Path, progress: &Progress, began: Instant) -> Result<()> {
    let count = preflight(bytes)?;
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
    ensure!(archive.len() == count, "archive entry count mismatch");
    let mut seen = BTreeSet::new();
    let mut total = 0u64;
    for index in 0..archive.len() {
        check(progress, began)?;
        let mut entry = archive.by_index(index)?;
        let name = entry.name().context("decode archive path")?;
        let relative = safe_path(name.as_ref())?;
        ensure!(
            seen.insert(relative.to_string_lossy().to_lowercase()),
            "duplicate archive path"
        );
        let mode = entry.unix_mode().unwrap_or(0) & 0o170000;
        ensure!(
            matches!(mode, 0 | 0o100000 | 0o040000) && !entry.is_symlink(),
            "archive links unsupported"
        );
        ensure!(entry.size() <= MAX_FILE, "archive file too large");
        total = total
            .checked_add(entry.size())
            .context("archive size overflow")?;
        ensure!(total <= MAX_EXPANDED, "expanded pack too large");
        ensure!(
            entry.size()
                <= entry
                    .compressed_size()
                    .saturating_mul(1000)
                    .saturating_add(1024),
            "archive expansion ratio too high"
        );
        let destination = target.join(relative);
        // 新規の私有一時領域だけへ書き、パスの正規化とリンク拒否で脱出を防ぐ。
        if entry.is_dir() {
            fs::create_dir_all(destination)?;
            continue;
        }
        fs::create_dir_all(destination.parent().context("missing parent")?)?;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)?;
        let expected = entry.size();
        let copied = bounded_copy(&mut entry, &mut file, expected, progress, began, false)?;
        ensure!(copied == expected, "archive length mismatch");
    }
    Ok(())
}

const STANDARD_SVG_DOCTYPE: &str = "<!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\" \"http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd\">";

// 固定ハッシュで確認した配布物の指定画像だけに使う。DTDの解析・取得は行わない。
fn remove_standard_svg_doctype(bytes: &[u8]) -> Result<Vec<u8>> {
    let text = std::str::from_utf8(bytes)?;
    ensure!(
        text.matches(STANDARD_SVG_DOCTYPE).count() == 1,
        "expected standard SVG doctype missing"
    );
    Ok(text.replacen(STANDARD_SVG_DOCTYPE, "", 1).into_bytes())
}

fn prepare(
    entry: &CatalogEntry,
    bytes: &[u8],
    cache: &Path,
    progress: &Progress,
    began: Instant,
) -> Result<Vec<ImportedPack>> {
    check(progress, began)?;
    ensure!(bytes.len() as u64 <= MAX_DOWNLOAD, "download too large");
    let hash = hex::encode(Sha256::digest(bytes));
    ensure!(hash == entry.sha256, "download SHA-256 mismatch");
    progress.stage.store(1, Ordering::Relaxed);
    let temp = tempfile::tempdir()?;
    extract(bytes, temp.path(), progress, began)?;
    let root = temp.path().join(entry.archive_root());
    let package: serde_json::Value = serde_json::from_slice(&fs::read(root.join("package.json"))?)?;
    ensure!(
        package["publisher"].as_str() == Some(entry.publisher)
            && package["name"].as_str() == Some(entry.package)
            && package["version"].as_str() == Some(entry.version),
        "catalog identity mismatch"
    );
    let choices = file_icons_core::discover(&root)?;
    ensure!(
        choices.len() == entry.themes.len()
            && choices
                .iter()
                .all(|choice| entry.themes.contains(&choice.id.as_str())),
        "catalog themes mismatch"
    );
    let mut normalizations = Vec::new();
    if let Archive::GitHubSource {
        standard_svg_doctypes,
        ..
    } = entry.archive
    {
        for relative in standard_svg_doctypes {
            check(progress, began)?;
            let path = root.join(relative);
            let before = file_icons_core::storage::read_file(&path, MAX_FILE)?;
            let after = remove_standard_svg_doctype(&before)?;
            fs::write(&path, &after)?;
            normalizations.push(serde_json::json!({
                "path": relative, "operation": "remove_standard_svg_doctype",
                "before_sha256": hex::encode(Sha256::digest(&before)),
                "after_sha256": hex::encode(Sha256::digest(&after)),
            }));
        }
    }
    let mut origin = serde_json::json!({"url":entry.url(), "sha256":hash, "repository":entry.repository,
        "version":entry.version, "license_summary":entry.license,
        "conversion":"SVG/PNG/JPEG to Windows ICO; artwork license remains unchanged"});
    if !normalizations.is_empty() {
        origin["normalizations"] = serde_json::json!(normalizations);
    }
    progress.stage.store(2, Ordering::Relaxed);
    fs::create_dir_all(cache)?;
    let staging = tempfile::tempdir_in(cache)?;
    let mut packs = Vec::new();
    let themes = choices.len();
    for (index, choice) in choices.into_iter().enumerate() {
        check(progress, began)?;
        progress.begin_theme(index + 1, themes);
        let mut pack = file_icons_core::import_observed(
            &root,
            &choice.id,
            staging.path(),
            Some(&origin),
            || check(progress, began),
            |completed, total| progress.converted(completed, total),
        )?;
        pack.id.push_str(&format!("@{}", entry.version));
        packs.push(pack);
    }
    check(progress, began)?;
    // 全テーマの変換が成功するまで、通常の保存先へ公開しない。
    let generated: u64 = fs::read_dir(staging.path())?
        .map(|entry| -> Result<u64> {
            let mut size = 0;
            for file in fs::read_dir(entry?.path())? {
                size += file?.metadata()?.len();
            }
            Ok(size)
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .sum();
    ensure!(generated <= MAX_EXPANDED, "generated pack too large");
    for pack in &mut packs {
        let old = pack
            .provenance
            .parent()
            .context("missing pack folder")?
            .to_path_buf();
        let destination = cache.join(old.file_name().context("missing pack name")?);
        if destination.exists() {
            file_icons_core::storage::verify_directory(cache, &destination)?;
            for file in fs::read_dir(&old)? {
                let file = file?;
                let stored = destination.join(file.file_name());
                let expected = fs::read(file.path())?;
                file_icons_core::storage::verify_file(&stored, &expected)?;
            }
        } else {
            fs::rename(&old, &destination).context("publish prepared icon folder")?;
        }
        let mut rules = pack.theme.rules().clone();
        for mapping in [
            &mut rules.files,
            &mut rules.extensions,
            &mut rules.language_files,
            &mut rules.language_extensions,
            &mut rules.language_patterns,
        ] {
            for path in mapping.values_mut() {
                *path = destination.join(path.strip_prefix(&old)?);
            }
        }
        pack.theme.replace_rules(rules)?;
        pack.provenance = destination.join("provenance.json");
    }
    Ok(packs)
}

/// 認証・Cookie・拡張機能の実行・任意URLを必要としない。
/// 呼び出し側は成功後にだけ一覧を保存し、適用操作を別に行う。
pub fn download(id: &str, cache: &Path, progress: Arc<Progress>) -> Result<Vec<ImportedPack>> {
    let entry = CATALOG
        .iter()
        .find(|entry| entry.id == id)
        .context("unknown catalog entry")?;
    let began = Instant::now();
    check(&progress, began)?;
    let response = client()?.get(entry.url()).send()?.error_for_status()?;
    ensure!(
        response
            .content_length()
            .is_none_or(|length| length <= MAX_DOWNLOAD),
        "download too large"
    );
    ensure!(
        response
            .headers()
            .get(reqwest::header::CONTENT_ENCODING)
            .is_none_or(|value| value == "identity"),
        "encoded HTTP body unsupported"
    );
    let mut bytes = Vec::new();
    bounded_copy(response, &mut bytes, MAX_DOWNLOAD, &progress, began, true)?;
    prepare(entry, &bytes, cache, &progress, began)
}

#[cfg(test)]
mod tests;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
