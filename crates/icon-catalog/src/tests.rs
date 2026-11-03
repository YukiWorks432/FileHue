use super::*;
use test_support::{archive, fixture};

#[test]
fn download_client_rejects_untrusted_tls_without_changing_system_roots() {
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};
    use std::net::TcpListener;

    // 証明書・鍵はこの試験専用。製品のクライアントへ信頼を追加しない。
    let certificate =
        CertificateDer::from_pem_slice(include_bytes!("test-fixtures/localhost-cert.pem")).unwrap();
    let key =
        PrivateKeyDer::from_pem_slice(include_bytes!("test-fixtures/localhost-key.pem")).unwrap();
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(vec![certificate], key)
    .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        listener.set_nonblocking(true).unwrap();
        let began = Instant::now();
        let (socket, _) = loop {
            match listener.accept() {
                Ok(accepted) => break accepted,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(began.elapsed() < Duration::from_secs(10));
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("{error}"),
            }
        };
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        socket
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let connection = rustls::ServerConnection::new(Arc::new(config)).unwrap();
        let mut stream = rustls::StreamOwned::new(connection, socket);
        // 証明書を拒否したクライアントからのTLS警告は正常な試験結果。
        let mut request = [0; 1024];
        if stream.read(&mut request).is_ok() {
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
        }
    });
    let result = client().unwrap().get(format!("https://{address}/")).send();
    server.join().unwrap();
    let error = result.expect_err("未知の自己署名証明書を受け入れてはいけない");
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(&error);
    let mut unknown_issuer = false;
    while let Some(error) = source {
        unknown_issuer |= error.downcast_ref::<rustls::Error>().is_some_and(|error| {
            matches!(
                error,
                rustls::Error::InvalidCertificate(rustls::CertificateError::UnknownIssuer)
            )
        });
        source = match error.downcast_ref::<std::io::Error>() {
            // io::Error::sourceは内部エラー自体を飛ばすため、get_refで検証結果までたどる。
            Some(error) => error
                .get_ref()
                .map(|inner| inner as &(dyn std::error::Error + 'static)),
            None => error.source(),
        };
    }
    assert!(
        unknown_issuer,
        "証明書検証以外の失敗を成功扱いしない: {error:?}"
    );
}

#[test]
fn release_urls_preserve_upstream_tag_names() {
    let (mut entry, _) = fixture();
    assert_eq!(
        entry.url(),
        "https://github.com/vscode-icons/vscode-icons/releases/download/v13.0.0/vscode-icons-13.0.0.vsix"
    );
    entry.release_tag = "13.0.0";
    assert_eq!(
        entry.url(),
        "https://github.com/vscode-icons/vscode-icons/releases/download/13.0.0/vscode-icons-13.0.0.vsix"
    );
}

fn source_fixture(doctype: &[u8]) -> (CatalogEntry, Vec<u8>) {
    let (mut entry, bytes) = fixture();
    let commit = "0123456789abcdef0123456789abcdef01234567";
    let mut source = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut files = Vec::new();
    for index in 0..source.len() {
        let mut file = source.by_index(index).unwrap();
        let name = format!(
            "theme-icons-0123456789abcdef0123456789abcdef01234567/{}",
            file.name().unwrap().strip_prefix("extension/").unwrap()
        );
        let mut content = Vec::new();
        file.read_to_end(&mut content).unwrap();
        if file.name().unwrap() == "extension/red.svg" {
            content = [doctype, content.as_slice()].concat();
        }
        files.push((name, content));
    }
    let contents: Vec<_> = files
        .iter()
        .map(|(name, data)| (name.as_str(), data.as_slice()))
        .collect();
    let bytes = archive(&contents);
    entry.repository = "example/theme-icons";
    entry.archive = Archive::GitHubSource {
        commit,
        standard_svg_doctypes: if doctype.is_empty() {
            &[]
        } else {
            &["red.svg"]
        },
    };
    entry.sha256 = Box::leak(hex::encode(Sha256::digest(&bytes)).into_boxed_str());
    (entry, bytes)
}

#[test]
fn pinned_source_archive_keeps_identity_rules_and_notices() {
    let (mut entry, bytes) = source_fixture(&[]);
    assert_eq!(
        entry.source_url(),
        "https://github.com/example/theme-icons/tree/0123456789abcdef0123456789abcdef01234567"
    );
    assert_eq!(
        entry.url(),
        "https://github.com/example/theme-icons/archive/0123456789abcdef0123456789abcdef01234567.zip"
    );
    let cache = tempfile::tempdir().unwrap();
    let packs = prepare(
        &entry,
        &bytes,
        cache.path(),
        &Progress::default(),
        Instant::now(),
    )
    .unwrap();
    assert_eq!(packs.len(), 1);
    assert_ne!(
        packs[0].theme.resolve("Cargo.toml"),
        packs[0].theme.resolve("normal.toml")
    );
    let metadata: serde_json::Value =
        serde_json::from_slice(&fs::read(&packs[0].provenance).unwrap()).unwrap();
    assert_eq!(
        metadata["download"]["url"],
        "https://github.com/example/theme-icons/archive/0123456789abcdef0123456789abcdef01234567.zip"
    );
    assert!(
        metadata["licenses"]["LICENSE.txt"]
            .as_str()
            .unwrap()
            .contains("Copyright fixture author")
    );
    entry.archive = Archive::Vsix("fixture.vsix");
    assert!(
        prepare(
            &entry,
            &bytes,
            cache.path(),
            &Progress::default(),
            Instant::now()
        )
        .is_err()
    );
    assert_eq!(fs::read_dir(cache.path()).unwrap().count(), 1);
}

#[test]
fn reviewed_doctype_normalization_preserves_artwork_without_enabling_dtds() {
    let standard = br#"<!DOCTYPE svg PUBLIC "-//W3C//DTD SVG 1.1//EN" "http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd">"#;
    let (mut entry, bytes) = source_fixture(standard);
    let cache = tempfile::tempdir().unwrap();
    let packs = prepare(
        &entry,
        &bytes,
        cache.path(),
        &Progress::default(),
        Instant::now(),
    )
    .unwrap();
    let baseline_cache = tempfile::tempdir().unwrap();
    let (baseline_entry, baseline_bytes) = fixture();
    let baseline = prepare(
        &baseline_entry,
        baseline_bytes,
        baseline_cache.path(),
        &Progress::default(),
        Instant::now(),
    )
    .unwrap();
    assert_eq!(
        fs::read(packs[0].theme.resolve("Cargo.toml").unwrap()).unwrap(),
        fs::read(baseline[0].theme.resolve("Cargo.toml").unwrap()).unwrap()
    );
    let record: serde_json::Value =
        serde_json::from_slice(&fs::read(&packs[0].provenance).unwrap()).unwrap();
    let mut input_archive = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
    let mut input = Vec::new();
    input_archive
        .by_name("theme-icons-0123456789abcdef0123456789abcdef01234567/red.svg")
        .unwrap()
        .read_to_end(&mut input)
        .unwrap();
    let normalization = &record["download"]["normalizations"][0];
    assert_eq!(normalization["path"], "red.svg");
    assert_eq!(normalization["operation"], "remove_standard_svg_doctype");
    assert_eq!(
        normalization["before_sha256"],
        hex::encode(Sha256::digest(&input))
    );
    assert_eq!(
        normalization["after_sha256"],
        hex::encode(Sha256::digest(&input[standard.len()..]))
    );

    entry.archive = Archive::GitHubSource {
        commit: "0123456789abcdef0123456789abcdef01234567",
        standard_svg_doctypes: &[],
    };
    assert!(
        prepare(
            &entry,
            &bytes,
            cache.path(),
            &Progress::default(),
            Instant::now()
        )
        .is_err()
    );
    let internal = br#"<!DOCTYPE svg [<!ENTITY value "unexpected">]>"#;
    for declaration in [
        internal.to_vec(),
        [standard.as_slice(), internal.as_slice()].concat(),
    ] {
        let (entry, bytes) = source_fixture(&declaration);
        assert!(
            prepare(
                &entry,
                &bytes,
                cache.path(),
                &Progress::default(),
                Instant::now()
            )
            .is_err()
        );
    }
    assert_eq!(fs::read_dir(cache.path()).unwrap().count(), 1);
}

#[test]
fn verified_download_is_immutable_and_keeps_provenance() {
    let cache = tempfile::tempdir().unwrap();
    let (entry, bytes) = fixture();
    let progress = Progress::default();
    let packs = prepare(&entry, bytes, cache.path(), &progress, Instant::now()).unwrap();
    let converted = progress.conversion();
    assert_eq!(
        (
            converted.theme,
            converted.themes,
            converted.completed,
            converted.total
        ),
        (1, 1, 2, 2)
    );
    let pack = &packs[0];
    assert_eq!(pack.id, entry.pack_id("vscode-icons"));
    assert_ne!(
        pack.theme.resolve("Cargo.toml"),
        pack.theme.resolve("settings.toml")
    );
    let receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(&pack.provenance).unwrap()).unwrap();
    assert_eq!(receipt["download"]["sha256"], entry.sha256);
    assert!(
        receipt["licenses"]["LICENSE.txt"]
            .as_str()
            .unwrap()
            .contains("Copyright fixture author")
    );
    let again = prepare(
        &entry,
        bytes,
        cache.path(),
        &Progress::default(),
        Instant::now(),
    )
    .unwrap();
    assert_eq!(again[0].provenance, pack.provenance);
    assert_eq!(fs::read_dir(cache.path()).unwrap().count(), 1);
}

