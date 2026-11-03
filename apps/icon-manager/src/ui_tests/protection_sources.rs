//! 実際の照会、画像抽出、GUI入力、保存を専用プロファイルで結合する。
use super::*;
use std::{fs, process::Command};

pub(super) fn reveal(
    window: &mut gpui_kit::Window,
    id: impl Into<gpui_kit::ElementId>,
    cx: &mut gpui_kit::App,
) {
    reveal_in(window, id, "protection-rows", cx);
}

pub(super) fn reveal_in(
    window: &mut gpui_kit::Window,
    id: impl Into<gpui_kit::ElementId>,
    viewport_id: &'static str,
    cx: &mut gpui_kit::App,
) {
    let id = id.into();
    if window.try_find(id.clone()).is_none() {
        window.scroll(
            viewport_id,
            gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(10000.))),
            cx,
        );
    }
    for _ in 0..20 {
        window.render_frame(cx);
        let Some(target) = window.try_find(id.clone()).map(|target| target.bounds()) else {
            window.scroll(
                viewport_id,
                gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-100.))),
                cx,
            );
            continue;
        };
        let viewport = window.find(viewport_id).bounds();
        if target.top() >= viewport.top() && target.bottom() <= viewport.bottom() {
            return;
        }
        let delta = if target.bottom() > viewport.bottom() {
            -100.
        } else {
            100.
        };
        window.scroll(
            viewport_id,
            gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(delta))),
            cx,
        );
    }
    panic!(
        "操作対象を一覧内へ表示できない: {id:?}, target={:?}, viewport={:?}",
        window.try_find(id.clone()).map(|node| node.bounds()),
        window.find(viewport_id).bounds()
    );
}

