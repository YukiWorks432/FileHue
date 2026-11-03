//! 出典として必要な原文を、容量・件数・走査範囲を制限して収集する。
use anyhow::{Context, Result, ensure};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path, PathBuf},
};

const MAX_BYTES: u64 = 8 * 1024 * 1024;
const MAX_FILES: usize = 256;
const MAX_ENTRIES: usize = 12_000;
fn notice(name: &str) -> bool {
    let name = name.to_ascii_uppercase();
    // license.png等の見本・アイコンを、名前だけで許諾文と誤認しない。
    // 明示参照とlicenses等の専用フォルダーは、この判定を使わず収集する。
    if name.rsplit_once('.').is_some_and(|(_, extension)| {
        matches!(
            extension,
            "SVG" | "PNG" | "JPG" | "JPEG" | "ICO" | "GIF" | "WEBP" | "BMP"
        )
    }) {
        return false;
    }
    [
        "LICENSE",
        "LICENCE",
        "NOTICE",
        "COPYING",
        "COPYRIGHT",
        "README",
        "THIRD_PARTY_NOTICES",
        "THIRD-PARTY-NOTICES",
    ]
    .iter()
    .any(|prefix| name.starts_with(prefix))
}
fn ordinary(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 == 0
    }
    #[cfg(not(windows))]
    {
        !metadata.file_type().is_symlink()
    }
}
fn insert(paths: &mut BTreeSet<PathBuf>, path: PathBuf) -> Result<()> {
    paths.insert(path);
    ensure!(paths.len() <= MAX_FILES, "too many notice files");
    Ok(())
}
pub(super) fn collect(
    root: &Path,
    package: &serde_json::Value,
    check: &mut impl FnMut() -> Result<()>,
) -> Result<BTreeMap<String, String>> {
    let mut paths = BTreeSet::new();
    if let Some(license) = package["license"].as_str() {
        const PREFIX: &str = "SEE LICENSE IN ";
        if license.to_ascii_uppercase().starts_with(PREFIX) {
            let name = license[PREFIX.len()..].trim();
            let path = Path::new(name);
            ensure!(
                !name.is_empty()
                    && !name.contains(['\\', ':'])
                    && path
                        .components()
                        .all(|part| matches!(part, Component::Normal(_))),
                "invalid explicit license path"
            );
            let source = root.join(path);
            ensure!(
                source.canonicalize()?.starts_with(root),
                "license path escapes extension"
            );
            insert(&mut paths, source)?;
        }
    }
    let mut pending = vec![(root.to_path_buf(), 0usize, false)];
    let mut visited = 0usize;
    while let Some((directory, depth, license_directory)) = pending.pop() {
        check()?;
        ensure!(depth <= 8, "notice directory nesting limit exceeded");
        for entry in fs::read_dir(directory)? {
            check()?;
            visited += 1;
            ensure!(visited <= MAX_ENTRIES, "too many extension entries");
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if matches!(name.as_str(), ".git" | ".svn") {
                continue;
            }
            let metadata = fs::symlink_metadata(entry.path())?;
            ensure!(ordinary(&metadata), "linked extension entry is unsupported");
            if metadata.is_dir() {
                let lower = name.to_ascii_lowercase();
                pending.push((
                    entry.path(),
                    depth + 1,
                    license_directory
                        || matches!(lower.as_str(), "licenses" | "licences" | "notices"),
                ));
            } else if metadata.is_file() && (notice(&name) || license_directory) {
                insert(&mut paths, entry.path())?;
            }
        }
    }
    let mut output = BTreeMap::new();
    let mut total = 0u64;
    for path in paths {
        check()?;
        let bytes = crate::storage::read_file(&path, MAX_BYTES - total)
            .context("notice byte budget exceeded")?;
        total += bytes.len() as u64;
        let name = path
            .strip_prefix(root)?
            .to_string_lossy()
            .replace('\\', "/");
        output.insert(name, String::from_utf8(bytes)?);
    }
    Ok(output)
}
