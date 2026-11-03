use serde_json::json;
use std::{fs, path::Path};

fn metadata(manifest: &Path, license: &str) -> serde_json::Value {
    json!({"packages":[{"id":"app","name":"file-icons-manager","version":"1","source":null},{"id":"fixture","name":"fixture","version":"1","license":license,"authors":["Author"],"repository":null,"source":"registry+https://github.com/rust-lang/crates.io-index","manifest_path":manifest}],
        "resolve":{"nodes":[{"id":"app","deps":[{"pkg":"fixture","dep_kinds":[{"kind":null}]}]},{"id":"fixture","deps":[]}]}})
}

#[test]
fn supplementary_license_is_copied_exactly_and_tampering_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join("licenses/upstream")).unwrap();
    fs::create_dir(root.join("fixture")).unwrap();
    fs::write(root.join("Cargo.lock"), "version=4\npackage=[]").unwrap();
    fs::write(root.join("fixture/Cargo.toml"), "[package]\nname='fixture'").unwrap();
    fs::write(root.join("licenses/upstream/original.txt"), b"abc").unwrap();
    fs::write(root.join("licenses/upstream/manifest.json"),json!([{"package":"fixture","version":"1","file":"original.txt","url":"https://example.invalid/original","sha256":"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"}]).to_string()).unwrap();
    let metadata = metadata(&root.join("fixture/Cargo.toml"), "MIT");
    let output = root.join("notices");
    let records = xtask::package::licenses(root, &metadata, &output).unwrap();
    assert_eq!(records[0]["review_required"], false);
    assert_eq!(
        fs::read(output.join("fixture-1/UPSTREAM-LICENSE.txt")).unwrap(),
        b"abc"
    );
    fs::write(root.join("licenses/upstream/original.txt"), b"abd").unwrap();
    assert!(xtask::package::licenses(root, &metadata, &root.join("tampered")).is_err());
}

#[test]
fn mpl_archive_requires_the_exact_locked_source_and_retains_original_notices() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let package = root.join("registry/src/example/fixture-1");
    let cache = root.join("registry/cache/example");
    fs::create_dir_all(&package).unwrap();
    fs::create_dir_all(&cache).unwrap();
    fs::create_dir_all(root.join("licenses/upstream")).unwrap();
    fs::write(root.join("licenses/upstream/manifest.json"), "[]").unwrap();
    fs::write(package.join("Cargo.toml"), "[package]\nname='fixture'").unwrap();
    fs::write(package.join("LICENSE"), "Original MPL notice").unwrap();
    fs::write(root.join("Cargo.lock"),"version=4\n[[package]]\nname='fixture'\nversion='1'\nchecksum='ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad'").unwrap();
    let metadata = metadata(&package.join("Cargo.toml"), "MPL-2.0");
    assert!(xtask::package::licenses(root, &metadata, &root.join("missing")).is_err());
    fs::write(cache.join("fixture-1.crate"), b"abc").unwrap();
    let records = xtask::package::licenses(root, &metadata, &root.join("good")).unwrap();
    assert_eq!(
        records[0]["source_archive"]["sha256"],
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        fs::read(root.join("good/fixture-1/fixture-1.crate")).unwrap(),
        b"abc"
    );
    assert_eq!(
        fs::read_to_string(root.join("good/fixture-1/LICENSE")).unwrap(),
        "Original MPL notice"
    );
    fs::write(cache.join("fixture-1.crate"), b"abd").unwrap();
    assert!(xtask::package::licenses(root, &metadata, &root.join("bad")).is_err());
}

