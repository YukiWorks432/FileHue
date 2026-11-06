//! 拡張子なしの許可範囲、対応名の見本、保存後の表示を隔離環境で確かめる。
use super::*;
use std::process::Command;

pub(super) fn fixture() -> IsolatedRegistryProfile {
    let registry = super::target_protection::fixture();
    let mut settings = load().unwrap();
    for pack in &mut settings.packs {
        let mut rules = pack.theme.rules().clone();
        rules
            .files
            .insert("dockerfile".into(), rules.extensions["json"].clone());
        pack.theme.replace_rules(rules).unwrap();
    }
    file_icons_windows::save(&settings).unwrap();
    let service = crate::ManagerService::acquire().unwrap();
    let pack = service.settings().unwrap().packs[0].id.clone();
    let dll = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/debug/file_icons_shell.dll");
    service.apply(&pack, &dll).unwrap();
    registry
}

#[test]
fn extensionless_controls_in_isolated_profile() {
    let profile = tempfile::tempdir().unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "ui_tests::extensionless_protection::extensionless_controls_in_child",
            "--nocapture",
        ])
        .env("LOCALAPPDATA", profile.path())
        .env("WFI_GUI_PROBE", "1")
        .test_output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[gpui_kit::test]
#[ignore = "親試験の専用プロファイルと隔離レジストリでのみ実行する"]
fn extensionless_controls_in_child(cx: &mut TestAppContext) {
    assert_eq!(std::env::var("WFI_GUI_PROBE").as_deref(), Ok("1"));
    let _registry = fixture();
    let before = load().unwrap();
    assert!(before.preserve_existing_icons);
    assert!(
        before
            .icon_exclusions
            .iter()
            .any(|row| row.extension.is_empty())
    );
    let (window, manager) = cx.update(|cx| {
        gpui_kit::init(cx);
        theme::apply(ThemeMode::Dark, cx);
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1120.), px(1050.)),
                    cx,
                ))),
                ..Default::default()
            },
            cx,
            Manager::open,
        )
        .unwrap()
    });
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("protection-open", cx);
    })
    .unwrap();
    cx.run_until_parked();
    let provider = manager.update(cx, |this, _| {
        let provider = this
            .protection
            .source("mts")
            .unwrap()
            .provider_id
            .clone()
            .unwrap();
        assert_eq!(
            this.protection.source("").unwrap().provider_id.as_ref(),
            Some(&provider)
        );
        provider
    });
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("protection-collapse-all", cx);
        w.render_frame(cx);
        assert!(w.try_find("protection-extensionless-scope").is_none());
        w.click("protection-expand-all", cx);
        w.render_frame(cx);
        assert!(w.find("protection-extensionless-scope").visible());
        assert!(
            w.find("provider-toggle-extensionless")
                .label()
                .unwrap()
                .contains("拡張子のないファイルなど")
        );
        assert!(
            w.find("protection-extensionless-scope")
                .label()
                .unwrap()
                .contains("全パック共通")
        );
        assert!(
            w.find("protection-result-current-")
                .label()
                .unwrap()
                .contains("dockerfile")
        );
        assert!(
            w.find("protection-result-after-")
                .label()
                .unwrap()
                .contains("元の表示")
        );
        let inspection = manager.read(cx).protection.inspection;
        w.click("pack-icon-", cx);
        w.render_frame(cx);
        assert!(
            w.find("pack-icon-")
                .label()
                .unwrap()
                .starts_with("パックのアイコンを許可")
        );
        assert!(
            w.find("protection-result-after-")
                .label()
                .unwrap()
                .contains("パックのアイコン")
        );
        assert!(
            w.find("protection-result-current-")
                .label()
                .unwrap()
                .contains("元の表示")
        );
        assert!(manager.read(cx).protection.source("mts").unwrap().protect);
        assert_eq!(manager.read(cx).protection.inspection, inspection);
        assert!(load().unwrap().icon_protection_rules.extensions.is_empty());
        w.click("provider-toggle-extensionless", cx);
        w.render_frame(cx);
        manager.update(cx, |this, _| {
            let mut legacy = this.protection.rules.extensions[""].clone();
            legacy.protect = true;
            this.protection
                .rules
                .providers
                .entry(provider.clone())
                .or_default()
                .insert(String::new(), legacy);
        });
        super::protection_sources::reveal(w, format!("provider-pack-{provider}"), cx);
        w.click(format!("provider-pack-{provider}"), cx);
        w.render_frame(cx);
        super::protection_sources::reveal(w, format!("provider-default-{provider}"), cx);
        w.click(format!("provider-default-{provider}"), cx);
        w.render_frame(cx);
        assert_eq!(
            w.find(format!("provider-default-{provider}")).checked(),
            Some(true)
        );
        assert!(manager.read(cx).protection.rules.providers[&provider][""].protect);
        assert!(!manager.read(cx).protection.source("").unwrap().protect);
        super::protection_sources::reveal(w, format!("provider-toggle-{provider}"), cx);
        w.click(format!("provider-toggle-{provider}"), cx);
        w.render_frame(cx);
        // 同じ画像資源のソフトを閉じても、拡張子なしの項目は独立して開ける。
        super::protection_sources::reveal(w, "provider-toggle-extensionless", cx);
        w.click("provider-toggle-extensionless", cx);
        w.render_frame(cx);
        assert_eq!(w.find("pack-icon-").checked(), Some(true));
        w.click("protection-save", cx);
    })
    .unwrap();
    cx.run_until_parked();
    let saved = load().unwrap();
    assert!(saved.preserve_existing_icons);
    assert!(!saved.icon_protection_rules.extensions[""].protect);
    assert!(
        !saved
            .icon_exclusions
            .iter()
            .any(|row| row.extension.is_empty())
    );
    assert!(
        saved
            .icon_exclusions
            .iter()
            .any(|row| row.extension == "mts")
    );
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        assert!(
            w.find("protection-result-current-")
                .label()
                .unwrap()
                .contains("パックのアイコン")
        );
        w.click("protection-back", cx);
        w.render_frame(cx);
        w.click("en", cx);
        w.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("protection-open", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        assert!(
            w.find("provider-toggle-extensionless")
                .label()
                .unwrap()
                .contains("Extensionless files")
        );
        assert!(
            w.find("pack-icon-")
                .label()
                .unwrap()
                .starts_with("Allow pack icons")
        );
        w.click("default-icon-", cx);
        w.render_frame(cx);
        assert!(
            w.find("protection-result-after-")
                .label()
                .unwrap()
                .contains("Original appearance")
        );
        w.click("protection-save", cx);
    })
    .unwrap();
    cx.run_until_parked();
    let inherited = load().unwrap();
    assert!(inherited.preserve_existing_icons);
    assert!(!inherited.icon_protection_rules.extensions.contains_key(""));
    assert!(
        inherited
            .icon_protection_rules
            .providers
            .values()
            .all(|rules| !rules.contains_key(""))
    );
    assert!(
        inherited
            .icon_exclusions
            .iter()
            .any(|row| row.extension.is_empty())
    );
    cx.update_window(window, |_, w, cx| {
        // パターンだけのパックに仮の名前を当てて、元表示と断定しない。
        manager.update(cx, |this, cx| {
            let active = this.settings.active.clone().unwrap();
            let pack = this
                .settings
                .packs
                .iter_mut()
                .find(|pack| pack.id == active)
                .unwrap();
            let mut rules = pack.theme.rules().clone();
            rules.files.remove("dockerfile");
            pack.theme.replace_rules(rules).unwrap();
            cx.notify();
        });
        w.render_frame(cx);
        assert!(
            w.find("protection-result-current-")
                .label()
                .unwrap()
                .contains("Check a filename")
        );
        assert!(
            w.find("protection-result-after-")
                .label()
                .unwrap()
                .contains("Check a filename")
        );
        // 識別子の辞書順では先になるアプリと並べても、拡張子なしを先頭へ出す。
        manager.update(cx, |this, cx| {
            let mut sources = this.protection.sources.clone();
            for source in &mut sources {
                if !source.registration.extension.is_empty() {
                    source.provider_id = Some("application:order-fixture".into());
                    source.provider_name = Some("Example Editor".into());
                }
            }
            this.accept_protection_sources(sources);
            this.refresh_protection_policy(cx);
        });
        w.render_frame(cx);
        w.click("protection-collapse-all", cx);
        w.render_frame(cx);
        w.press("home", cx);
        w.render_frame(cx);
        assert!(
            w.find("provider-toggle-extensionless").bounds().top()
                < w.find("provider-toggle-application:order-fixture")
                    .bounds()
                    .top()
        );
        w.click("protection-expand-all", cx);
        w.render_frame(cx);
        assert!(
            w.find("provider-toggle-extensionless").bounds().top()
                < w.find("provider-toggle-application:order-fixture")
                    .bounds()
                    .top()
        );
    })
    .unwrap();
    assert!(
        load().unwrap().packs[0]
            .theme
            .resolve("Dockerfile")
            .is_some()
    );
}