#[test]
fn integrity_identity_and_cancellation_fail_before_publishing() {
    let cache = tempfile::tempdir().unwrap();
    let (mut entry, bytes) = fixture();
    let mut changed = bytes.to_vec();
    changed[20] ^= 1;
    assert!(
        prepare(
            &entry,
            &changed,
            cache.path(),
            &Progress::default(),
            Instant::now()
        )
        .unwrap_err()
        .to_string()
        .contains("SHA-256")
    );
    entry.publisher = "different-author";
    assert!(
        prepare(
            &entry,
            bytes,
            cache.path(),
            &Progress::default(),
            Instant::now()
        )
        .unwrap_err()
        .to_string()
        .contains("identity")
    );
    let progress = Progress::default();
    progress.cancel();
    assert!(
        prepare(&entry, bytes, cache.path(), &progress, Instant::now())
            .unwrap_err()
            .is::<Cancelled>()
    );
    assert_eq!(fs::read_dir(cache.path()).unwrap().count(), 0);
    assert!(
        bounded_copy(
            Cursor::new(b"12345"),
            Vec::new(),
            4,
            &Progress::default(),
            Instant::now(),
            false
        )
        .is_err()
    );
    assert!(
        bounded_copy(
            Cursor::new(b"x"),
            Vec::new(),
            4,
            &Progress::default(),
            Instant::now() - DEADLINE,
            false
        )
        .is_err()
    );
}

