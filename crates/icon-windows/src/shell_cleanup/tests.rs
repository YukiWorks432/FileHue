use super::*;
use file_icons_core::registry::Change;

const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
fn fixture(base: &Path) -> PathBuf {
    let path = base.join("shell").join(ABC).join("file_icons_shell.dll");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, b"abc").unwrap();
    path
}

#[test]
fn cleanup_preserves_registration_unknown_changed_and_locked_files() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("distribution.dll");
    fs::write(&source, b"original distribution").unwrap();
    let base = root.path().join("home");
    let dll = fixture(&base);
    let mut settings = Settings::default();
    assert!(cleanup_at(&settings, &base, true).is_err());
    assert!(dll.is_file());
    settings.active = Some("active/theme".into());
    assert!(cleanup_at(&settings, &base, false).is_err());
    assert!(dll.is_file());
    settings.active = None;
    settings.journal.push(Change {
        key: "Software\\Classes\\Test".into(),
        name: "".into(),
        before: None,
        after: file_icons_core::registry::Value {
            kind: 1,
            bytes: vec![0, 0],
        },
    });
    assert!(cleanup_at(&settings, &base, false).is_err());
    assert!(dll.is_file());
    settings.journal.clear();
    let note = dll.parent().unwrap().join("notes.txt");
    fs::write(&note, b"user notes").unwrap();
    assert!(cleanup_at(&settings, &base, false).is_err());
    assert_eq!(fs::read(&note).unwrap(), b"user notes");
    assert!(dll.is_file());
    fs::remove_file(note).unwrap();
    fs::write(&dll, b"changed").unwrap();
    assert!(cleanup_at(&settings, &base, false).is_err());
    assert_eq!(fs::read(&dll).unwrap(), b"changed");
    fs::write(&dll, b"abc").unwrap();
    let locked = fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ.0)
        .open(&dll)
        .unwrap();
    assert!(cleanup_at(&settings, &base, false).is_err());
    assert!(dll.is_file());
    drop(locked);
    cleanup_at(&settings, &base, false).unwrap();
    assert!(!dll.parent().unwrap().exists());
    assert_eq!(fs::read(&source).unwrap(), b"original distribution");
    cleanup_at(&settings, &base, false).unwrap();
}

#[test]
fn thumbnail_cleanup_verifies_owned_content_and_preserves_icon_storage() {
    let root = tempfile::tempdir().unwrap();
    let base = root.path().join("home");
    let icon = fixture(&base);
    let thumbnail = base
        .join("thumbnail-shell")
        .join(ABC)
        .join("file_icons_thumbnail_shell.dll");
    fs::create_dir_all(thumbnail.parent().unwrap()).unwrap();
    fs::write(&thumbnail, b"changed").unwrap();
    assert!(cleanup_store(&base, "thumbnail-shell", "file_icons_thumbnail_shell.dll").is_err());
    assert_eq!(fs::read(&thumbnail).unwrap(), b"changed");
    fs::write(&thumbnail, b"abc").unwrap();
    cleanup_store(&base, "thumbnail-shell", "file_icons_thumbnail_shell.dll").unwrap();
    assert!(!thumbnail.exists());
    assert_eq!(fs::read(icon).unwrap(), b"abc");
}

#[test]
fn cleanup_never_follows_redirected_storage_and_limits_work() {
    let root = tempfile::tempdir().unwrap();
    let base = root.path().join("home");
    let dll = fixture(&base);
    let external = root.path().join("external");
    fs::create_dir(&external).unwrap();
    let external_file = external.join("file_icons_shell.dll");
    fs::write(&external_file, b"abc").unwrap();
    // ハードリンクの場合も、管理領域外の名前や内容を削除しない。
    fs::remove_file(&dll).unwrap();
    fs::hard_link(&external_file, &dll).unwrap();
    let _ = cleanup_at(&Settings::default(), &base, false);
    assert_eq!(fs::read(&external_file).unwrap(), b"abc");
    // シンボリックリンクを作れる環境では、親とファイルの両方の転送を拒否する。
    let link_base = root.path().join("linked-home");
    fs::create_dir(&link_base).unwrap();
    if std::os::windows::fs::symlink_dir(&external, link_base.join("shell")).is_ok() {
        eprintln!("directory symlink refusal checked");
        assert!(cleanup_at(&Settings::default(), &link_base, false).is_err());
        assert_eq!(fs::read(&external_file).unwrap(), b"abc");
    }
    let file_base = root.path().join("file-home");
    let target = file_base
        .join("shell")
        .join(ABC)
        .join("file_icons_shell.dll");
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    if std::os::windows::fs::symlink_file(&external_file, &target).is_ok() {
        eprintln!("file symlink refusal checked");
        assert!(cleanup_at(&Settings::default(), &file_base, false).is_err());
        assert_eq!(fs::read(&external_file).unwrap(), b"abc");
    }
    let limits = root.path().join("limit-home");
    let file = fixture(&limits);
    let actual = file.canonicalize().unwrap();
    assert!(delete_verified(&actual, ABC, &mut 2, Instant::now()).is_err());
    assert!(file.is_file());
    assert!(delete_verified(&actual, ABC, &mut 512, Instant::now() - DEADLINE).is_err());
    assert!(file.is_file());
    let huge = fs::OpenOptions::new().write(true).open(&file).unwrap();
    huge.set_len(MAX_BYTES + 1).unwrap();
    drop(huge);
    assert!(cleanup_at(&Settings::default(), &limits, false).is_err());
    assert_eq!(fs::metadata(file).unwrap().len(), MAX_BYTES + 1);
}
