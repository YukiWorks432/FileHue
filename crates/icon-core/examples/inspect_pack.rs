//! ローカルの展開済み拡張を検証する開発用コマンド。登録は変更しない。
use anyhow::{Context, Result};
use std::path::PathBuf;
fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let root = PathBuf::from(args.next().context("extension folder required")?);
    let cache = PathBuf::from(args.next().context("cache folder required")?);
    for choice in file_icons_core::discover(&root)? {
        let pack = file_icons_core::import(&root, &choice.id, &cache)?;
        println!(
            "{}: {} name rules, {} extension rules",
            pack.id,
            pack.theme.rules().files.len(),
            pack.theme.rules().extensions.len()
        );
        for name in [
            "Cargo.toml",
            "test.toml",
            "package.json",
            "test.json",
            "README.md",
            "test.md",
            "test.rs",
        ] {
            println!("{name}: {:?}", pack.theme.resolve(name));
        }
    }
    Ok(())
}