#[test]
fn archives_cannot_escape_overwrite_or_allocate_unbounded_output() {
    let workspace = tempfile::tempdir().unwrap();
    let outside = workspace.path().join("keep");
    fs::write(&outside, b"unchanged").unwrap();
    let target = workspace.path().join("extract");
    fs::create_dir(&target).unwrap();
    for name in [
        "../keep",
        "/keep",
        "C:/keep",
        "extension/../../keep",
        "extension/a:b",
        "extension/CON.txt",
        "extension/name.",
        "extension/a\\b",
        "extension/LPT1",
        "extension/a\0b",
    ] {
        let zip = archive(&[(name, b"replace")]);
        assert!(
            extract(&zip, &target, &Progress::default(), Instant::now()).is_err(),
            "accepted {name:?}"
        );
        assert_eq!(fs::read(&outside).unwrap(), b"unchanged");
    }
    let dup = archive(&[("extension/a", b"first"), ("extension/A", b"second")]);
    assert!(extract(&dup, &target, &Progress::default(), Instant::now()).is_err());
    assert_eq!(fs::read(target.join("extension/a")).unwrap(), b"first");
    let huge = archive(&[("extension/huge", &vec![0; MAX_FILE as usize + 1])]);
    assert!(extract(&huge, &target, &Progress::default(), Instant::now()).is_err());
    assert!(!target.join("extension/huge").exists());
    let bomb = archive(&[("extension/bomb", &vec![0; MAX_FILE as usize])]);
    assert!(extract(&bomb, &target, &Progress::default(), Instant::now()).is_err());
    assert!(!target.join("extension/bomb").exists());
    let mut zip = archive(&[("small", b"x")]);
    let end = zip.len() - 22;
    zip[end + 8..end + 12].copy_from_slice(&[0xff, 0xff, 0xff, 0xff]);
    assert!(preflight(&zip).is_err());
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .add_symlink(
            "extension/link",
            "../../keep",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
    let linked = writer.finish().unwrap().into_inner();
    assert!(extract(&linked, &target, &Progress::default(), Instant::now()).is_err());
    assert!(!target.join("extension/link").exists());
}

#[test]
fn reserved_archive_components_are_rejected_before_filesystem_access() {
    let target = tempfile::tempdir().unwrap();
    // 判定が退行しても実デバイスへ到達しないよう、親の位置を通常ファイルで塞ぐ。
    let barrier = target.path().join("extension");
    fs::write(&barrier, b"unchanged").unwrap();
    for name in [
        "extension/CON .txt",
        "extension/NUL .txt",
        "extension/con .TXT",
        "extension/CoN   .json",
        "extension/PRN .log",
        "extension/AUX .svg",
        "extension/CONIN$ .txt",
        "extension/CONOUT$ .txt",
        "extension/COM1 .txt",
        "extension/LPT1 .txt",
        "extension/COM¹ .txt",
        "extension/CON.txt",
        "extension/LPT1",
        "extension/CON .data/icon.svg",
        "extension/art/NUL .data/icon.svg",
        "extension/AUX .data/",
    ] {
        let bytes = archive(&[(name, b"replace")]);
        let error =
            extract(&bytes, target.path(), &Progress::default(), Instant::now()).unwrap_err();
        assert!(
            error.to_string().contains("unsafe archive path"),
            "{name:?}: {error:#}"
        );
        assert_eq!(fs::read(&barrier).unwrap(), b"unchanged");
        assert_eq!(fs::read_dir(target.path()).unwrap().count(), 1);
    }
}

#[test]
fn ordinary_archive_names_keep_their_original_spelling() {
    let names = [
        "extension/console.txt",
        "extension/auxiliary.json",
        "extension/null.md",
        "extension/asset name.svg",
        "extension/console .svg",
        "extension/art work/日本語.svg",
    ];
    let files = names.map(|name| (name, b"artwork".as_slice()));
    let bytes = archive(&files);
    let target = tempfile::tempdir().unwrap();
    extract(&bytes, target.path(), &Progress::default(), Instant::now()).unwrap();
    for name in names {
        assert_eq!(fs::read(target.path().join(name)).unwrap(), b"artwork");
    }
}

#[test]
fn decoded_archive_names_preserve_unicode_and_still_reject_unsafe_paths() {
    let target = tempfile::tempdir().unwrap();
    let unicode = archive(&[("extension/日本語.svg", b"unicode artwork")]);
    extract(
        &unicode,
        target.path(),
        &Progress::default(),
        Instant::now(),
    )
    .unwrap();
    assert_eq!(
        fs::read(target.path().join("extension/日本語.svg")).unwrap(),
        b"unicode artwork"
    );

    // ZIPの従来の文字コードCP437で記録された名前も、復号後に同じパス検査を通す。
    for (name, expected) in [
        ("extension/cafX.svg", Some("extension/café.svg")),
        ("extension/cafX:icon.svg", None),
    ] {
        let target = tempfile::tempdir().unwrap();
        let mut bytes = archive(&[(name, b"legacy artwork")]);
        let end = bytes.len() - 22;
        let central = u32::from_le_bytes(bytes[end + 16..end + 20].try_into().unwrap()) as usize;
        let letter = name.find('X').unwrap();
        for (header, length, flags) in [(0, 30, 6), (central, 46, 8)] {
            bytes[header + length + letter] = 0x82;
            let offset = header + flags;
            let original = u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap());
            bytes[offset..offset + 2].copy_from_slice(&(original & !0x0800).to_le_bytes());
        }
        let result = extract(&bytes, target.path(), &Progress::default(), Instant::now());
        if let Some(expected) = expected {
            result.unwrap();
            assert_eq!(
                fs::read(target.path().join(expected)).unwrap(),
                b"legacy artwork"
            );
        } else {
            assert!(result.is_err());
            assert_eq!(fs::read_dir(target.path()).unwrap().count(), 0);
        }
    }
}