#[test]
fn multiple_supplements_keep_attribution_evidence_and_original_declarations() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join("licenses/upstream")).unwrap();
    fs::create_dir(root.join("fixture")).unwrap();
    fs::write(root.join("Cargo.lock"), "version=4\npackage=[]").unwrap();
    fs::write(root.join("fixture/Cargo.toml"), "original declaration").unwrap();
    fs::write(
        root.join("fixture/README.md"),
        "original author attribution",
    )
    .unwrap();
    for file in ["terms.txt", "authors.txt"] {
        fs::write(root.join("licenses/upstream").join(file), b"abc").unwrap();
    }
    let mut entries = json!([
        {"package":"fixture","version":"1","file":"terms.txt","destination":"APACHE.txt","selected_license":"Apache-2.0","review_reason":"upstream confirmation pending","sha256":"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"},
        {"package":"fixture","version":"1","file":"authors.txt","destination":"AUTHORS.txt","kind":"attribution","sha256":"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"}
    ]);
    let manifest = root.join("licenses/upstream/manifest.json");
    fs::write(&manifest, entries.to_string()).unwrap();
    let metadata = metadata(&root.join("fixture/Cargo.toml"), "MIT OR Apache-2.0");
    let output = root.join("good");
    let records = xtask::package::licenses(root, &metadata, &output).unwrap();
    assert_eq!(records[0]["selected_license"], "Apache-2.0");
    assert_eq!(records[0]["review_required"], true);
    for file in ["APACHE.txt", "AUTHORS.txt"] {
        assert_eq!(
            fs::read(output.join("fixture-1").join(file)).unwrap(),
            b"abc"
        );
    }
    assert_eq!(
        fs::read_to_string(output.join("fixture-1/README.md")).unwrap(),
        "original author attribution"
    );
    fs::write(root.join("licenses/upstream/authors.txt"), b"abd").unwrap();
    assert!(xtask::package::licenses(root, &metadata, &root.join("tampered")).is_err());
    fs::write(root.join("licenses/upstream/authors.txt"), b"abc").unwrap();
    entries[1]["destination"] = json!("apache.TXT");
    fs::write(&manifest, entries.to_string()).unwrap();
    assert!(xtask::package::licenses(root, &metadata, &root.join("collision")).is_err());
    entries[1]["destination"] = json!("../escaped.txt");
    fs::write(&manifest, entries.to_string()).unwrap();
    assert!(xtask::package::licenses(root, &metadata, &root.join("escape")).is_err());
    entries.as_array_mut().unwrap().remove(0);
    entries[0]["destination"] = json!("AUTHORS.txt");
    fs::write(&manifest, entries.to_string()).unwrap();
    let records =
        xtask::package::licenses(root, &metadata, &root.join("attribution-only")).unwrap();
    assert_eq!(records[0]["review_required"], true);
}

