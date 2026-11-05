use super::*;

#[test]
fn recommended_protection_gui_in_isolated_profile() {
    let profile = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "ui_tests::protection_recommendations::recommended_in_child",
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
#[ignore = "親試験が作る専用領域でのみ実行する"]
fn recommended_in_child(cx: &mut TestAppContext) {
    assert_eq!(std::env::var("WFI_GUI_PROBE").as_deref(), Ok("1"));
    let _registry = super::target_protection::fixture();
    let before = serde_json::to_value(load().unwrap()).unwrap();
    let (window, manager) = cx.update(|cx| {
        gpui_kit::init(cx);
        gpui_kit::open_window(WindowOptions::default(), cx, Manager::open).unwrap()
    });
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("protection-open", cx);
    })
    .unwrap();
    cx.run_until_parked();
    let inspection = cx.read(|cx| manager.read(cx).protection.inspection);
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("protection-toggle", cx);
        for _ in 0..100 {
            if w.find("protection-recommended").focused() == Some(true) {
                break;
            }
            w.press("tab", cx);
        }
        assert_eq!(w.find("protection-recommended").focused(), Some(true));
        w.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(
        cx.read(|cx| manager.read(cx).protection.inspection),
        inspection
    );
    assert!(!cx.read(|cx| manager.read(cx).protection.source("mts").unwrap().protect));
    assert!(cx.read(|cx| manager.read(cx).protection.source("m2ts").unwrap().protect));
    assert_eq!(serde_json::to_value(load().unwrap()).unwrap(), before);
    let rules = cx.read(|cx| manager.read(cx).protection.rules.clone());
    cx.update_window(window, |_, w, cx| {
        w.click("protection-recommended", cx);
    })
    .unwrap();
    assert_eq!(
        cx.read(|cx| manager.read(cx).protection.rules.clone()),
        rules
    );
    cx.update_window(window, |_, w, cx| {
        super::protection_sources::reveal(w, "original-icon-mts", cx);
        w.click("original-icon-mts", cx);
    })
    .unwrap();
    assert!(cx.read(|cx| manager.read(cx).protection.source("mts").unwrap().protect));
    cx.update_window(window, |_, w, cx| {
        w.click("protection-back", cx);
        w.click("protection-open", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(cx.read(|cx| manager.read(cx).protection.source("mts").unwrap().protect));
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("protection-recommended", cx);
        w.render_frame(cx);
        w.click("protection-recommended-changes", cx);
        w.render_frame(cx);
        assert!(
            w.find("protection-change-row-mts")
                .label()
                .unwrap()
                .contains("保護 → パック許可")
        );
        w.click("protection-change-edit-mts", cx);
        w.render_frame(cx);
        super::protection_sources::reveal(w, "original-icon-mts", cx);
        w.click("original-icon-mts", cx);
        w.render_frame(cx);
        w.click("protection-review-changes", cx);
        w.render_frame(cx);
        assert!(
            w.find("protection-change-row-mts")
                .label()
                .unwrap()
                .contains("指定のみ変更")
        );
        w.click("protection-changes-recommended", cx);
        w.render_frame(cx);
        assert!(
            w.find("protection-change-row-mts")
                .label()
                .unwrap()
                .contains("保護 → パック許可")
        );
        w.click("protection-change-edit-mts", cx);
        w.render_frame(cx);
        super::protection_sources::reveal(w, "pack-icon-mts", cx);
        w.click("pack-icon-mts", cx);
    })
    .unwrap();
    assert_eq!(serde_json::to_value(load().unwrap()).unwrap(), before);
    // 保存先の置換を一時的に拒否し、実際の保存失敗から再試行まで確認する。
    use std::os::windows::fs::OpenOptionsExt;
    let locked = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(home().unwrap().join("settings.library.json"))
        .unwrap();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("protection-recommended", cx);
        w.render_frame(cx);
        w.click("protection-save", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(cx.read(|cx| manager.read(cx).operation.failed()));
    assert_eq!(serde_json::to_value(load().unwrap()).unwrap(), before);
    assert_eq!(
        cx.read(|cx| manager.read(cx).protection.rules.clone()),
        rules
    );
    drop(locked);
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("protection-save", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(!load().unwrap().icon_protection_rules.extensions["mts"].protect);
    assert!(load().unwrap().preserve_existing_icons);
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        super::protection_sources::reveal(w, "pack-icon-m2ts", cx);
        w.click("pack-icon-m2ts", cx);
        w.render_frame(cx);
        w.click("protection-save", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(!load().unwrap().icon_protection_rules.extensions["m2ts"].protect);
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        super::protection_sources::reveal(w, "default-icon-m2ts", cx);
        w.click("default-icon-m2ts", cx);
        w.render_frame(cx);
        w.click("protection-refresh", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("protection-review-changes", cx);
        w.render_frame(cx);
        assert!(
            w.find("protection-change-row-m2ts")
                .label()
                .unwrap()
                .contains("パック許可 → 保護")
        );
    })
    .unwrap();
}

#[test]
#[ignore = "専用領域で日英・明暗・狭幅・拡大の実描画を行う"]
fn recommended_visual_probe() {
    assert_eq!(std::env::var("WFI_GUI_PROBE").as_deref(), Ok("1"));
    let output = std::path::PathBuf::from(std::env::var("WFI_VISUAL_OUTPUT").unwrap());
    assert!(output.is_absolute() && home().unwrap().starts_with(output.parent().unwrap()));
    std::fs::create_dir_all(&output).unwrap();
    let _registry = super::target_protection::fixture();
    let settings = load().unwrap();
    let sources =
        file_icons_windows::inspect_existing_icons(&settings.packs[0], &settings).unwrap();
    gpui_kit::application()
        .with_assets(crate::ui_assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            let (window, manager) =
                gpui_kit::open_window(WindowOptions::default(), cx, Manager::open).unwrap();
            cx.spawn(async move |cx| {
                window
                    .update(cx, |_, w, cx| {
                        manager.update(cx, |this, cx| this.open_protection(w, cx))
                    })
                    .unwrap();
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(2))
                    .await;
                manager.update(cx, |this, _| this.protection.visual_sources(sources));
                window
                    .update(cx, |_, w, cx| {
                        w.render_frame(cx);
                        w.click("protection-recommended", cx);
                        w.render_frame(cx);
                    })
                    .unwrap();
                for (language, mode, width, font) in [
                    ("ja", ThemeMode::Dark, 1120., 16.),
                    ("en", ThemeMode::Light, 1120., 16.),
                    ("ja", ThemeMode::Dark, 640., 18.),
                    ("en", ThemeMode::Light, 640., 18.),
                ] {
                    window
                        .update(cx, |_, w, cx| {
                            theme::apply(mode, cx);
                            gpui_kit::component::theme::Theme::update(cx, |theme| {
                                theme.font_size = px(font)
                            });
                            manager.update(cx, |this, cx| {
                                this.settings.language = language.into();
                                this.protection.changes_open = false;
                                cx.notify();
                            });
                            w.resize(size(px(width), px(640.)));
                            w.render_frame(cx);
                            w.click("protection-recommended-changes", cx);
                            w.render_frame(cx);
                        })
                        .unwrap();
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(350))
                        .await;
                    window
                        .update(cx, |_, w, cx| {
                            w.render_frame(cx);
                            w.render_to_image()
                                .unwrap()
                                .save(output.join(format!("recommended-{language}-{width}.png")))
                                .unwrap();
                            w.click("protection-changes-pending", cx);
                            w.render_frame(cx);
                            w.render_to_image()
                                .unwrap()
                                .save(output.join(format!("pending-{language}-{width}.png")))
                                .unwrap();
                        })
                        .unwrap();
                }
                cx.update(|cx| cx.quit());
            })
            .detach();
        });
}
