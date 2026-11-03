//! 原文の収集を保持し、cargo-aboutの選択結果を配布対象へ照合する。
use crate::package;
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

pub const ABOUT_VERSION: &str = "0.9.2";

/// 元の依存IDで照合し、ツールが返した絶対パス等を配布形式へ持ち込まない。
pub fn normalize(metadata: &Value, raw: &Value, records: &mut [Value]) -> Result<Value> {
    let expected: BTreeMap<_, _> = package::dependencies(metadata)?
        .into_iter()
        .map(|p| {
            (
                p["id"].as_str().unwrap(),
                (p["name"].as_str().unwrap(), p["version"].as_str().unwrap()),
            )
        })
        .collect();
    let mut selections = BTreeMap::<(&str, &str), BTreeSet<&str>>::new();
    let mut licenses = Vec::new();
    for license in raw["licenses"]
        .as_array()
        .context("cargo-aboutのlicensesがありません")?
    {
        let id = license["id"]
            .as_str()
            .context("ライセンス識別子がありません")?;
        let text = license["text"]
            .as_str()
            .context("ライセンス本文がありません")?;
        ensure!(
            !id.is_empty() && !text.trim().is_empty(),
            "空のライセンス情報です"
        );
        let mut users = BTreeSet::new();
        for user in license["used_by"]
            .as_array()
            .context("used_byがありません")?
        {
            let package_id = user["crate"]["id"].as_str().context("依存IDがありません")?;
            if let Some(&(name, version)) = expected.get(package_id) {
                users.insert((name, version));
                selections.entry((name, version)).or_default().insert(id);
            }
        }
        if !users.is_empty() {
            licenses.push(json!({"id":id,"name":license["name"].as_str().context("ライセンス名がありません")?,"text":text,
                "used_by":users.into_iter().map(|(name,version)| json!({"name":name,"version":version})).collect::<Vec<_>>()}));
        }
    }
    ensure!(!licenses.is_empty(), "配布対象のライセンス情報がありません");
    for item in raw["crates"]
        .as_array()
        .context("cargo-aboutのcratesがありません")?
    {
        let id = item["package"]["id"]
            .as_str()
            .context("依存IDがありません")?;
        if let Some(key) = expected.get(id) {
            ensure!(
                selections.contains_key(key),
                "cargo-about対象の依存にライセンス本文がありません: {id}"
            );
        }
    }
    for record in records {
        let key = (
            record["name"].as_str().context("依存名がありません")?,
            record["version"].as_str().context("依存版がありません")?,
        );
        let ids = selections.get(&key).cloned().unwrap_or_default();
        if let Some(selected) = record["selected_license"].as_str() {
            ensure!(
                ids.contains(selected),
                "補完時とcargo-aboutのライセンス選択が一致しません: {}@{}",
                key.0,
                key.1
            );
        }
        // 通常ビルドに未使用の機能由来の依存も原文収集には残す。
        record["selection_status"] = json!(if ids.is_empty() {
            "supplement-only"
        } else {
            "cargo-about"
        });
        record["about_licenses"] = json!(ids);
    }
    Ok(
        json!({"format":1,"generator":format!("cargo-about {ABOUT_VERSION}"),"target":package::TARGET,
        "scope":"default-features-normal-and-build-dependencies","licenses":licenses}),
    )
}