#[test]
fn about_output_is_bound_to_package_ids_and_retains_both_and_obligations() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("fixture-1")).unwrap();
    fs::write(temp.path().join("fixture-1/LICENSE"), "original copyright").unwrap();
    let metadata = json!({"packages":[
        {"id":"app","name":"file-icons-manager","version":"1","source":null},
        {"id":"wanted","name":"fixture","version":"1","source":"registry"},
        {"id":"other-feature","name":"optional","version":"1","source":"registry"}],
        "resolve":{"nodes":[{"id":"app","deps":[{"pkg":"wanted","dep_kinds":[{"kind":null}]},{"pkg":"other-feature","dep_kinds":[{"kind":null}]}]},{"id":"wanted","deps":[]},{"id":"other-feature","deps":[]}]}});
    let raw = json!({"crates":[{"package":{"id":"wanted"}}],"licenses":[
        {"id":"Apache-2.0","name":"Apache","text":"terms <script>example</script>","source_path":"C:/Users/private/LICENSE","used_by":[{"crate":{"id":"wanted","manifest_path":"C:/Users/private/Cargo.toml"}}]},
        {"id":"MIT","name":"MIT","text":"other required terms","used_by":[{"crate":{"id":"wanted"}}]},
        {"id":"MIT","name":"MIT","text":"unrelated older version","used_by":[{"crate":{"id":"wrong-version","name":"fixture","version":"0"}}]}]});
    let mut records = vec![
        json!({"name":"fixture","version":"1","selected_license":"Apache-2.0","files":[{"path":"fixture-1/LICENSE","sha256":xtask::package::digest(&temp.path().join("fixture-1/LICENSE")).unwrap()}],"review_required":true}),
        json!({"name":"optional","version":"1","files":[]}),
    ];
    let report = xtask::notices::normalize(&metadata, &raw, &mut records).unwrap();
    assert_eq!(records[0]["about_licenses"], json!(["Apache-2.0", "MIT"]));
    assert_eq!(records[1]["selection_status"], "supplement-only");
    assert_eq!(report["licenses"].as_array().unwrap().len(), 2);
    assert!(!report.to_string().contains("C:/Users/private"));
    let html = xtask::notices::html(&report, &records, temp.path()).unwrap();
    assert!(html.contains("&lt;script&gt;example&lt;/script&gt;"));
    assert!(!html.contains("<script>"));
    assert!(html.contains("fixture-1/LICENSE"));
    assert!(html.contains("Review required"));
    records[0]["selected_license"] = json!("ISC");
    assert!(xtask::notices::normalize(&metadata, &raw, &mut records).is_err());
    records[0]["selected_license"] = json!("Apache-2.0");
    let mut broken = raw.clone();
    broken["licenses"] = json!([]);
    assert!(xtask::notices::normalize(&metadata, &broken, &mut records).is_err());
    broken = raw;
    broken["crates"]
        .as_array_mut()
        .unwrap()
        .push(json!({"package":{"id":"other-feature"}}));
    assert!(xtask::notices::normalize(&metadata, &broken, &mut records).is_err());
}

#[test]
fn identical_originals_are_shared_without_losing_names_or_distinct_attributions() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("input");
    let output = temp.path().join("output");
    for name in [
        "one/LICENSE",
        "two/LICENSE",
        "two/COPYRIGHT",
        "two/source.crate",
    ] {
        let path = source.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let bytes: &[u8] = match name {
            "one/LICENSE" | "two/LICENSE" => b"Shared terms",
            "two/COPYRIGHT" => b"Copyright Example",
            _ => b"Original source",
        };
        fs::write(path, bytes).unwrap();
    }
    let file = |path: &str| json!({"path":path,"sha256":xtask::package::digest(&source.join(path)).unwrap()});
    let mut records = vec![
        json!({"name":"one","version":"1","files":[file("one/LICENSE")]}),
        json!({"name":"two","version":"1","files":[file("two/LICENSE"),file("two/COPYRIGHT")],"source_archive":file("two/source.crate")}),
    ];
    xtask::notices::copy_sources(&source, &output, &mut records).unwrap();
    assert_eq!(
        fs::read(output.join(records[1]["source_archive"]["path"].as_str().unwrap())).unwrap(),
        b"Original source"
    );
    let report = json!({"licenses":[]});
    let html = xtask::notices::html(&report, &records, &source).unwrap();
    assert!(html.contains("one/LICENSE"));
    assert!(html.contains("two/LICENSE"));
    assert!(html.contains("two/COPYRIGHT"));
    assert!(html.contains("two/source.crate"));
    assert!(html.contains("https://static.crates.io/crates/two/two-1.crate"));
    assert!(html.contains(records[1]["source_archive"]["sha256"].as_str().unwrap()));
    assert_eq!(html.matches("<pre>Shared terms</pre>").count(), 1);
    assert!(html.contains("<pre>Copyright Example</pre>"));
    fs::write(output.join("THIRD_PARTY_LICENSES.html"), &html).unwrap();
    xtask::notices::verify_texts(&output, &records, &report).unwrap();
    fs::write(
        output.join("THIRD_PARTY_LICENSES.html"),
        html.replace(">two/COPYRIGHT</a>", ">missing reference</a>"),
    )
    .unwrap();
    assert!(xtask::notices::verify_texts(&output, &records, &report).is_err());
    fs::write(
        output.join("THIRD_PARTY_LICENSES.html"),
        html.replace(
            records[1]["source_archive"]["sha256"].as_str().unwrap(),
            "wrong checksum",
        ),
    )
    .unwrap();
    assert!(xtask::notices::verify_texts(&output, &records, &report).is_err());
    fs::write(
        output.join("THIRD_PARTY_LICENSES.html"),
        html.replace(">two/source.crate</a>", ">missing source reference</a>"),
    )
    .unwrap();
    assert!(xtask::notices::verify_texts(&output, &records, &report).is_err());
    fs::write(
        output.join("THIRD_PARTY_LICENSES.html"),
        html.replace("Copyright Example", "Copyright Missing"),
    )
    .unwrap();
    assert!(xtask::notices::verify_texts(&output, &records, &report).is_err());
}

