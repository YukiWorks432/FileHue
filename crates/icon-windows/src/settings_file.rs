//! 管理用の一覧・言語と、Shellが読む適用中の情報を分ける。
use crate::{STATE_LIMIT, Settings};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

const FORMAT_VERSION: u32 = 4;

#[derive(Debug)]
pub struct UnsupportedSettingsFormat;

impl std::fmt::Display for UnsupportedSettingsFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("unsupported settings format")
    }
}
impl std::error::Error for UnsupportedSettingsFormat {}

#[derive(Deserialize)]
struct Document {
    #[serde(default)]
    format_version: u32,
    #[serde(flatten)]
    settings: Settings,
}
#[derive(Serialize)]
struct Runtime<'a> {
    format_version: u32,
    #[serde(flatten)]
    settings: &'a Settings,
}
#[derive(Serialize, Deserialize)]
struct Library {
    #[serde(default = "crate::protection::enabled_by_default")]
    preserve_existing_icons: bool,
    #[serde(default)]
    icon_protection_rules: crate::IconProtectionRules,
    #[serde(default)]
    format_version: u32,
    language: String,
    #[serde(default)]
    selections: std::collections::BTreeMap<String, file_icons_core::selection::Selection>,
    packs: Vec<file_icons_core::ImportedPack>,
    pending_pack_removals: Vec<PathBuf>,
}
#[derive(Serialize)]
struct LibraryRef<'a> {
    preserve_existing_icons: bool,
    icon_protection_rules: &'a crate::IconProtectionRules,
    format_version: u32,
    language: &'a str,
    selections: &'a std::collections::BTreeMap<String, file_icons_core::selection::Selection>,
    packs: &'a [file_icons_core::ImportedPack],
    pending_pack_removals: &'a [PathBuf],
}
fn library_path(path: &Path) -> PathBuf {
    path.with_extension("library.json")
}
fn bytes(path: &Path) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(STATE_LIMIT as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= STATE_LIMIT, "settings too large");
    Ok(bytes)
}
fn document(path: &Path) -> Result<Option<Settings>> {
    match bytes(path) {
        Ok(bytes) => {
            let document: Document = serde_json::from_slice(&bytes)?;
            if document.format_version != FORMAT_VERSION {
                return Err(UnsupportedSettingsFormat.into());
            }
            Ok(Some(document.settings))
        }
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}
pub(crate) fn runtime(path: &Path) -> Result<Settings> {
    Ok(document(path)?.unwrap_or_default())
}
pub(crate) fn attach_library(settings: &mut Settings, path: &Path) -> Result<()> {
    let library: Library = serde_json::from_slice(&bytes(&library_path(path))?)?;
    if library.format_version != FORMAT_VERSION {
        return Err(UnsupportedSettingsFormat.into());
    }
    settings.language = library.language;
    settings.preserve_existing_icons = library.preserve_existing_icons;
    settings.icon_protection_rules = library.icon_protection_rules;
    settings.selections = library.selections;
    settings.packs = library.packs;
    settings.pending_pack_removals = library.pending_pack_removals;
    Ok(())
}
pub(crate) fn load(path: &Path) -> Result<Settings> {
    let document = document(path)?;
    let has_document = document.is_some();
    let mut settings = document.unwrap_or_default();
    if has_document || library_path(path).try_exists()? {
        attach_library(&mut settings, path)?;
    }
    Ok(settings)
}
fn stage(root: &Path, data: &[u8]) -> Result<tempfile::NamedTempFile> {
    let mut file = tempfile::NamedTempFile::new_in(root)?;
    file.write_all(data)?;
    file.as_file().sync_all()?;
    Ok(file)
}
pub(crate) fn save(settings: &Settings, path: &Path) -> Result<()> {
    save_inner(settings, path, None)
}

/// 完全な退避を作成した入口だけが、退避時と同じ原本を置き換えられる。
pub(crate) struct DamagedBackup {
    path: PathBuf,
    originals: [Option<Vec<u8>>; 2],
}
impl DamagedBackup {
    pub(crate) fn save(&self, settings: &Settings) -> Result<()> {
        save_inner(settings, &self.path, Some(self))
    }
}
fn optional_bytes(path: &Path) -> Result<Option<Vec<u8>>> {
    if path.try_exists()? {
        Ok(Some(bytes(path)?))
    } else {
        Ok(None)
    }
}
pub(crate) fn backup_damaged(path: &Path) -> Result<DamagedBackup> {
    let paths = [path.to_path_buf(), library_path(path)];
    // 両方の原本を上限まで読み切ってから退避を作る。増大中の入力も上限+1で拒否する。
    let originals = [optional_bytes(&paths[0])?, optional_bytes(&paths[1])?];
    let root = path.parent().context("settings parent missing")?;
    let mut staged = Vec::new();
    for (original, prefix) in originals
        .iter()
        .zip(["settings-damaged-", "settings-library-damaged-"])
    {
        if let Some(data) = original {
            let mut backup = tempfile::Builder::new()
                .prefix(prefix)
                .suffix(".json")
                .tempfile_in(root)?;
            backup.write_all(data)?;
            backup.as_file().sync_all()?;
            staged.push(backup);
        }
    }
    for backup in staged {
        backup.keep()?;
    }
    Ok(DamagedBackup {
        path: path.into(),
        originals,
    })
}
fn save_inner(settings: &Settings, path: &Path, backup: Option<&DamagedBackup>) -> Result<()> {
    save_inner_with(settings, path, backup, |file, path| {
        file.persist(path)?;
        Ok(())
    })
}

fn save_inner_with(
    settings: &Settings,
    path: &Path,
    backup: Option<&DamagedBackup>,
    persist_runtime: impl FnOnce(tempfile::NamedTempFile, &Path) -> Result<()>,
) -> Result<()> {
    let runtime = Settings {
        preserve_existing_icons: settings.preserve_existing_icons,
        icon_exclusions: settings.icon_exclusions.clone(),
        active: settings.active.clone(),
        active_selection: settings.active_selection.clone(),
        packs: settings
            .packs
            .iter()
            .filter(|pack| settings.active.as_ref() == Some(&pack.id))
            .cloned()
            .collect(),
        fallbacks: settings.fallbacks.clone(),
        original_handlers: settings.original_handlers.clone(),
        journal: settings.journal.clone(),
        created_keys: settings.created_keys.clone(),
        ..Default::default()
    };
    let runtime = file_icons_core::bounded_json::encode(
        &Runtime {
            format_version: FORMAT_VERSION,
            settings: &runtime,
        },
        STATE_LIMIT,
    )?;
    let library = file_icons_core::bounded_json::encode(
        &LibraryRef {
            format_version: FORMAT_VERSION,
            icon_protection_rules: &settings.icon_protection_rules,
            preserve_existing_icons: settings.preserve_existing_icons,
            selections: &settings.selections,
            language: &settings.language,
            packs: &settings.packs,
            pending_pack_removals: &settings.pending_pack_removals,
        },
        STATE_LIMIT,
    )?;
    let root = path.parent().context("settings parent missing")?;
    fs::create_dir_all(root)?;
    // 保存前に書込可否を確認する。変更しないShellデータにも既存の排他契約を適用する。
    let _guard = if path.try_exists()? {
        let mut options = fs::OpenOptions::new();
        options.read(true).write(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(5);
        }
        Some(options.open(path)?)
    } else {
        None
    };
    let existing = if path.try_exists()? {
        Some(bytes(path)?)
    } else {
        None
    };
    if let Some(backup) = backup {
        ensure!(
            [optional_bytes(path)?, optional_bytes(&library_path(path))?] == backup.originals,
            "退避後に設定が変わりました。再試行してください / Settings changed after backup; retry"
        );
    } else {
        // 形式番号だけでなく本文も解析し、破損した原本を通常保存で失わない。
        document(path).context(
            "設定を読み取れません。復旧操作を使用してください / Cannot read settings; use recovery",
        )?;
        if library_path(path).try_exists()? {
            attach_library(&mut Settings::default(), path)
                .context("パック一覧を読み取れません。復旧操作を使用してください / Cannot read library; use recovery")?;
        }
    }
    let staged_library = stage(root, &library)?;
    let staged_runtime = if existing.as_deref() == Some(runtime.as_slice()) {
        None
    } else {
        Some(stage(root, &runtime)?)
    };
    // Windowsの原子的置換は既存宛先の追加アクセスを要求するため、事前確認ハンドルを閉じる。
    drop(_guard);
    // 登録前の独立記録は呼出側が先行保存する。一覧を確定してからShell用情報を切り替える。
    let previous_library = if library_path(path).try_exists()? {
        Some(bytes(&library_path(path))?)
    } else {
        None
    };
    staged_library.persist(library_path(path))?;
    if let Some(file) = staged_runtime
        && let Err(error) = persist_runtime(file, path)
    {
        // 二番目の置換に失敗したら、一覧だけが保存済みに見える状態を残さない。
        if let Some(previous) = previous_library {
            stage(root, &previous)
                .and_then(|file| {
                    file.persist(library_path(path))
                        .map(|_| ())
                        .map_err(Into::into)
                })
                .context("failed to restore previous library after settings replacement failed")?;
        } else {
            fs::remove_file(library_path(path))
                .context("failed to remove new library after settings replacement failed")?;
        }
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_replace_failure_restores_previous_library_or_removes_new_one() {
        for existing_library in [true, false] {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("settings.json");
            let library = library_path(&path);
            let original = Settings {
                language: "ja".into(),
                ..Default::default()
            };
            save(&original, &path).unwrap();
            let runtime_before = fs::read(&path).unwrap();
            let library_before = fs::read(&library).unwrap();
            if !existing_library {
                fs::remove_file(&library).unwrap();
            }
            let changed = Settings {
                language: "en".into(),
                active: Some("other".into()),
                ..Default::default()
            };
            let result = save_inner_with(&changed, &path, None, |_, _| {
                anyhow::bail!("simulated runtime replacement failure")
            });
            assert!(result.is_err());
            assert_eq!(fs::read(&path).unwrap(), runtime_before);
            if existing_library {
                assert_eq!(fs::read(&library).unwrap(), library_before);
                assert_eq!(load(&path).unwrap().language, "ja");
            } else {
                assert!(!library.exists());
            }
        }
    }

    #[cfg(windows)]
    #[test]
    fn rollback_failure_returns_error_and_keeps_recovery_record() {
        use std::os::windows::fs::OpenOptionsExt;

        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("settings.json");
        let library = library_path(&path);
        save(&Settings::default(), &path).unwrap();
        let before = fs::read(&path).unwrap();
        let recovery = root.path().join("recovery.json");
        let record = br#"{"version":2,"active":null,"fallbacks":{},"original_handlers":{},"journal":[{"key":"Software\\Classes\\Example.File\\DefaultIcon","name":"","before":null,"after":{"kind":1,"bytes":[0,0]}}],"created_keys":[]}"#;
        fs::write(&recovery, record).unwrap();
        let locked = std::cell::RefCell::new(None);
        let changed = Settings {
            language: "en".into(),
            active: Some("other".into()),
            ..Default::default()
        };
        let result = save_inner_with(&changed, &path, None, |_, _| {
            *locked.borrow_mut() = Some(
                fs::OpenOptions::new()
                    .read(true)
                    .share_mode(0)
                    .open(&library)?,
            );
            anyhow::bail!("simulated runtime replacement failure")
        });
        assert!(
            format!("{:#}", result.unwrap_err()).contains("failed to restore previous library")
        );
        drop(locked);
        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(fs::read(&recovery).unwrap(), record);
        assert!(crate::recovery::Recovery::read(&path).unwrap().is_some());
        assert_eq!(load(&path).unwrap().language, "en");
    }

    #[test]
    fn damaged_backup_is_bounded_complete_and_bound_to_unchanged_originals() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("settings.json");
        let library = library_path(&path);
        let boundary = vec![b' '; STATE_LIMIT];
        fs::write(&path, &boundary).unwrap();
        fs::write(&library, b"broken library").unwrap();
        let backup = backup_damaged(&path).unwrap();
        let saved: Vec<_> = fs::read_dir(root.path())
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("settings-damaged-")
            })
            .collect();
        assert_eq!(saved.len(), 1);
        assert_eq!(fs::read(&saved[0]).unwrap(), boundary);
        fs::write(&library, b"changed after backup").unwrap();
        assert!(backup.save(&Settings::default()).is_err());
        assert_eq!(fs::read(&path).unwrap(), boundary);
        assert_eq!(fs::read(&library).unwrap(), b"changed after backup");
        fs::write(&library, b"broken library").unwrap();
        backup.save(&Settings::default()).unwrap();
        assert!(load(&path).unwrap().active.is_none());

        let other = tempfile::tempdir().unwrap();
        let path = other.path().join("settings.json");
        fs::write(&path, b"broken").unwrap();
        let library = library_path(&path);
        fs::write(&library, vec![b' '; STATE_LIMIT + 1]).unwrap();
        assert!(backup_damaged(&path).is_err());
        assert_eq!(fs::read_dir(other.path()).unwrap().count(), 2);
        assert_eq!(fs::read(&path).unwrap(), b"broken");
        assert_eq!(
            fs::metadata(&library).unwrap().len(),
            (STATE_LIMIT + 1) as u64
        );
    }
}
