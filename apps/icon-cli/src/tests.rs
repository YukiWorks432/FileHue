use super::*;
use crate::test_process::CommandOutput;
use std::{fs, process::Command};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CLASSES_ROOT as HKCR, HKEY_CURRENT_USER as HKCU, RegOverridePredefKey,
};
use winreg::{RegKey, enums::*};
mod conflicts;

struct Redirect {
    physical: RegKey,
    name: String,
}
impl Redirect {
    fn new() -> (Self, RegKey) {
        let physical = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags("", KEY_ALL_ACCESS)
            .unwrap();
        let name = format!(
            "Software\\WindowsFileIconsDev\\Tests\\cli-{}",
            std::process::id()
        );
        assert!(physical.open_subkey(&name).is_err());
        eprintln!("試験用HKCUキー: {name}");
        let user = physical.create_subkey(&name).unwrap().0;
        let classes = user.create_subkey("Software\\Classes").unwrap().0;
        unsafe {
            RegOverridePredefKey(HKCU, Some(HKEY(user.raw_handle())))
                .ok()
                .unwrap();
            RegOverridePredefKey(HKCR, Some(HKEY(classes.raw_handle())))
                .ok()
                .unwrap();
        }
        (Self { physical, name }, classes)
    }
}
impl Drop for Redirect {
    fn drop(&mut self) {
        unsafe {
            RegOverridePredefKey(HKCR, None).ok().unwrap();
            RegOverridePredefKey(HKCU, None).ok().unwrap();
        }
        self.physical.delete_subkey_all(&self.name).unwrap();
    }
}
fn dispatch(manager: &Manager, values: &[&str]) -> Result<Value> {
    let args = Args::parse(&values.iter().map(|s| s.to_string()).collect::<Vec<_>>())?;
    commands::run(manager, &args, Arc::new(Progress::default()), false)
}
#[test]
fn registration_switch_conflict_recovery_and_cancellation_in_isolation() {
    if std::env::var_os("WFI_CLI_PROBE").is_none() {
        let directory = tempfile::tempdir().unwrap();
        let executable = std::env::current_exe().unwrap();
        let copied = directory.path().join("cli-tests.exe");
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
            .args([
                "--exact",
                "tests::registration_switch_conflict_recovery_and_cancellation_in_isolation",
                "--nocapture",
            ])
            .env("WFI_CLI_PROBE", "1")
            .env("LOCALAPPDATA", directory.path().join("profile"))
            .test_output()
            .unwrap();
        assert!(
            output.status.success(),
            "{} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let (_redirect, classes) = Redirect::new();
    let progress = Arc::new(Progress::default());
    let _console = initialize(progress.clone()).unwrap();
    let class = classes.create_subkey("Cli.Test").unwrap().0;
    class
        .create_subkey("DefaultIcon")
        .unwrap()
        .0
        .set_value("", &"original.ico,3")
        .unwrap();
    class
        .create_subkey("shell\\open\\command")
        .unwrap()
        .0
        .set_value("", &"original.exe %1")
        .unwrap();
    classes
        .create_subkey(".wfi-cli-probe")
        .unwrap()
        .0
        .set_value("", &"Cli.Test")
        .unwrap();
    for (ext, class_name) in [("lnk", "lnkfile"), ("exe", "exefile")] {
        classes
            .create_subkey(format!(".{ext}"))
            .unwrap()
            .0
            .set_value("", &class_name)
            .unwrap();
        classes
            .create_subkey(format!("{class_name}\\DefaultIcon"))
            .unwrap()
            .0
            .set_value("", &"original-protected.ico,7")
            .unwrap();
    }
    classes
        .create_subkey(".wfi-protected-alias")
        .unwrap()
        .0
        .set_value("", &"lnkfile")
        .unwrap();
    let source = tempfile::tempdir().unwrap();
    fs::write(source.path().join("package.json"),r#"{"name":"cli-test","version":"1","contributes":{"iconThemes":[{"id":"a","label":"A","path":"theme.json"},{"id":"b","label":"B","path":"theme.json"}]}}"#).unwrap();
    fs::write(source.path().join("theme.json"),r#"{"iconDefinitions":{"x":{"iconPath":"icon.svg"}},"fileNames":{"special.wfi-cli-probe":"x","sample.wfi-cli-probe":"x"},"fileExtensions":{"wfi-cli-probe":"x","wfi-protected-alias":"x","lnk":"x","exe":"x"}}"#).unwrap();
    fs::write(source.path().join("icon.svg"),r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="16" height="16" fill="#00ff00"/></svg>"##).unwrap();
    let manager = Manager::acquire().unwrap();
    let a = manager.import(source.path(), "a", &progress).unwrap();
    let b = manager.import(source.path(), "b", &progress).unwrap();
    // この試験は、明示的に保護を無効にした場合の上書き・復旧を検証する。
    dispatch(&manager, &["protection", "set", "off"]).unwrap();
    let before = fs::read(file_icons_windows::home().unwrap().join("settings.json")).unwrap();
    dispatch(&manager, &["apply", &a, "--dry-run"]).unwrap();
    assert_eq!(
        fs::read(file_icons_windows::home().unwrap().join("settings.json")).unwrap(),
        before
    );
    assert_eq!(
        class
            .open_subkey("DefaultIcon")
            .unwrap()
            .get_value::<String, _>("")
            .unwrap(),
        "original.ico,3"
    );
    dispatch(&manager, &["apply", &a]).unwrap();
    // 旧版が保護クラスを書き換えた状態を、専用レジストリと復元記録にだけ作る。
    use file_icons_core::registry::{Change, Value};
    let recovery_path = file_icons_windows::home().unwrap().join("recovery.json");
    let mut record: serde_json::Value =
        serde_json::from_slice(&fs::read(&recovery_path).unwrap()).unwrap();
    for class_name in ["lnkfile", "exefile"] {
        let key = classes
            .open_subkey_with_flags(format!("{class_name}\\DefaultIcon"), KEY_ALL_ACCESS)
            .unwrap();
        let before = key.get_raw_value("").unwrap();
        key.set_value("", &"%1").unwrap();
        let after = key.get_raw_value("").unwrap();
        let change = Change {
            key: format!("Software\\Classes\\{class_name}\\DefaultIcon"),
            name: String::new(),
            before: Some(Value {
                kind: before.vtype as u32,
                bytes: before.bytes.into_owned(),
            }),
            after: Value {
                kind: after.vtype as u32,
                bytes: after.bytes.into_owned(),
            },
        };
        record["journal"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::to_value(change).unwrap());
    }
    fs::write(&recovery_path, serde_json::to_vec(&record).unwrap()).unwrap();
    dispatch(&manager, &["apply", &a]).unwrap();
    for class_name in ["lnkfile", "exefile"] {
        assert_eq!(
            classes
                .open_subkey(format!("{class_name}\\DefaultIcon"))
                .unwrap()
                .get_value::<String, _>("")
                .unwrap(),
            "original-protected.ico,7"
        );
        assert!(
            !file_icons_windows::load()
                .unwrap()
                .journal
                .iter()
                .any(|c| c.key.contains(class_name))
        );
    }
    assert_eq!(
        file_icons_windows::load().unwrap().active.as_deref(),
        Some(a.as_str())
    );
    assert_eq!(
        class
            .open_subkey("shell\\open\\command")
            .unwrap()
            .get_value::<String, _>("")
            .unwrap(),
        "original.exe %1"
    );
    assert_eq!(
        classify(&dispatch(&manager, &["pack", "remove", &a]).unwrap_err()).1,
        4
    );
    dispatch(
        &manager,
        &["targets", "edit", &a, "file:special.wfi-cli-probe", "off"],
    )
    .unwrap();
    let selected = file_icons_windows::load().unwrap();
    assert_eq!(selected.active.as_deref(), Some(a.as_str()));
    assert!(!selected.active_selection.enabled("special.wfi-cli-probe"));
    assert!(selected.active_selection.enabled("normal.wfi-cli-probe"));
    dispatch(
        &manager,
        &["targets", "edit", &a, "file:sample.wfi-cli-probe", "off"],
    )
    .unwrap();
    let selected = file_icons_windows::load().unwrap();
    assert_eq!(selected.active.as_deref(), Some(a.as_str()));
    assert!(!selected.journal.is_empty());
    assert!(class.open_subkey("shellex\\IconHandler").is_ok());
    assert!(
        selected
            .active_selection
            .resolve(&selected.packs[0].theme, "normal.wfi-cli-probe")
            .is_some()
    );
    // 適用中の全無効化は元へ戻し、別テーマの編集では現在の適用を変えない。
    dispatch(&manager, &["targets", "edit", &b, "all", "off"]).unwrap();
    assert_eq!(
        file_icons_windows::load().unwrap().active.as_deref(),
        Some(a.as_str())
    );
    dispatch(
        &manager,
        &["targets", "edit", &a, "all", "off", "--dry-run"],
    )
    .unwrap();
    assert_eq!(
        file_icons_windows::load().unwrap().active.as_deref(),
        Some(a.as_str())
    );
    dispatch(&manager, &["targets", "edit", &a, "all", "off"]).unwrap();
    let restored = file_icons_windows::load().unwrap();
    assert!(restored.active.is_none() && restored.journal.is_empty());
    assert_eq!(
        class
            .open_subkey("DefaultIcon")
            .unwrap()
            .get_value::<String, _>("")
            .unwrap(),
        "original.ico,3"
    );
    dispatch(&manager, &["targets", "edit", &b, "all", "inherit"]).unwrap();
    dispatch(&manager, &["apply", &b]).unwrap();
    assert_eq!(
        file_icons_windows::load().unwrap().active.as_deref(),
        Some(b.as_str())
    );
    class
        .open_subkey_with_flags("DefaultIcon", KEY_SET_VALUE)
        .unwrap()
        .set_value("", &"external.ico")
        .unwrap();
    let conflict = dispatch(&manager, &["restore"]).unwrap_err();
    assert_eq!(classify(&conflict), ("registration_conflict", 6));
    let detail = format!("{conflict:#}");
    assert!(detail.contains("DefaultIcon"));
    assert!(
        detail.contains(
            &file_icons_windows::home()
                .unwrap()
                .join("settings.json")
                .display()
                .to_string()
        )
    );
    assert_eq!(
        class
            .open_subkey("DefaultIcon")
            .unwrap()
            .get_value::<String, _>("")
            .unwrap(),
        "external.ico"
    );
    class
        .open_subkey_with_flags("DefaultIcon", KEY_SET_VALUE)
        .unwrap()
        .set_value("", &"%1")
        .unwrap();
    let home = file_icons_windows::home().unwrap();
    fs::write(home.join("settings.json"), b"broken").unwrap();
    dispatch(&manager, &["recover", "--dry-run"]).unwrap();
    assert_eq!(fs::read(home.join("settings.json")).unwrap(), b"broken");
    dispatch(&manager, &["recover"]).unwrap();
    assert!(fs::read_dir(&home).unwrap().any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("settings-damaged-")
    }));
    assert!(file_icons_windows::load().unwrap().active.is_none());
    assert_eq!(
        class
            .open_subkey("DefaultIcon")
            .unwrap()
            .get_value::<String, _>("")
            .unwrap(),
        "original.ico,3"
    );
    assert!(class.open_subkey("shellex\\IconHandler").is_err());
    // Ctrl+Cの受付を実際の変換の中断契約へ接続する。
    let before = fs::read(home.join("settings.library.json")).unwrap();
    assert_eq!(unsafe { interrupt(CTRL_C_EVENT) }, BOOL(1));
    let interrupted = manager.import(source.path(), "a", &progress).unwrap_err();
    assert_eq!(classify(&interrupted), ("cancelled", 130));
    assert_eq!(
        fs::read(home.join("settings.library.json")).unwrap(),
        before
    );
    dispatch(&manager, &["cleanup"]).unwrap();
}
