use anyhow::{Context, Result, bail, ensure};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::{PathBuf, absolute},
};

fn main() {
    if let Err(error) = run() {
        eprintln!("{error:#}");
        std::process::exit(1)
    }
}
fn run() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let Some(command) = args.next() else {
        help();
        return Ok(());
    };
    if command == "--help" || command == "-h" {
        help();
        return Ok(());
    }
    let mut values = BTreeMap::<String, OsString>::new();
    let mut dirty = false;
    let mut public_candidate = false;
    let mut skip_shell = false;
    while let Some(arg) = args.next() {
        if arg == "--allow-dirty" {
            ensure!(!dirty, "重複した引数です");
            dirty = true;
            continue;
        }
        if arg == "--public-candidate" {
            ensure!(!public_candidate, "重複した引数です");
            public_candidate = true;
            continue;
        }
        if arg == "--skip-shell" {
            ensure!(!skip_shell, "重複した引数です");
            skip_shell = true;
            continue;
        }
        let name = arg.to_str().context("引数名が不正です")?.to_owned();
        ensure!(
            name.starts_with("--") && !values.contains_key(&name),
            "不正または重複した引数: {name}"
        );
        values.insert(name, args.next().context("引数の値がありません")?);
    }
    let root = xtask::root();
    ensure!(
        !public_candidate || command == "package" || command == "verify-release",
        "--public-candidateはpackageとverify-releaseだけで使えます"
    );
    ensure!(
        !skip_shell || command == "verify-release",
        "--skip-shellはverify-releaseだけで使えます"
    );
    let purpose = if public_candidate {
        xtask::package::Purpose::PublicCandidate
    } else {
        xtask::package::Purpose::Validation
    };
    match command.to_str() {
        Some("check-binaries") => {
            ensure!(
                !dirty && values.keys().all(|s| s == "--folder"),
                "check-binariesの引数が不正です"
            );
            let folder = absolute(PathBuf::from(
                values.get("--folder").context("--folderが必要です")?,
            ))?;
            let sysroot = xtask::package::run(&root, "rustc", &["--print", "sysroot"], 30)?;
            xtask::privacy::verify(&folder, &root, std::path::Path::new(&sysroot))?;
            println!("Binary path privacy: OK");
        }
        Some("licenses") => {
            ensure!(
                !dirty && values.keys().all(|s| s == "--output"),
                "licensesの引数が不正です"
            );
            let output = values
                .get("--output")
                .map(|p| absolute(PathBuf::from(p)))
                .transpose()?;
            xtask::notices::standalone(&root, output.as_deref())?;
        }
        Some("package") => {
            ensure!(
                values
                    .keys()
                    .all(|s| ["--profile", "--verify", "--metadata"].contains(&s.as_str())),
                "packageの引数が不正です"
            );
            if let Some(path) = values.get("--verify") {
                ensure!(
                    !dirty && !values.contains_key("--profile"),
                    "verifyと作成オプションは併用できません"
                );
                let folder = absolute(PathBuf::from(path))?;
                let metadata = values
                    .get("--metadata")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| folder.parent().unwrap().to_path_buf());
                xtask::package::verify(&folder, &metadata, purpose)?;
                let sysroot = xtask::package::run(&root, "rustc", &["--print", "sysroot"], 30)?;
                xtask::privacy::verify(
                    &PathBuf::from(path),
                    &root,
                    std::path::Path::new(&sysroot),
                )?;
                println!("Package manifest: OK");
            } else {
                ensure!(
                    !values.contains_key("--metadata"),
                    "--metadataはverifyで使用してください"
                );
                xtask::package::package(
                    &root,
                    values
                        .get("--profile")
                        .map(|s| s.to_str().context("構成が不正です"))
                        .transpose()?
                        .unwrap_or("release"),
                    dirty,
                    purpose,
                )?;
            }
        }
        Some("verify-release") => {
            ensure!(
                !dirty
                    && values
                        .keys()
                        .all(|s| ["--archive", "--report"].contains(&s.as_str())),
                "verify-releaseの引数が不正です"
            );
            let report = absolute(PathBuf::from(
                values.get("--report").context("--reportが必要です")?,
            ))?;
            let archive = match values.get("--archive") {
                Some(p) => absolute(PathBuf::from(p))?,
                None => xtask::package::package(&root, "release", false, purpose)?.archive,
            };
            println!(
                "{}",
                xtask::release::verify(&root, &archive, &report, purpose, !skip_shell)?
            );
        }
        Some("smoke") => {
            ensure!(
                !dirty
                    && values
                        .keys()
                        .all(|s| ["--executable", "--profile"].contains(&s.as_str())),
                "smokeの引数が不正です"
            );
            let executable = absolute(
                values
                    .get("--executable")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| root.join("target/debug/file-icons-manager.exe")),
            )?;
            let profile = absolute(
                values
                    .get("--profile")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| root.join("work/gui-smoke/profile")),
            )?;
            let parent = profile.parent().context("保存先が不正です")?;
            std::fs::create_dir_all(parent)?;
            let logs = tempfile::Builder::new()
                .prefix("smoke-")
                .tempdir_in(parent)?
                .keep();
            xtask::release::smoke(&executable, &profile, &logs)?;
        }
        Some("check-docs") => {
            ensure!(
                !dirty && values.keys().all(|s| s == "--root"),
                "check-docsの引数が不正です"
            );
            let directory = values.get("--root").map(PathBuf::from).unwrap_or(root);
            println!("Document links: {}", xtask::documents::verify(&directory)?);
        }
        _ => bail!("不明なxtaskです。cargo xtask --helpを確認してください"),
    }
    Ok(())
}
fn help() {
    println!(
        "開発専用コマンド\n  cargo xtask check-binaries --folder <folder>\n  cargo xtask licenses [--output <folder>]\n  cargo xtask package [--public-candidate] [--profile debug|release] [--allow-dirty]\n  cargo xtask package --verify <folder> [--metadata <folder>] [--public-candidate]\n  cargo xtask verify-release [--public-candidate] [--skip-shell] [--archive <zip>] --report <json>\n  cargo xtask smoke [--executable <exe>] [--profile <folder>]\n  cargo xtask check-docs [--root <folder>]"
    )
}
