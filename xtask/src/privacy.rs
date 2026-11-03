//! 配布用ビルドでのパス置換と、完成したバイナリの漏えい検査。
use anyhow::{Context, Result, ensure};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

pub fn roots(root: &Path, sysroot: &Path) -> Vec<(PathBuf, &'static str)> {
    let mut result = Vec::new();
    if let Some(home) = env::var_os("USERPROFILE") {
        let home = PathBuf::from(home);
        result.push((home.clone(), "/build/user"));
        result.push((
            env::var_os("CARGO_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".cargo")),
            "/build/cargo",
        ));
        result.push((
            env::var_os("RUSTUP_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".rustup")),
            "/build/rustup",
        ));
    }
    result.push((sysroot.to_path_buf(), "/build/toolchain"));
    result.push((root.to_path_buf(), "/build/source"));
    result
}

pub fn flags(mappings: &[(PathBuf, &str)], existing: &[String]) -> Result<String> {
    let mut flags = existing.to_vec();
    // 複数の規則が一致した場合は最後の指定が優先される。
    for (path, replacement) in mappings {
        let path = path.to_str().context("ビルドパスがUTF-8ではありません")?;
        for variant in [path.replace('\\', "/"), path.replace('/', "\\")] {
            ensure!(
                !variant.contains('\x1f'),
                "ビルドパスに不正な区切りがあります"
            );
            flags.push(format!("--remap-path-prefix={variant}={replacement}"));
        }
    }
    // PDB本体とPE内のデバッグ情報を配布しない。
    flags.push("--remap-path-scope=all".into());
    flags.push("-Cstrip=debuginfo".into());
    Ok(flags.join("\x1f"))
}

pub fn build_command(root: &Path, sysroot: &Path, args: &[&str]) -> Result<Command> {
    let existing: Vec<String> = if let Ok(encoded) = env::var("CARGO_ENCODED_RUSTFLAGS") {
        encoded
            .split('\x1f')
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect()
    } else {
        env::var("RUSTFLAGS")
            .unwrap_or_default()
            .split_whitespace()
            .map(str::to_owned)
            .collect()
    };
    let mut command = Command::new("cargo");
    command.args(args).current_dir(root).env(
        "CARGO_ENCODED_RUSTFLAGS",
        flags(&roots(root, sysroot), &existing)?,
    );
    Ok(command)
}

/// UTF-8とUTF-16LEの両方を、区切り・ASCIIの大文字小文字の違いを含めて検査する。
pub fn inspect(data: &[u8], prefixes: &[String]) -> Result<()> {
    let lowercase = data.to_ascii_lowercase();
    let generic = (b'a'..=b'z')
        .flat_map(|drive| {
            [
                format!("{}:\\users\\", drive as char),
                format!("{}:/users/", drive as char),
            ]
        })
        .collect::<Vec<_>>();
    for prefix in prefixes.iter().chain(generic.iter()) {
        if prefix.is_empty() {
            continue;
        }
        for variant in [prefix.replace('\\', "/"), prefix.replace('/', "\\")] {
            let utf8 = variant.as_bytes().to_ascii_lowercase();
            let utf16 = variant
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>()
                .to_ascii_lowercase();
            ensure!(
                memchr::memmem::find(&lowercase, &utf8).is_none()
                    && memchr::memmem::find(&lowercase, &utf16).is_none(),
                "配布バイナリにビルド元の絶対パスが残っています"
            );
        }
    }
    Ok(())
}

pub fn verify(folder: &Path, root: &Path, sysroot: &Path) -> Result<()> {
    let prefixes: Vec<_> = roots(root, sysroot)
        .into_iter()
        .map(|(p, _)| p.to_string_lossy().into_owned())
        .collect();
    for name in crate::package::BINARIES {
        inspect(&fs::read(folder.join(name))?, &prefixes)
            .with_context(|| format!("バイナリのパス検査: {name}"))?;
    }
    Ok(())
}