#[test]
fn destination_policy_rejects_credentials_ports_and_foreign_hosts() {
    for url in [
        "http://github.com/a",
        "https://github.com.evil.test/a",
        "https://127.0.0.1/a",
        "https://github.com:8443/a",
        "https://user:pass@github.com/a",
        "https://example.org/a",
        "https://codeload.github.com.evil.test/a",
        "https://codeload.github.com:8443/a",
    ] {
        assert!(!approved_url(&reqwest::Url::parse(url).unwrap()));
    }
    assert!(approved_url(
        &reqwest::Url::parse("https://release-assets.githubusercontent.com/a").unwrap()
    ));
    assert!(approved_url(
        &reqwest::Url::parse("https://codeload.github.com/example/theme-icons/zip/commit").unwrap()
    ));
}

#[test]
#[ignore = "公式配布元への通信を伴う受け入れ確認"]
fn official_releases_download_and_compile() {
    let cache = tempfile::tempdir().unwrap();
    for entry in CATALOG {
        let packs = download(entry.id, cache.path(), Arc::new(Progress::default())).unwrap();
        assert_eq!(packs.len(), entry.themes.len());
        for pack in packs {
            assert!(pack.theme.resolve("sample.json").is_some(), "{}", pack.id);
            let metadata: serde_json::Value =
                serde_json::from_slice(&fs::read(pack.provenance).unwrap()).unwrap();
            assert_eq!(metadata["download"]["sha256"], entry.sha256);
            assert!(!metadata["licenses"].as_object().unwrap().is_empty());
        }
    }
}

