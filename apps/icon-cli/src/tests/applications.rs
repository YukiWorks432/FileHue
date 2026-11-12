//! Windowsのアプリ登録を共有する種類を保護し、他の種類への適用を継続する。
use super::*;

#[test]
fn application_associations_preserve_shared_registration_without_blocking_apply() {
    if std::env::var_os("WFI_APPLICATION_ASSOCIATION_PROBE").is_none() {
        let directory = tempfile::tempdir().unwrap();
        let executable = std::env::current_exe().unwrap();
        let copied = directory.path().join("application-tests.exe");
        fs::copy(&executable, &copied).unwrap();
        fs::copy(
            executable
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("file_icons_shell.dll"),
            directory.path().join("file_icons_shell.dll"),
        )
        .unwrap();
        let output = Command::new(copied)
            .args(["--exact", "tests::applications::application_associations_preserve_shared_registration_without_blocking_apply", "--nocapture"])
            .env("WFI_APPLICATION_ASSOCIATION_PROBE", "1")
            .env("LOCALAPPDATA", directory.path().join("profile"))
            .test_output().unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let (_redirect, classes) = Redirect::new();
    let progress = Arc::new(Progress::default());
    let _console = initialize(progress.clone()).unwrap();
    let application = r"aPpLiCaTiOnS\Illustrator.EXE";
    let app = classes.create_subkey(application).unwrap().0;
    let app_icon = app.create_subkey("DefaultIcon").unwrap().0;
    app_icon.set_value("", &"illustrator.ico,3").unwrap();
    let command = app.create_subkey(r"shell\open\command").unwrap().0;
    command.set_value("", &"illustrator.exe %1").unwrap();
    // パック内の共有種類と、全件調査で見つかるパック外の種類を含める。
    for extension in ["wfi-app", "wfi-app-alias", "wfi-outside"] {
        let key = classes.create_subkey(format!(".{extension}")).unwrap().0;
        key.set_value("", &application).unwrap();
        if extension == "wfi-outside" {
            key.create_subkey("DefaultIcon")
                .unwrap()
                .0
                .set_value("", &"outside.ico,7")
                .unwrap();
        }
    }
    classes
        .create_subkey(".wfi-normal")
        .unwrap()
        .0
        .set_value("", &"Wfi.Normal")
        .unwrap();
    let normal = classes.create_subkey("Wfi.Normal").unwrap().0;
    normal
        .create_subkey("DefaultIcon")
        .unwrap()
        .0
        .set_value("", &"normal.ico,2")
        .unwrap();
    let source = tempfile::tempdir().unwrap();
    fs::write(source.path().join("package.json"), r#"{"name":"application-associations","version":"1","contributes":{"iconThemes":[{"id":"one","label":"Applications","path":"theme.json"}]}}"#).unwrap();
    fs::write(source.path().join("theme.json"), r#"{"iconDefinitions":{"x":{"iconPath":"icon.svg"}},"fileExtensions":{"wfi-app":"x","wfi-app-alias":"x","wfi-normal":"x"}}"#).unwrap();
    fs::write(source.path().join("icon.svg"), r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="16" height="16" fill="#ff0000"/></svg>"##).unwrap();
    let manager = Manager::acquire().unwrap();
    let id = manager.import(source.path(), "one", &progress).unwrap();
    let settings = manager.settings().unwrap();
    let pack = settings.packs.iter().find(|p| p.id == id).unwrap();
    let rows = file_icons_windows::inspect_existing_icons(pack, &settings).unwrap();
    for extension in ["wfi-app", "wfi-app-alias", "wfi-outside"] {
        let row = rows
            .iter()
            .find(|r| r.registration.extension == extension)
            .unwrap();
        assert!(row.always_protected && row.protect);
    }
    // 共通保護を解除し、個別許可を保存しても、共有アプリ登録の保護を外さない。
    let mut settings = settings;
    for row in &rows {
        settings
            .icon_protection_rules
            .set_extension(row, Some(false));
    }
    file_icons_windows::save(&settings).unwrap();
    dispatch(&manager, &["protection", "set", "off"]).unwrap();
    dispatch(&manager, &["apply", &id, "--dry-run"]).unwrap();
    dispatch(&manager, &["apply", &id]).unwrap();
    let applied = manager.settings().unwrap();
    assert_eq!(applied.active.as_deref(), Some(id.as_str()));
    assert_eq!(
        normal
            .open_subkey("DefaultIcon")
            .unwrap()
            .get_value::<String, _>("")
            .unwrap(),
        "%1"
    );
    assert!(!applied.journal.is_empty());
    assert!(
        applied
            .journal
            .iter()
            .all(|c| !c.key.to_lowercase().contains("applications\\"))
    );
    assert_eq!(
        app_icon.get_value::<String, _>("").unwrap(),
        "illustrator.ico,3"
    );
    assert_eq!(
        command.get_value::<String, _>("").unwrap(),
        "illustrator.exe %1"
    );
    assert!(app.open_subkey(r"shellex\IconHandler").is_err());
    for extension in ["wfi-app", "wfi-app-alias", "wfi-outside"] {
        assert!(
            applied
                .icon_exclusions
                .iter()
                .any(|r| r.extension == extension)
        );
        assert_eq!(
            classes
                .open_subkey(format!(".{extension}"))
                .unwrap()
                .get_value::<String, _>("")
                .unwrap(),
            application
        );
    }
    // 同じパックの再適用と解除も、共有アプリと既定アプリを維持する。
    dispatch(&manager, &["apply", &id]).unwrap();
    dispatch(&manager, &["restore"]).unwrap();
    assert!(manager.settings().unwrap().journal.is_empty());
    assert_eq!(
        normal
            .open_subkey("DefaultIcon")
            .unwrap()
            .get_value::<String, _>("")
            .unwrap(),
        "normal.ico,2"
    );
    assert_eq!(
        app_icon.get_value::<String, _>("").unwrap(),
        "illustrator.ico,3"
    );
    assert_eq!(
        command.get_value::<String, _>("").unwrap(),
        "illustrator.exe %1"
    );
}
