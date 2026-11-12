//! Explorerが読み込むDLLを、配布元から独立した不変の保存先へ配置する。
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};
use windows::Win32::Storage::FileSystem::FILE_SHARE_READ;

/// レジストリは変更しない。同じ内容は再利用し、既存のDLLを上書きしない。
pub fn stage_shell(source: &Path) -> Result<PathBuf> {
    stage_in(
        source,
        &crate::home()?.join("shell"),
        "file_icons_shell.dll",
    )
}

/// サムネイルDLLはアイコン用とは別の不変の保存場所へ配置する。
pub fn stage_thumbnail_shell(source: &Path) -> Result<PathBuf> {
    stage_in(
        source,
        &crate::home()?.join("thumbnail-shell"),
        "file_icons_thumbnail_shell.dll",
    )
}

pub(crate) fn planned_shell(source: &Path) -> Result<PathBuf> {
    planned_in_store(source, "shell", "file_icons_shell.dll")
}

/// 配置せずに、サムネイルDLLの保存予定先を求める。
pub fn planned_thumbnail_shell(source: &Path) -> Result<PathBuf> {
    planned_in_store(source, "thumbnail-shell", "file_icons_thumbnail_shell.dll")
}

fn planned_in_store(source: &Path, store_name: &str, filename: &str) -> Result<PathBuf> {
    let length = fs::metadata(source)?.len();
    ensure!(
        source.is_file() && length > 0 && length <= 512 * 1024 * 1024,
        "invalid Shell DLL source"
    );
    let digest = hash_file(source)?;
    let hash = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let store = existing_directory(&crate::home()?.canonicalize()?.join(store_name))?;
    let bucket = existing_directory(&store.join(hash))?;
    Ok(bucket.join(filename))
}

fn existing_directory(path: &Path) -> Result<PathBuf> {
    if !path.try_exists()? {
        return Ok(path.to_owned());
    }
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_dir()
            && metadata.file_attributes()
                & windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT.0
                == 0,
        "Shell storage directory is a link or reparse point"
    );
    Ok(path.canonicalize()?)
}

fn hash_file(path: &Path) -> Result<Vec<u8>> {
    let mut file = fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut length = 0u64;
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        length += count as u64;
        ensure!(length <= 512 * 1024 * 1024, "Shell DLL too large");
        hash.update(&buffer[..count]);
    }
    Ok(hash.finalize().to_vec())
}

