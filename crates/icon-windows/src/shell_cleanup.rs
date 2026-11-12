//! 登録解除後の、所有する配置済みDLLだけを検証して削除する。
use crate::{CLSID_TEXT, Settings, home, load};
use anyhow::{Result, ensure};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    os::windows::{
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawHandle,
    },
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use windows::Win32::{Foundation::HANDLE, Storage::FileSystem::*};
use winreg::{
    RegKey,
    enums::{HKEY_CLASSES_ROOT, HKEY_CURRENT_USER},
};

const MAX_ENTRIES: usize = 1024;
const MAX_BYTES: u64 = 512 * 1024 * 1024;
const DEADLINE: Duration = Duration::from_secs(15);

#[derive(Debug)]
pub struct ShellCleanupPending {
    pub cause: anyhow::Error,
}
impl std::fmt::Display for ShellCleanupPending {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.cause)
    }
}
impl std::error::Error for ShellCleanupPending {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.cause.as_ref())
    }
}

fn registered() -> Result<bool> {
    registered_guid(CLSID_TEXT)
}

fn registered_guid(guid: &str) -> Result<bool> {
    let path = format!("CLSID\\{guid}\\InprocServer32");
    for (root, path) in [
        (
            RegKey::predef(HKEY_CURRENT_USER),
            format!("Software\\Classes\\{path}"),
        ),
        (RegKey::predef(HKEY_CLASSES_ROOT), path),
    ] {
        match root.open_subkey(path) {
            Ok(_) => return Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(error.into()),
        }
    }
    Ok(false)
}

/// GUIの単独管理ロック内で実行する。復元と削除は別操作とする。
pub fn cleanup_staged_shell() -> Result<()> {
    (|| {
        let settings = load()?;
        let root = home()?;
        ensure!(
            crate::thumbnails::Recovery::load(&root.join("thumbnail-recovery.json"))?.is_none()
                && crate::thumbnails::Settings::load(&root.join("thumbnail-settings.json"))?
                    .unwrap_or_default()
                    .selected
                    .is_empty(),
            "restore thumbnail registration before deleting staged Shell DLLs"
        );
        for format in crate::thumbnails::Format::ALL {
            let guid = windows::core::GUID::from_u128(format.clsid());
            ensure!(
                !registered_guid(&format!("{{{guid:?}}}"))?,
                "thumbnail provider remains registered"
            );
        }
        cleanup_at(&settings, &root, registered()?)?;
        cleanup_store(&root, "thumbnail-shell", "file_icons_thumbnail_shell.dll")
    })()
    .map_err(|cause| ShellCleanupPending { cause }.into())
}

fn hash_name(name: &str) -> bool {
    name.len() == 64
        && name
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn ordinary(metadata: &fs::Metadata) -> bool {
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 == 0
}

fn delete_verified(
    path: &Path,
    expected_hash: &str,
    budget: &mut u64,
    began: Instant,
) -> Result<()> {
    // 削除権限を含む所有ハンドルを使い、検証と削除の間の差し替えを防ぐ。
    let mut file = fs::OpenOptions::new()
        .access_mode(FILE_GENERIC_READ.0 | DELETE.0)
        .share_mode(FILE_SHARE_READ.0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(path)?;
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file() && ordinary(&metadata),
        "unexpected DLL file type"
    );
    ensure!(
        metadata.len() > 0 && metadata.len() <= *budget,
        "DLL cleanup byte limit reached; retry remaining files"
    );
    verify_open_path(&file, path)?;
    let mut hash = Sha256::new();
    let mut bytes = [0u8; 64 * 1024];
    loop {
        ensure!(
            began.elapsed() < DEADLINE,
            "DLL cleanup deadline reached; retry remaining files"
        );
        let count = file.read(&mut bytes)?;
        if count == 0 {
            break;
        }
        ensure!(count as u64 <= *budget, "DLL cleanup byte limit reached");
        *budget -= count as u64;
        hash.update(&bytes[..count]);
    }
    ensure!(
        hex::encode(hash.finalize()) == expected_hash,
        "stored DLL changed; preserve it for inspection"
    );
    mark_delete(&file)?;
    drop(file);
    Ok(())
}

fn verify_open_path(file: &fs::File, path: &Path) -> Result<()> {
    let handle = HANDLE(file.as_raw_handle());
    let mut buffer = vec![0u16; 32768];
    let length =
        unsafe { GetFinalPathNameByHandleW(handle, &mut buffer, FILE_NAME_NORMALIZED) } as usize;
    ensure!(
        length > 0 && length < buffer.len(),
        "cannot verify opened DLL path"
    );
    let actual = PathBuf::from(String::from_utf16(&buffer[..length])?);
    ensure!(actual == path, "opened DLL is outside owned storage");
    Ok(())
}
fn mark_delete(file: &fs::File) -> Result<()> {
    let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
    unsafe {
        SetFileInformationByHandle(
            HANDLE(file.as_raw_handle()),
            FileDispositionInfo,
            (&disposition as *const FILE_DISPOSITION_INFO).cast(),
            std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
        )?;
    }
    Ok(())
}
fn remove_empty_directory(path: &Path) -> Result<()> {
    let directory = fs::OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES.0 | DELETE.0)
        .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0 | FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(path)?;
    let metadata = directory.metadata()?;
    ensure!(
        metadata.is_dir() && ordinary(&metadata),
        "unexpected Shell bucket type"
    );
    verify_open_path(&directory, path)?;
    mark_delete(&directory)?;
    drop(directory);
    Ok(())
}