#[test]
fn source_rules_gui_in_isolated_profile() {
    let profile = tempfile::tempdir().unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "ui_tests::protection_sources::source_rules_gui_in_child",
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
#[ignore = "親試験が作る専用プロファイルでのみ起動する"]
fn source_rules_gui_in_child(cx: &mut TestAppContext) {
    assert_eq!(std::env::var("WFI_GUI_PROBE").as_deref(), Ok("1"));
    let registry = IsolatedRegistryProfile::new();
    for root in [&registry._classes, &registry._effective] {
        for (extension, class, icon) in [
            ("wfi-a", "Source.A", "shell32.dll,2"),
            ("wfi-aa", "Source.A", "shell32.dll,2"),
            ("wfi-b", "Source.B", "imageres.dll,2"),
            (
                "wfi-pkg-a",
                "Package.A",
                "@{Example.App_1.0.0.0_x64__8wekyb3d8bbwe?ms-resource://Example.App/Files/Assets/a.png}",
            ),
            (
                "wfi-pkg-b",
                "Package.B",
                "@{Example.App_1.0.0.0_x64__8wekyb3d8bbwe?ms-resource://Example.App/Files/Assets/b.png}",
            ),
        ] {
            root.create_subkey(format!(".{extension}"))
                .unwrap()
                .0
                .set_value("", &class)
                .unwrap();
            root.create_subkey(format!("{class}\\shell\\open\\command"))
                .unwrap()
                .0
                .set_value(
                    "",
                    &format!(
                        "\"{}\\System32\\notepad.exe\" \"%1\"",
                        std::env::var("SystemRoot").unwrap()
                    ),
                )
                .unwrap();
            if class.starts_with("Package.") {
                let application = root
                    .create_subkey(format!("{class}\\Application"))
                    .unwrap()
                    .0;
                application
                    .set_value("AppUserModelID", &"Example.App_8wekyb3d8bbwe!Editor")
                    .unwrap();
                application
                    .set_value("ApplicationName", &"Example packaged editor")
                    .unwrap();
            }
            root.create_subkey(format!("{class}\\DefaultIcon"))
                .unwrap()
                .0
                .set_value("", &icon)
                .unwrap();
        }
    }
    let root = home().unwrap().join("source");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("package.json"), r#"{"name":"source-test","publisher":"test","version":"1.0.0","contributes":{"iconThemes":[{"id":"icons","label":"Icons","path":"theme.json"},{"id":"other","label":"Other","path":"other.json"}]}}"#).unwrap();
    fs::write(root.join("theme.json"), r#"{"iconDefinitions":{"a":{"iconPath":"icon.svg"}},"fileExtensions":{"wfi-a":"a","wfi-aa":"a","wfi-b":"a","wfi-pkg-a":"a","wfi-pkg-b":"a"}}"#).unwrap();
    fs::write(
        root.join("other.json"),
        r#"{"iconDefinitions":{"a":{"iconPath":"icon.svg"}},"fileExtensions":{"wfi-b":"a"}}"#,
    )
    .unwrap();
    fs::write(root.join("icon.svg"), r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="16" height="16" fill="#ee1100"/></svg>"##).unwrap();
    {
        let service = crate::ManagerService::acquire().unwrap();
        service.import(&root, "icons", &Default::default()).unwrap();
        service.import(&root, "other", &Default::default()).unwrap();
    }
    let saved = load().unwrap();
    let sources = file_icons_windows::inspect_existing_icons(&saved.packs[0], &saved).unwrap();
    let package_rows = sources
        .iter()
        .filter(|row| row.registration.extension.starts_with("wfi-pkg-"))
        .collect::<Vec<_>>();
    assert_eq!(package_rows.len(), 2);
    assert!(package_rows.iter().all(|row| row.provider_id.as_deref()
        == Some("package:example.app_8wekyb3d8bbwe!Editor")
        && row.provider_name.as_deref() == Some("Example packaged editor")));
    let provider = sources
        .iter()
        .find(|r| r.registration.extension == "wfi-a")
        .unwrap()
        .provider_id
        .as_ref()
        .unwrap()
        .clone();
    let (window, manager) = cx.update(|cx| {
        gpui_kit::init(cx);
        theme::apply(ThemeMode::Dark, cx);
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1000.), px(1100.)),
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
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        reveal(w, format!("provider-icon-{provider}"), cx);
        assert!(w.find(format!("provider-icon-{provider}")).visible());
        reveal(w, "original-icon-wfi-a", cx);
        assert!(
            w.find("original-icon-wfi-a").visible(),
            "実資源の抽出後に見本が表示されること"
        );
        let original = w.find("original-icon-wfi-a").bounds();
        let pack = w.find("pack-icon-wfi-a").bounds();
        assert_eq!(
            original.size.width, pack.size.width,
            "比較画像の列幅を揃えること"
        );
        // 同じクラスを共有しても、片方の個別指定はほかの種類へ影響しない。
        let inspection = manager.read(cx).protection.inspection;
        w.click("pack-icon-wfi-a", cx);
        w.render_frame(cx);
        assert!(w.find("protection-row-wfi-a").visible());
        assert!(
            w.find("protection-saved-wfi-a")
                .label()
                .unwrap()
                .contains("基本設定")
        );
        assert!(
            w.find("protection-result-current-wfi-a")
                .label()
                .unwrap()
                .contains("元の表示")
        );
        assert!(
            w.find("protection-result-after-wfi-a")
                .label()
                .unwrap()
                .contains("元の表示")
        );
        assert!(!manager.read(cx).protection.source("wfi-a").unwrap().protect);
        assert!(
            manager
                .read(cx)
                .protection
                .source("wfi-aa")
                .unwrap()
                .protect
        );
        assert_eq!(w.find("pack-icon-wfi-a").checked(), Some(true));
        assert_eq!(w.find("default-icon-wfi-aa").checked(), Some(true));
        assert_eq!(manager.read(cx).protection.inspection, inspection);
        assert!(load().unwrap().icon_protection_rules.extensions.is_empty());
        w.click("default-icon-wfi-a", cx);
        w.scroll(
            "protection-rows",
            gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(10000.))),
            cx,
        );
        w.render_frame(cx);
        w.click("protection-toggle", cx);
        w.render_frame(cx);
        assert!(
            w.find("default-icon-wfi-a")
                .label()
                .unwrap()
                .contains("結果はパック"),
            "再調査待ちを挟まず方針を表示すること"
        );
        w.click("protection-toggle", cx);
        w.render_frame(cx);
        w.click("pack-icon-wfi-b", cx);
        w.render_frame(cx);
        assert_eq!(w.find("pack-icon-wfi-b").checked(), Some(true));
        w.click("default-icon-wfi-b", cx);
        w.render_frame(cx);
        assert!(
            w.find("default-icon-wfi-b")
                .label()
                .unwrap()
                .contains("結果は元のアイコン")
        );
        // 別ソフトの指定を残し、後の一括初期化が対象範囲を越えないことを検証する。
        w.click("pack-icon-wfi-b", cx);
        assert!(load().unwrap().icon_protection_rules.extensions.is_empty());
        // 表示と詳細の切替は下書きや保存値を変更しない。
        let draft = serde_json::to_value(&manager.read(cx).protection.rules).unwrap();
        w.click("protection-current", cx);
        w.render_frame(cx);
        assert!(w.try_find("original-icon-wfi-a").is_none());
        assert!(w.find("protection-show-all").visible());
        w.click("protection-current", cx);
        w.render_frame(cx);
        assert!(w.try_find("original-icon-wfi-a").is_none());
        w.click("protection-planned", cx);
        w.render_frame(cx);
        reveal(w, "original-icon-wfi-a", cx);
        assert!(w.find("original-icon-wfi-a").visible());
        w.click("protection-details", cx);
        w.render_frame(cx);
        reveal(w, "protection-copy-wfi-a", cx);
        assert!(w.find("protection-copy-wfi-a").visible());
        w.click("protection-details", cx);
        w.render_frame(cx);
        assert!(w.try_find("protection-copy-wfi-a").is_none());
        w.scroll(
            "protection-rows",
            gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(10000.))),
            cx,
        );
        w.render_frame(cx);
        assert_eq!(
            serde_json::to_value(&manager.read(cx).protection.rules).unwrap(),
            draft
        );
        assert!(load().unwrap().icon_protection_rules.providers.is_empty());
        // ソフトアイコンからもヘッダー全体と同じ開閉を行える。
        w.click(format!("provider-icon-{provider}"), cx);
        w.render_frame(cx);
        assert!(w.try_find("original-icon-wfi-a").is_none());
        assert!(w.try_find("original-icon-wfi-aa").is_none());
        assert!(w.try_find(format!("provider-pack-{provider}")).is_none());
        assert!(
            w.find("original-icon-wfi-b").visible(),
            "別の提供元は開いたままであること"
        );
        assert!(
            w.find(format!("provider-toggle-{provider}"))
                .label()
                .unwrap()
                .contains("Expand")
                || w.find(format!("provider-toggle-{provider}"))
                    .label()
                    .unwrap()
                    .contains("展開")
        );
        assert!(load().unwrap().icon_protection_rules.providers.is_empty());
        w.click(format!("provider-count-{provider}"), cx);
        w.render_frame(cx);
        reveal(w, "original-icon-wfi-a", cx);
        assert!(w.find("original-icon-wfi-a").visible());
        reveal(w, format!("provider-toggle-{provider}"), cx);
        w.click_at(
            format!("provider-toggle-{provider}"),
            gpui_kit::point(px(2.), px(2.)),
            cx,
        );
        w.render_frame(cx);
        assert!(
            w.try_find("original-icon-wfi-a").is_none(),
            "ヘッダーの余白からも閉じられること"
        );
        w.click_at(
            format!("provider-toggle-{provider}"),
            gpui_kit::point(px(2.), px(2.)),
            cx,
        );
        w.render_frame(cx);
        reveal(w, "original-icon-wfi-a", cx);
        assert!(w.find("original-icon-wfi-a").visible());
        assert_eq!(w.find("default-icon-wfi-a").checked(), Some(true));
    })
    .unwrap();
    cx.run_until_parked();
    let mut heading_focused = false;
    for _ in 0..60 {
        heading_focused = cx
            .update_window(window, |_, w, cx| {
                w.render_frame(cx);
                if w.find(format!("provider-toggle-{provider}")).focused() == Some(true) {
                    true
                } else {
                    w.press("tab", cx);
                    false
                }
            })
            .unwrap();
        if heading_focused {
            break;
        }
    }
    assert!(heading_focused);
    cx.update_window(window, |_, w, cx| {
        w.press("enter", cx);
        w.render_frame(cx);
        assert!(w.try_find("original-icon-wfi-a").is_none());
        w.press("enter", cx);
        w.render_frame(cx);
        assert!(w.find("original-icon-wfi-a").visible());
        w.click(format!("provider-pack-{provider}"), cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(
        load().unwrap().icon_protection_rules.providers.is_empty(),
        "一括操作は保存前の下書きであること"
    );
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        reveal(w, "original-icon-wfi-a", cx);
        w.click("original-icon-wfi-a", cx);
        w.render_frame(cx);
        assert_eq!(
            w.find(format!("provider-pack-{provider}")).checked(),
            Some(true)
        );
        assert!(
            w.find("default-icon-wfi-a")
                .label()
                .unwrap()
                .contains("結果はパック")
        );
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        reveal(w, "default-icon-wfi-a", cx);
        w.click("default-icon-wfi-a", cx);
        w.render_frame(cx);
        assert_eq!(w.find("default-icon-wfi-a").checked(), Some(true));
        assert!(
            !manager
                .read(cx)
                .protection
                .rules
                .extensions
                .contains_key("wfi-a")
        );
        assert!(
            !manager
                .read(cx)
                .protection
                .sources
                .iter()
                .find(|row| row.registration.extension == "wfi-a")
                .unwrap()
                .protect
        );
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        reveal(w, "original-icon-wfi-a", cx);
        w.click("original-icon-wfi-a", cx);
        w.render_frame(cx);
        assert!(
            w.try_find(format!("provider-overrides-{provider}"))
                .is_some()
        );
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("protection-save", cx);
    })
    .unwrap();
    cx.run_until_parked();
    let settings = load().unwrap();
    assert!(settings.icon_protection_rules.extensions["wfi-a"].protect);
    assert!(!settings.icon_protection_rules.providers[&provider]["wfi-aa"].protect);
    let other_sources =
        file_icons_windows::inspect_existing_icons(&settings.packs[1], &settings).unwrap();
    assert!(
        other_sources
            .iter()
            .any(|row| row.registration.extension == "wfi-a" && row.protect)
    );
    assert!(
        other_sources
            .iter()
            .any(|row| row.registration.extension == "wfi-aa" && !row.protect)
    );
    assert!(
        file_icons_windows::inspect_pack_icons(&settings.packs[1], &settings)
            .unwrap()
            .iter()
            .all(|row| row.registration.extension != "wfi-a"
                && row.registration.extension != "wfi-aa"),
        "対象画面の見込みには、このパックの適用計画に無関係な保存済み指定を混ぜないこと"
    );
    assert!(
        file_icons_windows::protection_report(&settings.packs[1], &settings)
            .unwrap()
            .iter()
            .all(|row| row.extension != "wfi-a" && row.extension != "wfi-aa"),
        "別パックの保存済み指定を一覧に出しても、無関係な登録計画へ混ぜないこと"
    );
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        reveal(w, "protection-saved-wfi-aa", cx);
        assert!(
            w.find("protection-saved-wfi-aa")
                .label()
                .unwrap()
                .contains("基本設定・パックを許可")
        );
        w.click("protection-pack", cx);
        w.press("down", cx);
        w.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        reveal(w, "protection-outside-wfi-a", cx);
        assert!(w.find("protection-outside-wfi-a").visible());
        assert!(
            w.find("protection-saved-wfi-a")
                .label()
                .unwrap()
                .contains("個別")
        );
        w.click("protection-pack", cx);
        w.press("up", cx);
        w.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    // 外部の登録変更後は、以前の許可を保存済みの指定として見せつつ保護を優先する。
    for root in [&registry._classes, &registry._effective] {
        root.create_subkey("Source.B\\DefaultIcon")
            .unwrap()
            .0
            .set_value("", &"imageres.dll,3")
            .unwrap();
    }
    cx.update_window(window, |_, w, cx| w.click("protection-refresh", cx))
        .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("protection-review", cx);
        w.render_frame(cx);
        reveal(w, "protection-saved-wfi-b", cx);
        let label = w.find("protection-saved-wfi-b").label().unwrap().to_owned();
        assert!(label.contains("パックを許可") && label.contains("要確認"));
        assert!(manager.read(cx).protection.source("wfi-b").unwrap().protect);
        assert!(!load().unwrap().icon_protection_rules.extensions["wfi-b"].protect);
    })
    .unwrap();
    for root in [&registry._classes, &registry._effective] {
        root.create_subkey("Source.B\\DefaultIcon")
            .unwrap()
            .0
            .set_value("", &"imageres.dll,2")
            .unwrap();
    }
    cx.update_window(window, |_, w, cx| {
        w.click("protection-planned", cx);
        w.click("protection-refresh", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(
        file_icons_windows::protection_report(&settings.packs[0], &settings)
            .unwrap()
            .iter()
            .all(|r| r.extension != "wfi-aa"),
        "個別の保護を共有先の許可へ伝播させないこと"
    );
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        reveal(w, format!("provider-default-{provider}"), cx);
        w.click(format!("provider-default-{provider}"), cx);
        w.render_frame(cx);
        assert!(w.try_find("dialog").is_none());
        assert_eq!(
            w.find(format!("provider-default-{provider}")).checked(),
            Some(true)
        );
        assert!(manager.read(cx).protection.rules.extensions["wfi-a"].protect);
        assert!(
            w.find("default-icon-wfi-a")
                .label()
                .unwrap()
                .contains("結果は元のアイコン")
        );
        w.click(format!("provider-pack-{provider}"), cx);
    })
    .unwrap();
    // 基本設定の変更は個別指定を維持する。独立した初期化でのみ両方を解除する。
    for (choice, expected) in [("original", Some(true)), ("reset", None)] {
        let before = cx
            .update_window(window, |_, w, cx| {
                w.render_frame(cx);
                w.scroll(
                    "protection-rows",
                    gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(10000.))),
                    cx,
                );
                w.render_frame(cx);
                let id = format!("provider-{choice}-{provider}");
                reveal(w, id.clone(), cx);
                let before = manager.read(cx).protection.rules.clone();
                w.click(id, cx);
                w.render_frame(cx);
                if choice == "reset" {
                    assert!(w.find("dialog").visible());
                    assert_eq!(manager.read(cx).protection.rules, before);
                    w.within("dialog").click("cancel", cx);
                    w.render_frame(cx);
                    assert!(w.try_find("dialog").is_none());
                    assert_eq!(manager.read(cx).protection.rules, before);
                } else {
                    assert_eq!(
                        w.find(format!("provider-original-{provider}")).checked(),
                        Some(true)
                    );
                    assert!(
                        w.find("default-icon-wfi-a")
                            .label()
                            .unwrap()
                            .contains("結果は元のアイコン")
                    );
                }
                before
            })
            .unwrap();
        if choice == "reset" {
            // 部品が閉じる遅延処理を終えてから、管理画面へのフォーカス復帰を確認する。
            cx.run_until_parked();
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(300));
            cx.run_until_parked();
            cx.update_window(window, |_, w, cx| {
                w.render_frame(cx);
                assert!(manager.read(cx).focus.is_focused(w));
                // マウス押下ではフォーカスを動かさない部品なので、Tabで開く位置を選ぶ。
                for _ in 0..80 {
                    if w.find(format!("provider-reset-{provider}")).focused() == Some(true) {
                        break;
                    }
                    w.press("tab", cx);
                    w.render_frame(cx);
                }
                assert_eq!(
                    w.find(format!("provider-reset-{provider}")).focused(),
                    Some(true)
                );
                w.press("enter", cx);
                w.render_frame(cx);
                assert!(w.find("dialog").visible());
                w.press("escape", cx);
                w.render_frame(cx);
                assert!(w.try_find("dialog").is_none());
                assert_eq!(manager.read(cx).protection.rules, before);
                assert!(load().unwrap().icon_protection_rules.extensions["wfi-a"].protect);
            })
            .unwrap();
            cx.run_until_parked();
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(300));
            cx.run_until_parked();
            cx.update_window(window, |_, w, cx| {
                w.render_frame(cx);
                assert_eq!(
                    w.find(format!("provider-reset-{provider}")).focused(),
                    Some(true)
                );
                w.click(format!("provider-reset-{provider}"), cx);
                w.within("dialog").click("ok", cx);
                w.render_frame(cx);
                assert_eq!(
                    w.find(format!("provider-default-{provider}")).checked(),
                    Some(true)
                );
                for extension in ["wfi-a", "wfi-aa"] {
                    reveal(w, format!("default-icon-{extension}"), cx);
                    assert_eq!(
                        w.find(format!("default-icon-{extension}")).checked(),
                        Some(true)
                    );
                }
                assert!(
                    !manager
                        .read(cx)
                        .protection
                        .rules
                        .extensions
                        .contains_key("wfi-a")
                );
                assert!(
                    load().unwrap().icon_protection_rules.extensions["wfi-a"].protect,
                    "確認後も保存するまでは永続化しない"
                );
            })
            .unwrap();
        }
        cx.update_window(window, |_, w, cx| w.click("protection-save", cx))
            .unwrap();
        cx.run_until_parked();
        let saved = load().unwrap();
        assert_eq!(
            saved
                .icon_protection_rules
                .providers
                .get(&provider)
                .and_then(|rules| rules.get("wfi-aa"))
                .map(|rule| rule.protect),
            expected
        );
        assert_eq!(
            saved
                .icon_protection_rules
                .extensions
                .get("wfi-a")
                .map(|rule| rule.protect),
            if choice == "reset" { None } else { Some(true) }
        );
        assert!(
            !saved.icon_protection_rules.extensions["wfi-b"].protect,
            "ほかのソフトの個別指定は保持すること"
        );
    }
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click(format!("provider-original-{provider}"), cx);
        w.render_frame(cx);
        assert_eq!(
            w.find(format!("provider-original-{provider}")).checked(),
            Some(true)
        );
        w.click(format!("provider-default-{provider}"), cx);
        w.render_frame(cx);
        assert!(
            w.try_find("dialog").is_none(),
            "個別指定がなければ確認は不要"
        );
        assert_eq!(
            w.find(format!("provider-default-{provider}")).checked(),
            Some(true)
        );
        reveal(w, "original-icon-wfi-a", cx);
        w.click("original-icon-wfi-a", cx);
        w.click("protection-save", cx);
    })
    .unwrap();
    cx.run_until_parked();
    // 個別指定を解除し、保存せず戻ると以前の設定を保持する。
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        reveal(w, "default-icon-wfi-a", cx);
        w.click("default-icon-wfi-a", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.click("protection-back", cx);
        w.render_frame(cx);
        w.click("protection-open", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(load().unwrap().icon_protection_rules.extensions["wfi-a"].protect);
    let mut focused = false;
    for _ in 0..60 {
        focused = cx
            .update_window(window, |_, w, cx| {
                w.render_frame(cx);
                if w.find("default-icon-wfi-a").focused() == Some(true) {
                    true
                } else {
                    w.press("tab", cx);
                    false
                }
            })
            .unwrap();
        if focused {
            break;
        }
    }
    assert!(focused);
    cx.update_window(window, |_, w, cx| w.press("enter", cx))
        .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("protection-save", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(
        !load()
            .unwrap()
            .icon_protection_rules
            .extensions
            .contains_key("wfi-a")
    );
    assert!(load().unwrap().journal.is_empty() && load().unwrap().active.is_none());
    // 素材と関連付けクラスが異なる同一パッケージアプリも一度に開閉する。
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        for _ in 0..20 {
            let id = "provider-toggle-package:example.app_8wekyb3d8bbwe!Editor";
            if w.try_find(id).is_some_and(|node| {
                let viewport = w.find("protection-rows").bounds();
                node.visible()
                    && node.bounds().top() >= viewport.top()
                    && node.bounds().bottom() <= viewport.bottom()
            }) {
                break;
            }
            w.scroll(
                "protection-rows",
                gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-150.))),
                cx,
            );
            w.render_frame(cx);
        }
        let id = "provider-toggle-package:example.app_8wekyb3d8bbwe!Editor";
        assert!(
            w.find(id)
                .label()
                .unwrap()
                .contains("Example packaged editor (2/2)")
        );
        w.click(id, cx);
        w.render_frame(cx);
        assert!(w.try_find("original-icon-wfi-pkg-a").is_none());
        assert!(w.try_find("original-icon-wfi-pkg-b").is_none());
    })
    .unwrap();
    // 一括で閉じた後も再展開でき、保存前の指定を失わない。
    let saved_before_collapse = serde_json::to_value(load().unwrap()).unwrap();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("protection-collapse-all", cx);
        w.render_frame(cx);
        for extension in ["wfi-a", "wfi-aa", "wfi-b", "wfi-pkg-a", "wfi-pkg-b"] {
            assert!(w.try_find(format!("original-icon-{extension}")).is_none());
        }
        w.click(format!("provider-toggle-{provider}"), cx);
        w.render_frame(cx);
        w.click(format!("provider-pack-{provider}"), cx);
        w.render_frame(cx);
        assert!(!manager.read(cx).protection.rules.providers[&provider]["wfi-aa"].protect);
        let draft_before = serde_json::to_value(&manager.read(cx).protection.rules).unwrap();
        w.click("protection-collapse-all", cx);
        w.render_frame(cx);
        assert!(w.try_find(format!("provider-pack-{provider}")).is_none());
        w.click(format!("provider-toggle-{provider}"), cx);
        w.render_frame(cx);
        assert!(w.find(format!("provider-pack-{provider}")).visible());
        w.click("protection-expand-all", cx);
        w.render_frame(cx);
        reveal(w, "original-icon-wfi-a", cx);
        assert!(w.find("original-icon-wfi-a").visible());
        reveal(w, "original-icon-wfi-pkg-a", cx);
        assert!(w.find("original-icon-wfi-pkg-a").visible());
        reveal(w, "original-icon-wfi-pkg-b", cx);
        assert!(w.find("original-icon-wfi-pkg-b").visible());
        assert_eq!(
            serde_json::to_value(&manager.read(cx).protection.rules).unwrap(),
            draft_before
        );
    })
    .unwrap();
    assert_eq!(
        serde_json::to_value(load().unwrap()).unwrap(),
        saved_before_collapse
    );
    // 明色・英語・狭い画面でも保存と戻るの操作領域を確保する。
    cx.simulate_window_resize(window, size(px(640.), px(640.)));
    cx.update_window(window, |_, w, cx| {
        theme::apply(ThemeMode::Light, cx);
        gpui_kit::component::theme::Theme::update(cx, |t| t.font_size = px(18.));
        manager.update(cx, |v, _| v.settings.language = "en".into());
        w.render_frame(cx);
        for id in [
            "protection-back",
            "protection-save",
            "protection-collapse-all",
            "protection-expand-all",
        ] {
            let b = w.find(id).bounds();
            assert!(b.bottom() <= px(640.) && b.right() <= px(640.));
        }
        // 狭い幅・文字拡大でも、固定操作が一覧の大半を奪わない。
        let viewport = w.find("protection-rows").bounds();
        assert!(
            viewport.size.height >= px(640. / 3.),
            "一覧の表示領域: {:?}",
            viewport
        );
        w.scroll(
            "protection-rows",
            gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(10000.))),
            cx,
        );
        w.render_frame(cx);
        let first_provider = sources
            .iter()
            .map(file_icons_windows::ExistingIcon::group_id)
            .min()
            .unwrap();
        let default = format!("provider-default-{first_provider}");
        let choice = w.find(default).bounds();
        assert!(choice.top() >= viewport.top() && choice.bottom() <= viewport.bottom());
    })
    .unwrap();
    // 最後のパックを削除しても、共通の保存済み指定を確認・変更できる。
    cx.update_window(window, |_, w, cx| w.click("protection-back", cx))
        .unwrap();
    let service = cx.update(|cx| manager.read(cx)._guard.clone().unwrap());
    for pack in load().unwrap().packs {
        service.remove(&pack.id).unwrap();
    }
    assert!(load().unwrap().packs.is_empty());
    assert!(!load().unwrap().icon_protection_rules.extensions["wfi-b"].protect);
    cx.update_window(window, |_, w, cx| {
        manager.update(cx, |this, cx| {
            this.settings = service.settings().unwrap();
            this.selected = None;
            this.open_protection(w, cx);
        });
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        reveal(w, "protection-outside-wfi-b", cx);
        assert!(
            w.find("protection-outside-wfi-b")
                .label()
                .unwrap()
                .contains("パックがありません")
        );
        assert!(
            w.find("protection-saved-wfi-b")
                .label()
                .unwrap()
                .contains("パックを許可")
        );
        reveal(w, "original-icon-wfi-b", cx);
        w.click("original-icon-wfi-b", cx);
        w.click("protection-save", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(load().unwrap().icon_protection_rules.extensions["wfi-b"].protect);
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        reveal(w, "default-icon-wfi-b", cx);
        w.click("default-icon-wfi-b", cx);
        w.click("protection-save", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(
        !load()
            .unwrap()
            .icon_protection_rules
            .extensions
            .contains_key("wfi-b")
    );
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        reveal(w, "protection-saved-wfi-b", cx);
        assert!(
            w.find("protection-saved-wfi-b")
                .label()
                .unwrap()
                .contains("全体")
        );
        w.click("protection-refresh", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update(|cx| assert!(manager.read(cx).protection.sources.is_empty()));
}

#[test]
fn pattern_preview_does_not_claim_an_unregistered_override() {
    let profile = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "ui_tests::protection_sources::pattern_preview_in_child",
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
fn pattern_preview_in_child(cx: &mut TestAppContext) {
    assert_eq!(std::env::var("WFI_GUI_PROBE").as_deref(), Ok("1"));
    let registry = super::target_protection::fixture();
    for root in [&registry._classes, &registry._effective] {
        root.create_subkey(".wfi-test\\DefaultIcon")
            .unwrap()
            .0
            .set_value("", &"shell32.dll,2")
            .unwrap();
    }
    assert_eq!(
        file_icons_windows::association("wfi-test").unwrap(),
        ".wfi-test"
    );
    let source = home().unwrap().join("target-protection-fixture");
    let package_path = source.join("package.json");
    let mut package: serde_json::Value =
        serde_json::from_slice(&fs::read(&package_path).unwrap()).unwrap();
    package["contributes"]["iconThemes"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"id":"legacy","label":"Legacy","path":"legacy.json"}));
    fs::write(&package_path, serde_json::to_vec(&package).unwrap()).unwrap();
    fs::write(source.join("legacy.json"), r#"{"iconDefinitions":{"code":{"iconPath":"icon.svg"}},"fileExtensions":{"wfi-test":"code"}}"#).unwrap();
    {
        let service = crate::ManagerService::acquire().unwrap();
        let legacy_id = service
            .import(&source, "legacy", &Default::default())
            .unwrap();
        let mut settings = service.settings().unwrap();
        let legacy = settings
            .packs
            .iter()
            .find(|pack| pack.id == legacy_id)
            .unwrap();
        let sources = file_icons_windows::inspect_existing_icons(legacy, &settings).unwrap();
        let original = sources
            .iter()
            .find(|row| row.registration.extension == "wfi-test")
            .unwrap();
        settings
            .icon_protection_rules
            .set_extension(original, Some(false));
        service
            .set_icon_protection_rules(true, settings.icon_protection_rules, false)
            .unwrap();
        let active_id = settings.packs[0].id.clone();
        let dll = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .join("file_icons_shell.dll");
        service.apply(&active_id, &dll).unwrap();
    }
    let settings = load().unwrap();
    assert!(!settings.icon_protection_rules.extensions["wfi-test"].protect);
    assert!(
        settings
            .icon_exclusions
            .iter()
            .any(|row| row.extension.is_empty())
    );
    assert!(
        file_icons_windows::protection_report(&settings.packs[0], &settings)
            .unwrap()
            .iter()
            .any(|row| row.extension.is_empty()),
        "名前パターンの適用経路はUnknownの保護に従うこと"
    );
    let (window, _manager) = cx.update(|cx| {
        gpui_kit::init(cx);
        theme::apply(ThemeMode::Dark, cx);
        gpui_kit::open_window(WindowOptions::default(), cx, Manager::open).unwrap()
    });
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("protection-open", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        reveal(w, "protection-result-after-wfi-test", cx);
        assert!(
            w.find("protection-saved-wfi-test")
                .label()
                .unwrap()
                .contains("パックを許可")
        );
        assert!(
            w.find("protection-result-after-wfi-test")
                .label()
                .unwrap()
                .contains("確認できません"),
            "Unknownの保護を調べず、パック画像になると断定しないこと"
        );
    })
    .unwrap();
}