fn stage_in(source: &Path, store: &Path, filename: &str) -> Result<PathBuf> {
    // コピー中のリンク・更新と重ならないよう、書き込みと削除の共有を許可しない。
    let mut source = fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ.0)
        .open(source)
        .context("Shell DLL is missing, unreadable, or being updated")?;
    ensure!(source.metadata()?.is_file(), "Shell DLL must be a file");
    fs::create_dir_all(store)?;
    let store = existing_directory(store)?;
    let mut temporary = tempfile::NamedTempFile::new_in(&store)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut length = 0u64;
    loop {
        let count = source.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        length += count as u64;
        ensure!(length <= 512 * 1024 * 1024, "Shell DLL too large");
        hash.update(&buffer[..count]);
        temporary.write_all(&buffer[..count])?;
    }
    ensure!(length > 0, "Shell DLL is empty");
    temporary.as_file().sync_all()?;
    let digest = hash.finalize();
    let bucket = store.join(hex::encode(digest));
    fs::create_dir_all(&bucket)?;
    // 保存先仮想化は通常のディレクトリにも起きる。実体を解決して以後の保存に使う。
    let bucket = existing_directory(&bucket)?;
    let destination = bucket.join(filename);
    if let Err(error) = temporary.persist_noclobber(&destination) {
        if error.error.kind() != std::io::ErrorKind::AlreadyExists {
            return Err(error.into());
        }
        // ロード中のDLLも読み取り可能であり、再配置のための上書きは不要。
        ensure!(
            hash_file(&destination)? == digest.as_slice(),
            "stored Shell DLL is damaged"
        );
    }
    let installed = destination.canonicalize()?;
    ensure!(
        installed.parent() == Some(bucket.as_path()),
        "Shell storage file was redirected"
    );
    Ok(installed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_process::CommandOutput;

    #[test]
    fn thumbnail_and_icon_libraries_are_stored_independently() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source.dll");
        let store = root.path().join("store");
        fs::write(&source, b"same fixture content").unwrap();
        let icon = stage_in(&source, &store, "file_icons_shell.dll").unwrap();
        let thumbnail = stage_in(&source, &store, "file_icons_thumbnail_shell.dll").unwrap();
        assert_ne!(icon, thumbnail);
        assert_eq!(icon.file_name().unwrap(), "file_icons_shell.dll");
        assert_eq!(
            thumbnail.file_name().unwrap(),
            "file_icons_thumbnail_shell.dll"
        );
        assert_eq!(fs::read(&icon).unwrap(), b"same fixture content");
        assert_eq!(fs::read(&thumbnail).unwrap(), b"same fixture content");
        assert_eq!(
            stage_in(&source, &store, "file_icons_thumbnail_shell.dll").unwrap(),
            thumbnail
        );
    }

    #[test]
    fn thumbnail_planned_path_matches_staged_path_without_prior_write() {
        let root = tempfile::tempdir().unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "deployment::tests::thumbnail_storage_child",
                "--nocapture",
            ])
            .env("LOCALAPPDATA", root.path())
            .env("WFI_THUMBNAIL_STORAGE_TEST", "1")
            .test_output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    #[ignore = "親試験が専用の保存先を指定する"]
    fn thumbnail_storage_child() -> Result<()> {
        ensure!(
            std::env::var("WFI_THUMBNAIL_STORAGE_TEST").as_deref() == Ok("1"),
            "test environment missing"
        );
        let root = crate::home()?;
        fs::create_dir_all(&root)?;
        let source = root.join("fixture.dll");
        fs::write(&source, b"thumbnail fixture")?;
        let planned = planned_thumbnail_shell(&source)?;
        assert!(!planned.exists());
        let installed = stage_thumbnail_shell(&source)?;
        assert_eq!(planned.canonicalize()?, installed);
        assert_eq!(fs::read(installed)?, b"thumbnail fixture");
        Ok(())
    }

    #[test]
    fn linked_storage_is_rejected_without_writing_to_its_target() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source.dll");
        fs::write(&source, b"module contents").unwrap();
        let external = root.path().join("external");
        fs::create_dir(&external).unwrap();
        let store = root.path().join("store");
        fs::create_dir(&store).unwrap();
        let bucket = store.join(hex::encode(Sha256::digest(b"module contents")));
        for link in [root.path().join("linked-store"), bucket] {
            let output = std::process::Command::new("cmd")
                .args(["/d", "/c", "mklink", "/J"])
                .arg(&link)
                .arg(&external)
                .test_output()
                .unwrap();
            assert!(output.status.success());
            let selected = if link.file_name().unwrap() == "linked-store" {
                &link
            } else {
                &store
            };
            assert!(stage_in(&source, selected, "file_icons_shell.dll").is_err());
            assert_eq!(fs::read_dir(&external).unwrap().count(), 0);
        }
    }

    #[test]
    #[ignore = "明示指定した実環境のDLL保存先仮想化を確認する。登録と設定は変更しない"]
    fn shell_storage_in_virtualized_appdata() {
        assert_eq!(std::env::var("WFI_BACKUP_PATH_PROBE").as_deref(), Ok("1"));
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source.dll");
        let contents = format!(
            "virtualized Shell path probe {} {:?}",
            std::process::id(),
            std::time::SystemTime::now()
        );
        fs::write(&source, contents.as_bytes()).unwrap();
        let expected = planned_shell(&source).unwrap();
        assert!(!expected.exists());
        let installed = stage_shell(&source).unwrap();
        assert_eq!(expected.canonicalize().unwrap(), installed);
        assert_eq!(fs::read(&installed).unwrap(), contents.as_bytes());
        assert_eq!(planned_shell(&source).unwrap(), installed);
        assert_eq!(stage_shell(&source).unwrap(), installed);
        fs::remove_file(&installed).unwrap();
        fs::remove_dir(installed.parent().unwrap()).unwrap();
    }

    #[test]
    fn source_can_change_or_disappear_without_replacing_a_staged_version() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("distribution.dll");
        let store = root.path().join("shell");
        fs::write(&source, b"first module contents").unwrap();
        let first = stage_in(&source, &store, "file_icons_shell.dll").unwrap();
        let modified = fs::metadata(&first).unwrap().modified().unwrap();
        assert_ne!(first, source);
        assert_eq!(
            stage_in(&source, &store, "file_icons_shell.dll").unwrap(),
            first
        );
        assert_eq!(fs::metadata(&first).unwrap().modified().unwrap(), modified);
        let writer = fs::OpenOptions::new().write(true).open(&source).unwrap();
        assert!(stage_in(&source, &store, "file_icons_shell.dll").is_err());
        assert_eq!(fs::read(&first).unwrap(), b"first module contents");
        drop(writer);
        fs::write(&source, b"second module contents").unwrap();
        let second = stage_in(&source, &store, "file_icons_shell.dll").unwrap();
        assert_ne!(first, second);
        fs::remove_file(&source).unwrap();
        assert_eq!(fs::read(first).unwrap(), b"first module contents");
        assert_eq!(fs::read(second).unwrap(), b"second module contents");
    }

    #[test]
    fn damaged_existing_copy_is_reported_without_overwriting_it() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("distribution.dll");
        fs::write(&source, b"module contents").unwrap();
        let store = root.path().join("shell");
        let staged = stage_in(&source, &store, "file_icons_shell.dll").unwrap();
        fs::write(&staged, b"damaged existing copy").unwrap();
        assert!(stage_in(&source, &store, "file_icons_shell.dll").is_err());
        assert_eq!(fs::read(staged).unwrap(), b"damaged existing copy");
        assert_eq!(fs::read(source).unwrap(), b"module contents");
    }
}
