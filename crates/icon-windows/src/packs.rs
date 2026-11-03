//! パック一覧の削除を先に保存し、素材の削除失敗は再試行できるように記録する。
use crate::{Settings, home, load, save};
use anyhow::{Context, Result, ensure};
use std::{
    fmt, fs,
    path::{Path, PathBuf},
};

#[derive(Debug)]
pub struct PackCleanupPending {
    pub cause: anyhow::Error,
}
impl fmt::Display for PackCleanupPending {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "pack material cleanup is pending: {}",
            self.cause
        )
    }
}
impl std::error::Error for PackCleanupPending {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.cause.as_ref())
    }
}

fn hash_name(name: &str) -> bool {
    name.len() == 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// 削除候補を解決した絶対パスが、所有するpacks直下に収まることを確認する。
fn owned_bucket(path: &Path) -> Result<Option<PathBuf>> {
    if !path.try_exists()? {
        return Ok(None);
    }
    let home = home()?.canonicalize()?;
    let store = home.join("packs");
    ensure!(
        store.canonicalize()? == store,
        "pack storage was redirected"
    );
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .context("invalid pack directory")?;
    ensure!(hash_name(name), "invalid pack directory name");
    let directory = path.canonicalize()?;
    ensure!(
        directory == store.join(name) && directory.is_dir(),
        "pack cleanup target is outside owned storage"
    );
    Ok(Some(directory))
}

fn refers(path: &Path, bucket: &Path) -> Result<bool> {
    if path.starts_with(bucket) {
        return Ok(true);
    }
    Ok(path.try_exists()? && path.canonicalize()?.starts_with(bucket))
}

fn still_referenced(settings: &Settings, bucket: &Path) -> Result<bool> {
    for pack in &settings.packs {
        for path in std::iter::once(&pack.provenance)
            .chain(pack.theme.rules().files.values())
            .chain(pack.theme.rules().extensions.values())
            .chain(pack.theme.rules().language_files.values())
            .chain(pack.theme.rules().language_extensions.values())
            .chain(pack.theme.rules().language_patterns.values())
        {
            if refers(path, bucket)? {
                return Ok(true);
            }
        }
    }
    for fallback in settings.fallbacks.values() {
        if refers(Path::new(&fallback.path), bucket)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn delete_bucket(settings: &Settings, path: &Path) -> Result<()> {
    let Some(directory) = owned_bucket(path)? else {
        return Ok(());
    };
    // ほかのパックや登録前の表示が使う素材は保持する。
    if still_referenced(settings, &directory)? {
        return Ok(());
    }
    let entries = fs::read_dir(&directory)?.collect::<std::io::Result<Vec<_>>>()?;
    // 未知のファイルやリンクを含む場合は、何も削除せず利用者のデータを守る。
    for entry in &entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let generated =
            name == "provenance.json" || name.strip_suffix(".ico").is_some_and(hash_name);
        ensure!(
            generated && entry.file_type()?.is_file() && !entry.file_type()?.is_symlink(),
            "pack directory contains an unexpected file: {}",
            entry.path().display()
        );
        ensure!(
            entry.path().parent() == Some(directory.as_path()),
            "invalid pack file parent"
        );
    }
    for entry in entries {
        fs::remove_file(entry.path())?;
    }
    // 再帰削除は使わず、空になった所有フォルダーだけを除去する。
    fs::remove_dir(directory)?;
    Ok(())
}

/// 適用中のパックは拒否し、共有素材を保持する。配布元の拡張には触れない。
pub fn remove_pack(settings: &mut Settings, id: &str) -> Result<()> {
    *settings = load()?;
    ensure!(
        settings.active.as_deref() != Some(id),
        "restore active pack before removing it"
    );
    let pack = settings
        .packs
        .iter()
        .find(|pack| pack.id == id)
        .context("pack not found")?;
    let parent = pack
        .provenance
        .parent()
        .context("pack provenance parent missing")?;
    let bucket = owned_bucket(parent)?;
    let mut next = settings.clone();
    next.packs.retain(|pack| pack.id != id);
    next.selections.remove(id);
    if let Some(bucket) = bucket
        && !still_referenced(&next, &bucket)?
        && !next.pending_pack_removals.contains(&bucket)
    {
        next.pending_pack_removals.push(bucket);
    }
    save(&next)?;
    *settings = next;
    cleanup_packs(settings)
}

/// 保存済みの削除要求だけを再試行する。失敗した保存先は記録を保持する。
pub fn cleanup_packs(settings: &mut Settings) -> Result<()> {
    *settings = load()?;
    let mut next = settings.clone();
    let mut failures = Vec::new();
    for path in &settings.pending_pack_removals {
        match delete_bucket(settings, path) {
            Ok(()) => next.pending_pack_removals.retain(|pending| pending != path),
            Err(error) => failures.push(format!("{}: {error:#}", path.display())),
        }
    }
    if next.pending_pack_removals != settings.pending_pack_removals {
        save(&next).map_err(|cause| PackCleanupPending { cause })?;
        *settings = next;
    }
    if !failures.is_empty() {
        return Err(PackCleanupPending {
            cause: anyhow::anyhow!(failures.join("\n")),
        }
        .into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_process::CommandOutput;
    use std::{os::windows::fs::OpenOptionsExt, process::Command};

    #[test]
    fn removal_preserves_active_shared_and_external_files_and_retries_failures() {
        if std::env::var_os("WFI_PACK_REMOVAL_PROBE").is_none() {
            let profile = tempfile::tempdir().unwrap();
            let result = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "packs::tests::removal_preserves_active_shared_and_external_files_and_retries_failures", "--nocapture"])
                .env("WFI_PACK_REMOVAL_PROBE", "1").env("LOCALAPPDATA", profile.path()).test_output().unwrap();
            assert!(
                result.status.success(),
                "{} {}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
            return;
        }
        let root = home().unwrap();
        let source = root.join("source-extension");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("package.json"), r#"{"name":"sample","version":"1","contributes":{"iconThemes":[{"id":"one","label":"One","path":"theme.json"},{"id":"two","label":"Two","path":"theme.json"}]}}"#).unwrap();
        fs::write(
            source.join("theme.json"),
            r#"{"iconDefinitions":{"x":{"iconPath":"icon.svg"}},"fileExtensions":{"rs":"x"}}"#,
        )
        .unwrap();
        fs::write(source.join("icon.svg"), r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="16" height="16" fill="#ff0000"/></svg>"##).unwrap();
        let one = file_icons_core::import(&source, "one", &root.join("packs")).unwrap();
        let two = file_icons_core::import(&source, "two", &root.join("packs")).unwrap();
        let first_icon = one.theme.resolve("code.rs").unwrap().to_path_buf();
        let directory = one.provenance.parent().unwrap().to_path_buf();
        let mut settings = Settings {
            active: Some(one.id.clone()),
            packs: vec![one.clone(), two.clone()],
            ..Default::default()
        };
        save(&settings).unwrap();
        let original = fs::read(root.join("settings.json")).unwrap();
        assert!(remove_pack(&mut settings, &one.id).is_err());
        assert_eq!(fs::read(root.join("settings.json")).unwrap(), original);
        assert!(first_icon.is_file());
        // 設定の置き換えができない間は、まだ一覧にあるパックの素材を消さない。
        let locked_settings = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(root.join("settings.json"))
            .unwrap();
        assert!(remove_pack(&mut settings, &two.id).is_err());
        assert_eq!(fs::read(root.join("settings.json")).unwrap(), original);
        assert!(two.provenance.is_file());
        drop(locked_settings);
        remove_pack(&mut settings, &two.id).unwrap();
        assert!(!two.provenance.exists());
        assert_eq!(settings.active.as_ref(), Some(&one.id));
        assert_eq!(settings.packs.len(), 1);
        // 以前の形式などでフォルダーを共有する場合は、ほかのパックを壊さない。
        let mut alias = one.clone();
        alias.id.push_str("/shared");
        settings.packs.push(alias.clone());
        save(&settings).unwrap();
        remove_pack(&mut settings, &alias.id).unwrap();
        assert!(first_icon.is_file() && one.provenance.is_file());
        settings
            .pending_pack_removals
            .push(directory.canonicalize().unwrap());
        save(&settings).unwrap();
        cleanup_packs(&mut settings).unwrap();
        assert!(first_icon.is_file());
        assert!(settings.pending_pack_removals.is_empty());
        settings.active = None;
        settings.fallbacks.insert(
            "Original.File".into(),
            crate::IconLocation {
                path: first_icon.to_string_lossy().into_owned(),
                index: 0,
            },
        );
        save(&settings).unwrap();
        remove_pack(&mut settings, &one.id).unwrap();
        assert!(first_icon.is_file());
        assert!(settings.packs.is_empty());
        settings.fallbacks.clear();
        settings.packs.push(one.clone());
        save(&settings).unwrap();
        let note = directory.join("user-notes.txt");
        fs::write(&note, b"keep user data").unwrap();
        let error = remove_pack(&mut settings, &one.id).unwrap_err();
        assert!(error.is::<PackCleanupPending>());
        assert!(load().unwrap().packs.is_empty());
        assert_eq!(load().unwrap().pending_pack_removals.len(), 1);
        assert!(first_icon.is_file() && one.provenance.is_file());
        assert_eq!(fs::read(&note).unwrap(), b"keep user data");
        fs::remove_file(&note).unwrap();
        let locked_icon = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&first_icon)
            .unwrap();
        assert!(cleanup_packs(&mut settings).is_err());
        assert_eq!(load().unwrap().pending_pack_removals.len(), 1);
        drop(locked_icon);
        cleanup_packs(&mut settings).unwrap();
        assert!(!directory.exists());
        assert!(load().unwrap().pending_pack_removals.is_empty());
        assert!(source.join("package.json").is_file() && source.join("icon.svg").is_file());
        // 設定を編集されても、管理領域外へ削除範囲を広げない。
        let outside = tempfile::tempdir().unwrap();
        let external_bucket = outside.path().join("a".repeat(64));
        fs::create_dir(&external_bucket).unwrap();
        let untouched = external_bucket.join(format!("{}.ico", "b".repeat(64)));
        fs::write(&untouched, b"external data").unwrap();
        fs::write(external_bucket.join("provenance.json"), b"{}").unwrap();
        settings.pending_pack_removals.push(external_bucket);
        save(&settings).unwrap();
        assert!(cleanup_packs(&mut settings).is_err());
        assert_eq!(fs::read(untouched).unwrap(), b"external data");
    }
}
