use serde_json::{Value, json};
use std::{fs, path::Path};
use xtask::package::{self, Purpose};

pub fn payload(root: &Path, purpose: Purpose) -> Value {
    let mut pe = vec![0u8; 328];
    pe[..2].copy_from_slice(b"MZ");
    pe[60..64].copy_from_slice(&64u32.to_le_bytes());
    pe[64..68].copy_from_slice(b"PE\0\0");
    pe[68..70].copy_from_slice(&0x8664u16.to_le_bytes());
    pe[84..86].copy_from_slice(&240u16.to_le_bytes());
    pe[88..90].copy_from_slice(&0x20bu16.to_le_bytes());
    pe[196..200].copy_from_slice(&16u32.to_le_bytes());
    for name in package::BINARIES.iter().chain(package::PAYLOAD).chain(&[
        "licenses/THIRD_PARTY_LICENSES.html",
        "licenses/rust/COPYRIGHT-library.html",
        "licenses/VSCODE-LICENSE.txt",
    ]) {
        let path = root.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            if package::BINARIES.contains(name) {
                pe.as_slice()
            } else {
                b"abc"
            },
        )
        .unwrap();
    }
    fs::write(root.join("licenses/THIRD_PARTY_LICENSES.html"),
        "<a href=\"rust/COPYRIGHT-library.html\">Rust</a><a href=\"VSCODE-LICENSE.txt\">VS Code</a>").unwrap();
    let metadata = root.parent().unwrap();
    fs::write(metadata.join("dependencies.json"), "[]").unwrap();
    fs::write(metadata.join("about.json"), "{\"licenses\":[]}").unwrap();
    let entries: Vec<_> = package::files(root).unwrap().iter().map(|p| {
        let file = root.join(p);
        json!({"path":p.to_string_lossy().replace('\\', "/"),"size":file.metadata().unwrap().len(),"sha256":package::digest(&file).unwrap()})
    }).collect();
    let manifest = json!({"format":5,"product":"FileHue","purpose":purpose,"version":"0.1.0",
        "target":"x86_64-pc-windows-msvc","profile":"release","signature":"unsigned",
        "source_repository":if purpose == Purpose::PublicCandidate {Some("https://github.com/YukiWorks432/filehue")} else {None},
        "source_commit":"0123456789abcdef0123456789abcdef01234567","source_dirty":false,
        "source_files_sha256":"fixture source hash","cargo_lock_sha256":"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        "license_records_sha256":package::digest(&metadata.join("dependencies.json")).unwrap(),
        "about_sha256":package::digest(&metadata.join("about.json")).unwrap(),
        "rustc":"rustc fixture","license_review_required":[],"publication_approved":false,"files":entries});
    fs::write(
        metadata.join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    manifest
}