fn cleanup_at(settings: &Settings, base: &Path, is_registered: bool) -> Result<()> {
    ensure!(
        !is_registered
            && settings.active.is_none()
            && settings.journal.is_empty()
            && settings.fallbacks.is_empty()
            && settings.original_handlers.is_empty()
            && settings.created_keys.is_empty(),
        "restore registration before deleting staged Shell DLLs"
    );
    cleanup_store(base, "shell", "file_icons_shell.dll")
}

fn cleanup_store(base: &Path, store_name: &str, filename: &str) -> Result<()> {
    let store = base.join(store_name);
    if !store.try_exists()? {
        return Ok(());
    }
    let base = base.canonicalize()?;
    let expected = base.join(store_name);
    ensure!(
        ordinary(&fs::symlink_metadata(&store)?) && store.canonicalize()? == expected,
        "Shell storage was redirected"
    );
    let mut entries = fs::read_dir(&store)?
        .take(MAX_ENTRIES + 1)
        .collect::<std::io::Result<Vec<_>>>()?;
    ensure!(
        entries.len() <= MAX_ENTRIES,
        "too many Shell storage entries; inspect storage before cleanup"
    );
    entries.sort_by_key(|entry| entry.file_name());
    let mut failures = Vec::new();
    let mut failed_count = 0;
    let mut budget = MAX_BYTES;
    let began = Instant::now();
    for entry in entries {
        let result = (|| -> Result<()> {
            ensure!(
                began.elapsed() < DEADLINE,
                "DLL cleanup deadline reached; retry remaining files"
            );
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| anyhow::anyhow!("invalid Shell storage name"))?;
            let directory = entry.path();
            ensure!(
                hash_name(&name)
                    && entry.file_type()?.is_dir()
                    && ordinary(&fs::symlink_metadata(&directory)?),
                "unexpected Shell storage entry"
            );
            let directory = directory.canonicalize()?;
            ensure!(
                directory == expected.join(&name),
                "Shell bucket was redirected"
            );
            let files = fs::read_dir(&directory)?
                .take(2)
                .collect::<std::io::Result<Vec<_>>>()?;
            if files.is_empty() {
                remove_empty_directory(&directory)?;
                return Ok(());
            }
            ensure!(
                files.len() == 1 && files[0].file_name() == filename,
                "unexpected files in Shell bucket"
            );
            let dll = directory.join(filename);
            delete_verified(&dll, &name, &mut budget, began)?;
            // 空の所有フォルダーだけを削除し、再帰削除を使わない。
            remove_empty_directory(&directory)?;
            Ok(())
        })();
        if let Err(error) = result {
            failed_count += 1;
            if failures.len() < 16 {
                failures.push(
                    format!("{}: {error:#}", entry.path().display())
                        .chars()
                        .take(2000)
                        .collect::<String>(),
                );
            }
        }
    }
    ensure!(
        failed_count == 0,
        "{failed_count} Shell storage entries remain:\n{}",
        failures.join("\n")
    );
    Ok(())
}

#[cfg(test)]
mod tests;