fn escaped(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// 原文を内容ごとに集約し、提供ソースの取得先とハッシュを示す。
pub fn html(report: &Value, records: &[Value], source: &Path) -> Result<String> {
    let mut originals = BTreeMap::<String, String>::new();
    let mut output = String::from(
        "<!doctype html><html lang=\"ja\"><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'\"><title>第三者の権利表示 / Third-party notices</title><style>body{font:16px system-ui;max-width:76rem;margin:2rem auto;padding:0 1rem}a{overflow-wrap:anywhere}pre{white-space:pre-wrap;overflow-wrap:anywhere}article{border-top:1px solid #888;padding:1rem 0}li{margin:.5rem 0;overflow-wrap:anywhere}</style><h1>第三者の権利表示 / Third-party notices</h1><p>選択結果とライセンス本文を掲載します。原文・追加通知・提供ソースは各依存のリンクを確認してください。生成成功は公開承認を意味しません。</p><p>Selected licenses and texts are listed below. Follow each dependency's links for original notices and source archives. Generation does not constitute publication approval.</p><p><a href=\"rust/COPYRIGHT-library.html\">Rust標準ライブラリ / Rust standard library</a>（配布ZIPに同梱 / included in the distribution ZIP）</p><p><a href=\"VSCODE-LICENSE.txt\">VS Code言語定義 / VS Code language definitions</a></p><h2>依存と原文 / Dependencies and originals</h2>",
    );
    for record in records {
        output.push_str(&format!(
            "<article><h3>{} {}</h3><p>{}</p>",
            escaped(record["name"].as_str().context("依存名がありません")?),
            escaped(record["version"].as_str().context("依存版がありません")?),
            escaped(&record["about_licenses"].to_string())
        ));
        if record["selection_status"] == "supplement-only" {
            output.push_str("<p>補足収集した原文です。通常ビルドのcargo-about選択対象外です。 / Supplementary originals outside cargo-about's default-build selection.</p>");
        }
        if record["review_required"] == true {
            output.push_str("<p><strong>要確認 / Review required</strong></p>");
            if let Some(reason) = record["review_reason"].as_str() {
                output.push_str(&format!("<p>{}</p>", escaped(reason)));
            }
        }
        if let Some(acceptance) = record.get("acceptance") {
            output.push_str(&format!(
                "<p>採用判断済み / Accepted for distribution: {} — {}</p><p>{}</p>",
                escaped(
                    acceptance["decided_on"]
                        .as_str()
                        .context("採用日がありません")?
                ),
                escaped(
                    acceptance["decided_by"]
                        .as_str()
                        .context("判断者がありません")?
                ),
                escaped(
                    acceptance["reason"]
                        .as_str()
                        .context("採用理由がありません")?
                )
            ));
            if let Some(reason) = record["review_reason"].as_str() {
                output.push_str(&format!("<p>{}</p>", escaped(reason)));
            }
        }
        output.push_str("<ul>");
        let mut entries = record["files"]
            .as_array()
            .context("原文一覧がありません")?
            .clone();
        if !record["source_archive"].is_null() {
            entries.push(record["source_archive"].clone());
        }
        for entry in entries {
            let path = entry["path"].as_str().context("原文パスがありません")?;
            let hash = entry["sha256"]
                .as_str()
                .context("原文ハッシュがありません")?;
            let is_archive = path.starts_with("sources/");
            let href = if is_archive {
                entry["url"]
                    .as_str()
                    .context("提供ソースの取得先がありません")?
                    .to_owned()
            } else {
                let original = source.join(package::relative(path)?);
                let bytes = fs::read(&original)?;
                ensure!(
                    hex::encode(Sha256::digest(&bytes)) == hash,
                    "原文が変更されました: {path}"
                );
                originals
                    .entry(hash.to_owned())
                    .or_insert(String::from_utf8(bytes)?);
                format!("#notice-{hash}")
            };
            output.push_str(&format!(
                "<li><a href=\"{}\">{}</a>{}</li>",
                escaped(&href),
                escaped(entry["original_path"].as_str().unwrap_or(path)),
                if is_archive {
                    format!(" — SHA-256: {}", escaped(hash))
                } else {
                    String::new()
                }
            ));
        }
        output.push_str("</ul></article>");
    }
    output.push_str("<h2>ライセンス本文 / License texts</h2>");
    for license in report["licenses"]
        .as_array()
        .context("licensesがありません")?
    {
        output.push_str(&format!(
            "<article><h3>{}</h3><p>",
            escaped(license["id"].as_str().context("識別子がありません")?)
        ));
        for user in license["used_by"]
            .as_array()
            .context("used_byがありません")?
        {
            output.push_str(&format!(
                "{} {}; ",
                escaped(user["name"].as_str().context("名前がありません")?),
                escaped(user["version"].as_str().context("版がありません")?)
            ));
        }
        let content = license["text"].as_str().context("本文がありません")?;
        let hash = hex::encode(Sha256::digest(content.as_bytes()));
        originals
            .entry(hash.clone())
            .or_insert_with(|| content.to_owned());
        output.push_str(&format!(
            "</p><p><a href=\"#notice-{hash}\">本文 / Text</a></p></article>"
        ));
    }
    output.push_str("<h2>原文 / Original texts</h2>");
    for (hash, content) in originals {
        output.push_str(&format!(
            "<article id=\"notice-{hash}\"><pre>{}</pre></article>",
            escaped(&content)
        ));
    }
    output.push_str("</html>");
    Ok(output)
}

/// 配布用HTMLのリンクを展開先に対して検査する。
pub fn verify_html(destination: &Path) -> Result<BTreeSet<String>> {
    let html = fs::read_to_string(destination.join("THIRD_PARTY_LICENSES.html"))?;
    let root = destination.canonicalize()?;
    let mut links = BTreeSet::new();
    for rest in html.split("<a href=\"").skip(1) {
        let encoded = rest
            .split_once('"')
            .context("権利表示のリンクが不正です")?
            .0;
        if encoded.starts_with("#notice-")
            || encoded.starts_with("https://static.crates.io/crates/")
        {
            continue;
        }
        let decoded = percent_encoding::percent_decode_str(encoded).decode_utf8()?;
        let path = root.join(package::relative(&decoded)?).canonicalize()?;
        ensure!(
            path.starts_with(&root) && path.is_file(),
            "権利表示のリンク先が不正です"
        );
        links.insert(decoded.into_owned());
    }
    ensure!(!links.is_empty(), "権利表示のリンクがありません");
    Ok(links)
}

/// 記録された各依存から原文へ到達でき、その原文が収集時のバイト列と一致することを確認する。
pub fn verify_texts(destination: &Path, records: &[Value], report: &Value) -> Result<()> {
    let html = fs::read_to_string(destination.join("THIRD_PARTY_LICENSES.html"))?;
    let mut hashes = BTreeSet::new();
    for record in records {
        let heading = format!(
            "<article><h3>{} {}</h3>",
            escaped(record["name"].as_str().context("依存名がありません")?),
            escaped(record["version"].as_str().context("依存版がありません")?)
        );
        let section = html
            .split_once(&heading)
            .context("依存の権利表示がありません")?
            .1
            .split_once("</article>")
            .context("依存の権利表示が不正です")?
            .0;
        for entry in record["files"].as_array().context("原文一覧がありません")? {
            let hash = entry["sha256"]
                .as_str()
                .context("原文ハッシュがありません")?;
            let path = entry["path"].as_str().context("原文パスがありません")?;
            let link = format!("<a href=\"#notice-{hash}\">{}</a>", escaped(path));
            ensure!(
                section.contains(&link),
                "依存から原文への参照がありません: {path}"
            );
            hashes.insert(hash.to_owned());
        }
        if let Some(archive) = record.get("source_archive").filter(|v| !v.is_null()) {
            let path = archive["path"]
                .as_str()
                .context("提供ソースのパスがありません")?;
            let label = archive["original_path"].as_str().unwrap_or(path);
            let name = record["name"].as_str().context("依存名がありません")?;
            let version = record["version"].as_str().context("依存版がありません")?;
            let href = format!("https://static.crates.io/crates/{name}/{name}-{version}.crate");
            ensure!(
                archive["url"] == href,
                "提供ソースの取得先が版と一致しません: {name}@{version}"
            );
            let hash = archive["sha256"]
                .as_str()
                .context("提供ソースのハッシュがありません")?;
            ensure!(
                section.contains(&format!(
                    "<a href=\"{}\">{}</a> — SHA-256: {}",
                    escaped(&href),
                    escaped(label),
                    escaped(hash)
                )),
                "依存から提供ソースへの参照がありません: {path}"
            );
        }
    }
    for license in report["licenses"]
        .as_array()
        .context("ライセンス一覧がありません")?
    {
        let content = license["text"]
            .as_str()
            .context("ライセンス本文がありません")?;
        hashes.insert(hex::encode(Sha256::digest(content.as_bytes())));
    }
    for hash in hashes {
        let marker = format!("<article id=\"notice-{hash}\"><pre>");
        let content = html
            .split_once(&marker)
            .context("原文がありません")?
            .1
            .split_once("</pre></article>")
            .context("原文が不正です")?
            .0;
        let decoded = content
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&#39;", "'")
            .replace("&amp;", "&");
        ensure!(
            hex::encode(Sha256::digest(decoded.as_bytes())) == hash,
            "原文の内容が一致しません: {hash}"
        );
    }
    Ok(())
}

pub fn generate(root: &Path, metadata: &Value, destination: &Path) -> Result<Vec<Value>> {
    let version = package::run(root, "cargo", &["about", "--version"], 30)?;
    ensure!(
        version == format!("cargo-about {ABOUT_VERSION}"),
        "cargo-about {ABOUT_VERSION}を導入してください（取得した版: {version}）"
    );
    // cargo-aboutはPowerShell環境での標準出力転送を拒否するため、専用ファイルで受け取る。
    let raw_dir = tempfile::Builder::new()
        .prefix("about-")
        .tempdir_in(root.join("work"))?;
    let raw_path = raw_dir.path().join("about.json");
    package::run(
        root,
        "cargo",
        &[
            "about",
            "generate",
            "--workspace",
            "--locked",
            "--offline",
            "--target",
            package::TARGET,
            "--config",
            "about.toml",
            "--format",
            "json",
            "--fail",
            "--output-file",
            raw_path.to_str().context("cargo-aboutの出力先が不正です")?,
        ],
        300,
    )?;
    let raw: Value = serde_json::from_slice(&fs::read(raw_path)?)?;
    fs::create_dir_all(root.join("work"))?;
    let originals = tempfile::Builder::new()
        .prefix("license-originals-")
        .tempdir_in(root.join("work"))?;
    let mut records = package::licenses(root, metadata, originals.path())?;
    let sysroot = package::run(root, "rustc", &["--print", "sysroot"], 30)?;
    fs::create_dir_all(destination.join("rust"))?;
    fs::copy(
        Path::new(&sysroot).join("share/doc/rust/COPYRIGHT-library.html"),
        destination.join("rust/COPYRIGHT-library.html"),
    )?;
    fs::copy(
        root.join("crates/icon-core/data/VSCODE-LICENSE.txt"),
        destination.join("VSCODE-LICENSE.txt"),
    )?;
    let mut report = normalize(metadata, &raw, &mut records)?;
    copy_sources(originals.path(), destination, &mut records)?;
    report["cargo_lock_sha256"] = json!(package::digest(&root.join("Cargo.lock"))?);
    report["config_sha256"] = json!(package::digest(&root.join("about.toml"))?);
    fs::write(
        destination.join("dependencies.json"),
        serde_json::to_vec_pretty(&records)?,
    )?;
    fs::write(
        destination.join("about.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    fs::write(
        destination.join("THIRD_PARTY_LICENSES.html"),
        html(&report, &records, originals.path())?,
    )?;
    Ok(records)
}

/// 提供ソースを配布先へコピーし、元の名前を表示用に保持する。
pub fn copy_sources(source: &Path, destination: &Path, records: &mut [Value]) -> Result<()> {
    let copy_source = |entry: &mut Value, name: &str, version: &str| -> Result<()> {
        let original = entry["path"]
            .as_str()
            .context("原文パスがありません")?
            .to_owned();
        let relative = package::relative(&original)?;
        let hash = entry["sha256"]
            .as_str()
            .context("原文ハッシュがありません")?
            .to_owned();
        ensure!(
            package::digest(&source.join(&relative))? == hash,
            "原文が変更されました: {original}"
        );
        let path = PathBuf::from("sources").join(&relative);
        fs::create_dir_all(destination.join(path.parent().unwrap()))?;
        fs::copy(source.join(&relative), destination.join(&path))?;
        entry["original_path"] = json!(original);
        entry["path"] = json!(path.to_string_lossy().replace('\\', "/"));
        entry["url"] = json!(format!(
            "https://static.crates.io/crates/{name}/{name}-{version}.crate"
        ));
        Ok(())
    };
    for record in records {
        let name = record["name"]
            .as_str()
            .context("依存名がありません")?
            .to_owned();
        let version = record["version"]
            .as_str()
            .context("依存版がありません")?
            .to_owned();
        if let Some(archive) = record.get_mut("source_archive").filter(|v| !v.is_null()) {
            copy_source(archive, &name, &version)?;
        }
    }
    Ok(())
}

pub fn standalone(root: &Path, output: Option<&Path>) -> Result<()> {
    if let Some(path) = output {
        ensure!(
            !path.exists(),
            "既存の権利表示は上書きしません: {}",
            path.display()
        );
    }
    let work = root.join("work");
    fs::create_dir_all(&work)?;
    let stage = tempfile::Builder::new()
        .prefix("licenses-")
        .tempdir_in(&work)?;
    let metadata: Value = serde_json::from_str(&package::run(
        root,
        "cargo",
        &[
            "metadata",
            "--locked",
            "--offline",
            "--format-version",
            "1",
            "--filter-platform",
            package::TARGET,
        ],
        180,
    )?)?;
    let records = generate(root, &metadata, stage.path())?;
    let path = if let Some(output) = output {
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
        package::move_new(stage.path(), output)?;
        output.to_path_buf()
    } else {
        stage.keep()
    };
    println!(
        "{}",
        json!({"folder":path,"dependencies":records.len(),"review_required":records.iter().filter(|r| r["review_required"] == true).count()})
    );
    Ok(())
}
