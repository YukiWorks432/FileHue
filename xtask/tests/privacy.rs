use std::{fs, path::PathBuf, process::Command};

#[test]
fn binary_scan_detects_path_variants_including_unaligned_utf16() {
    let prefixes = vec!["Q:/build/source-tree".to_owned()];
    for path in [
        "q:\\BUILD\\source-tree\\main.rs",
        "Q:/build/source-tree/main.rs",
        "D:\\Users\\build-owner\\toolchain\\library.rs",
    ] {
        let mut bytes = vec![0xff, 0x88, 0x00];
        bytes.extend_from_slice(path.as_bytes());
        assert!(xtask::privacy::inspect(&bytes, &prefixes).is_err());
        let mut bytes = vec![0xff];
        bytes.extend(path.encode_utf16().flat_map(u16::to_le_bytes));
        assert!(xtask::privacy::inspect(&bytes, &prefixes).is_err());
    }
    assert!(
        xtask::privacy::inspect(
            b"MZ\0/build/source/main.rs\0C:\\Windows\\System32\\shell32.dll",
            &prefixes
        )
        .is_ok()
    );
}

#[test]
fn remapping_changes_compiler_embedded_locations_in_an_actual_executable() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("example.rs");
    let executable = temp.path().join("example.exe");
    fs::write(&source, "fn main() { println!(\"{}\", file!()); }").unwrap();
    let mappings = vec![(PathBuf::from(temp.path()), "/build/source")];
    let flags = xtask::privacy::flags(&mappings, &[]).unwrap();
    let status = Command::new("rustc")
        .arg(&source)
        .arg("-o")
        .arg(&executable)
        .args(flags.split('\x1f'))
        .status()
        .unwrap();
    assert!(status.success());
    let output = Command::new(&executable).output().unwrap();
    assert!(output.status.success());
    let location = String::from_utf8(output.stdout).unwrap().replace('\\', "/");
    assert_eq!(location.trim(), "/build/source/example.rs");
    xtask::privacy::inspect(
        &fs::read(executable).unwrap(),
        &[temp.path().to_string_lossy().into_owned()],
    )
    .unwrap();
}
