//! 管理する不変ファイルの再利用を、取得方法に依存せず検証する。
use anyhow::{Context, Result, ensure};
use std::{fs, io::Read, path::Path};

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

pub fn verify_directory(root: &Path, directory: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(directory)?;
    ensure!(
        metadata.is_dir() && ordinary(&metadata),
        "redirected pack folder"
    );
    ensure!(
        directory.canonicalize()?
            == root
                .canonicalize()?
                .join(directory.file_name().context("pack folder has no name")?),
        "redirected pack folder"
    );
    Ok(())
}

pub fn read_file(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // 再解析点そのものを開き、検証中の書き込みと差し替えを拒否する。
        options.share_mode(1).custom_flags(0x00200000);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file()
            && ordinary(&metadata)
            && !fs::symlink_metadata(path)?.file_type().is_symlink(),
        "redirected stored file"
    );
    ensure!(
        path.canonicalize()?
            == path
                .parent()
                .unwrap()
                .canonicalize()?
                .join(path.file_name().context("stored file has no name")?),
        "redirected stored file"
    );
    ensure!(metadata.len() <= limit, "stored file is too large");
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= limit, "stored file is too large");
    Ok(bytes)
}

pub fn verify_file(path: &Path, expected: &[u8]) -> Result<()> {
    ensure!(
        read_file(path, expected.len() as u64)? == expected,
        "stored pack is damaged"
    );
    Ok(())
}
