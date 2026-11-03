//! 実ユーザーの関連付けを読み取りだけで調査する。登録・通知は行わない。
use anyhow::{Context, Result};
use std::{collections::BTreeSet, path::PathBuf};
use winreg::{RegKey, enums::HKEY_CLASSES_ROOT};

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let source = PathBuf::from(args.next().context("extension directory required")?);
    let cache = PathBuf::from(args.next().context("cache directory required")?);
    let dll = PathBuf::from(args.next().context("Shell DLL required")?);
    for theme in file_icons_core::discover(&source)? {
        let pack = file_icons_core::import(&source, &theme.id, &cache)?;
        let mut originals = BTreeSet::new();
        let mut classes = BTreeSet::new();
        for extension in pack.theme.registration_extensions() {
            classes.insert(file_icons_windows::association(&extension)?);
        }
        if pack.theme.needs_unknown_class() {
            classes.insert("Unknown".into());
        }
        for class in &classes {
            if let Ok(key) = RegKey::predef(HKEY_CLASSES_ROOT)
                .open_subkey(format!("{class}\\shellex\\IconHandler"))
            {
                let handler: String = key.get_value("")?;
                if !handler.is_empty() {
                    originals.insert((class.clone(), handler));
                }
            }
        }
        println!(
            "{}: {} classes, {} existing handlers",
            pack.id,
            classes.len(),
            originals.len()
        );
        for original in originals.iter().take(20) {
            println!("original: {original:?}");
        }
        match file_icons_windows::plan(&pack, &dll) {
            Ok(plan) => println!(
                "plan: {} values, {} potentially new keys, {} original handlers",
                plan.changes.len(),
                plan.created_keys.len(),
                plan.original_handlers.len()
            ),
            Err(error) => println!("plan error: {error:#}"),
        }
    }
    Ok(())
}