#[test]
fn total_output_and_forged_directory_counts_are_rejected_before_extraction() {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..17 {
        writer
            .start_file(
                format!("extension/file{i}"),
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        writer.write_all(b"x").unwrap();
    }
    let original = writer.finish().unwrap().into_inner();
    let end = original.len() - 22;
    let central = u32::from_le_bytes(original[end + 16..end + 20].try_into().unwrap()) as usize;
    let mut oversized = original.clone();
    let mut offset = central;
    for _ in 0..17 {
        oversized[offset + 24..offset + 28].copy_from_slice(&(8u32 * 1024 * 1024).to_le_bytes());
        let short =
            |i| u16::from_le_bytes([original[offset + i], original[offset + i + 1]]) as usize;
        offset += 46 + short(28) + short(30) + short(32);
    }
    let target = tempfile::tempdir().unwrap();
    assert!(
        extract(
            &oversized,
            target.path(),
            &Progress::default(),
            Instant::now()
        )
        .unwrap_err()
        .to_string()
        .contains("expanded pack too large")
    );
    assert_eq!(fs::read_dir(target.path()).unwrap().count(), 0);
    let mut forged = original;
    forged[end + 8..end + 12].copy_from_slice(&[1, 0, 1, 0]);
    assert!(preflight(&forged).is_err());
}