#[test]
fn acceptance_is_bound_to_the_exact_crate_and_all_supplement_records() {
    use sha2::{Digest, Sha256};
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let package = root.join("registry/src/example/fixture-1");
    fs::create_dir_all(&package).unwrap();
    fs::create_dir_all(root.join("registry/cache/example")).unwrap();
    fs::create_dir_all(root.join("licenses/upstream")).unwrap();
    fs::write(package.join("Cargo.toml"), "original declaration").unwrap();
    fs::write(root.join("registry/cache/example/fixture-1.crate"), b"abc").unwrap();
    let checksum = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    fs::write(
        root.join("Cargo.lock"),
        format!("version=4\n[[package]]\nname='fixture'\nversion='1'\nchecksum='{checksum}'"),
    )
    .unwrap();
    fs::write(root.join("licenses/upstream/terms.txt"), b"abc").unwrap();
    fs::write(root.join("licenses/upstream/authors.txt"), b"abc").unwrap();
    let mut extras = json!([
        {"package":"fixture","version":"1","file":"terms.txt","sha256":checksum,"crate_checksum":checksum,"review_reason":"upstream uncertainty","destination":"TERMS.txt"},
        {"package":"fixture","version":"1","file":"authors.txt","sha256":checksum,"kind":"attribution","destination":"AUTHORS.txt"}
    ]);
    let evidence_hash = hex::encode(Sha256::digest(serde_json::to_vec(&extras).unwrap()));
    extras[0]["acceptance"] = json!({"decided_on":"2026-10-25","decided_by":"Fixture maintainer","reason":"Accepted current evidence","crate_checksum":checksum,"supplements_sha256":evidence_hash});
    let manifest = root.join("licenses/upstream/manifest.json");
    fs::write(&manifest, extras.to_string()).unwrap();
    let metadata = metadata(&package.join("Cargo.toml"), "Apache-2.0");
    let records = xtask::package::licenses(root, &metadata, &root.join("accepted")).unwrap();
    assert_eq!(records[0]["review_required"], false);
    assert_eq!(records[0]["review_reason"], "upstream uncertainty");
    assert_eq!(records[0]["acceptance"]["decided_by"], "Fixture maintainer");
    assert_eq!(
        fs::read(root.join("accepted/fixture-1/AUTHORS.txt")).unwrap(),
        b"abc"
    );
    // 本文が同じでも出典・対象版・採用対象の変更は元の判断へ流用しない。
    for (field, bad) in [
        ("version", "2"),
        ("sha256", "different"),
        ("url", "https://example.invalid/new-origin"),
    ] {
        let mut changed = extras.clone();
        changed[1][field] = json!(bad);
        fs::write(&manifest, changed.to_string()).unwrap();
        assert!(
            xtask::package::licenses(root, &metadata, &root.join(field)).is_err(),
            "{field}"
        );
    }
    let mut changed = extras;
    changed[0]["acceptance"]["crate_checksum"] = json!("different");
    fs::write(&manifest, changed.to_string()).unwrap();
    assert!(xtask::package::licenses(root, &metadata, &root.join("wrong-crate")).is_err());
}
