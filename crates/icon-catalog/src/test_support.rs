//! 通信を置換したGUI試験でも、検証・展開・変換は実処理を通す。
use super::*;
use std::sync::LazyLock;
use zip::write::SimpleFileOptions;

pub fn archive(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in files {
        zip.start_file(
            *name,
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
        )
        .unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
static FIXTURE: LazyLock<Vec<u8>> = LazyLock::new(|| {
    archive(&[
    ("extension/package.json", br#"{"name":"vscode-icons","publisher":"vscode-icons-team","displayName":"Fixture icons","license":"MIT","version":"13.0.0","contributes":{"iconThemes":[{"id":"vscode-icons","label":"Fixture","path":"theme.json"}]}}"#),
    ("extension/theme.json", br#"{"iconDefinitions":{"red":{"iconPath":"red.svg"},"green":{"iconPath":"green.svg"}},"fileNames":{"Cargo.toml":"red","package.json":"red","README.md":"red"},"fileExtensions":{"toml":"green","json":"green","md":"green","broken":"undefined"}}"#),
    ("extension/red.svg", br##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="16" height="16" fill="#ff0000"/></svg>"##),
    ("extension/green.svg", br##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="16" height="16" fill="#00ff00"/></svg>"##),
    ("extension/LICENSE.txt", b"MIT License\nCopyright fixture author\nPermission is hereby granted for this test fixture."),
    ("extension/README.md", b"Original fixture artwork; no third-party artwork is included."),
])
});
static HASH: LazyLock<String> = LazyLock::new(|| hex::encode(Sha256::digest(&*FIXTURE)));

pub fn fixture() -> (CatalogEntry, &'static [u8]) {
    let actual = &CATALOG[0];
    (
        CatalogEntry {
            id: actual.id,
            name: "Fixture icons",
            author: "fixture author",
            version: actual.version,
            repository: actual.repository,
            release_tag: actual.release_tag,
            archive: Archive::Vsix("vscode-icons-13.0.0.vsix"),
            sha256: &HASH,
            publisher: actual.publisher,
            package: actual.package,
            themes: actual.themes,
            license: "MIT",
        },
        &FIXTURE,
    )
}
pub fn download_fixture(
    _: &str,
    cache: &Path,
    progress: Arc<Progress>,
) -> Result<Vec<ImportedPack>> {
    let (entry, bytes) = fixture();
    progress.bytes.store(bytes.len() as u64, Ordering::Relaxed);
    prepare(&entry, bytes, cache, &progress, Instant::now())
}
