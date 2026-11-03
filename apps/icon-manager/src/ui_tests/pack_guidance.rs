//! 版の比較から下書き・保存、見本から保護込みの結果までを実操作する。
use super::*;
use crate::{Kind, Status};
use file_icons_core::selection::Selection;
use gpui_kit::SharedString;
use std::{fs, sync::Arc};

pub(super) fn fixture() -> super::IsolatedRegistryProfile {
    let registry = super::target_protection::fixture();
    let mut settings = load().unwrap();
    let mut old = settings.packs.remove(0);
    old.id.push_str("@1.0.0");
    let mut new = old.clone();
    new.id = old.id.replace("@1.0.0", "@2.0.0");
    new.version = "2.0.0".into();
    new.label = "TypeScript icons / 日本語の長いパック名".into();
    let mut rules = file_icons_core::ThemeRules::default();
    let icon = old.theme.rules().extensions["json"].clone();
    rules.extensions.insert("json".into(), icon.clone());
    rules.extensions.insert("mts".into(), icon.clone());
    rules.extensions.insert("md".into(), icon.clone());
    old.theme = file_icons_core::CompiledTheme::new(rules.clone()).unwrap();
    rules.extensions.remove("md");
    rules.extensions.insert("rs".into(), icon);
    new.theme = file_icons_core::CompiledTheme::new(rules).unwrap();
    settings.packs = vec![new.clone(), old.clone()];
    let mut selection = Selection::default();
    selection.set("extension:json", Some(false)).unwrap();
    selection.set("extension:md", Some(false)).unwrap();
    settings.selections.insert(old.id, selection);
    file_icons_windows::save(&settings).unwrap();
    registry
}

#[test]
fn pack_guidance_in_isolated_profile() {
    let profile = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "ui_tests::pack_guidance::pack_guidance_in_child",
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
#[ignore = "専用プロファイルを持つ親試験からのみ起動する"]
fn pack_guidance_in_child(cx: &mut TestAppContext) {
    assert_eq!(std::env::var("WFI_GUI_PROBE").as_deref(), Ok("1"));
    let _registry = fixture();
    let saved = load().unwrap();
    let new_id = saved.packs[0].id.clone();
    let old_id = saved.packs[1].id.clone();
    let before = fs::read(home().unwrap().join("settings.library.json")).unwrap();
    let (window, manager) = cx.update(|cx| {
        gpui_kit::init(cx);
        theme::apply(ThemeMode::Dark, cx);
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1120.), px(780.)),
                    cx,
                ))),
                ..Default::default()
            },
            cx,
            Manager::open,
        )
        .unwrap()
    });
    let changes = SharedString::from(format!("version-changes-{old_id}"));
    let copy = SharedString::from(format!("target-copy-{old_id}"));
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click(pack_button(&manager, cx, "update-settings", 0), cx);
        w.render_frame(cx);
        assert_eq!(
            w.find(SharedString::from(format!("version-summary-{old_id}")))
                .label(),
            Some("一致 2 · 追加 1 · 削除 1")
        );
        w.click(changes.clone(), cx);
        w.render_frame(cx);
        let text = w.find("version-changes-list").label().unwrap().to_owned();
        assert!(text.contains("+ .rs") && text.contains("− .md"));
        w.press("alt-left", cx);
        w.render_frame(cx);
        assert!(w.find("target-toggle-extension:json").visible());
        w.click("target-copy-open", cx);
        w.render_frame(cx);
        w.click(copy.clone(), cx);
        w.render_frame(cx);
        assert_eq!(
            w.find("target-toggle-extension:json").checked(),
            Some(false)
        );
        assert_eq!(w.find("target-toggle-extension:rs").checked(), Some(true));
        w.click("target-undo", cx);
        w.render_frame(cx);
        assert_eq!(w.find("target-toggle-extension:json").checked(), Some(true));
        assert_eq!(
            fs::read(home().unwrap().join("settings.library.json")).unwrap(),
            before
        );
        w.click("target-copy-open", cx);
        w.render_frame(cx);
        w.click(copy, cx);
        w.render_frame(cx);
        w.click("target-save", cx);
    })
    .unwrap();
    cx.run_until_parked();
    let after = load().unwrap();
    assert_eq!(after.selections[&old_id], saved.selections[&old_id]);
    assert!(!after.selections[&new_id].enabled("sample.json"));
    assert!(after.selections[&new_id].enabled("sample.rs"));
    assert!(after.selections[&new_id].explicit("extension:md").is_none());
    assert_eq!(after.packs.len(), 2);
    assert!(after.active.is_none() && after.journal.is_empty());
    let after_save = fs::read(home().unwrap().join("settings.library.json")).unwrap();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("target-back", cx);
        w.render_frame(cx);
        w.click(pack_button(&manager, cx, "details", 0), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.scroll(
            "pack-scroll",
            gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-2000.))),
            cx,
        );
        w.render_frame(cx);
        w.click("preview-name", cx);
        w.input("sample.mts", cx);
        w.render_frame(cx);
        assert!(!w.find("typed-preview").label().unwrap().contains("Windows"));
        w.click("check-typed-preview", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        assert!(
            w.find("target-planned")
                .label()
                .unwrap()
                .contains("既存アイコンを保護中")
        );
        assert!(
            w.find("target-planned")
                .label()
                .unwrap()
                .contains("sample.mts")
        );
        w.press("escape", cx);
        w.render_frame(cx);
        w.click("target-protection-extension:mts", cx);
        w.render_frame(cx);
        w.click("pack-icon-mts", cx);
        w.render_frame(cx);
        w.click("target-details-open", cx);
        w.render_frame(cx);
        assert!(
            w.find("target-planned")
                .label()
                .unwrap()
                .contains("パックのアイコン")
        );
        // 方針の切り替えと確認画面への移動だけでは保存しない。
        assert!(manager.read(cx).targets.pack.as_deref() == Some(&new_id));
        assert_eq!(
            fs::read(home().unwrap().join("settings.library.json")).unwrap(),
            after_save
        );
        w.click("target-back", cx);
        w.render_frame(cx);
        w.click("target-back", cx);
        w.render_frame(cx);
        manager.update(cx, |view, _| {
            let progress = Arc::new(file_icons_catalog::Progress::default());
            progress.begin_theme(2, 4);
            progress.converted(12, 30);
            view.operation
                .begin(Kind::Download("vscode-icons"), Some(progress));
        });
        // 件数・経過の表示と中断を、実際に進捗を置く一覧画面から操作する。
        w.render_frame(cx);
        let message = w.find("operation-progress").label().unwrap().to_owned();
        assert!(
            message.contains("テーマ 2/4")
                && message.contains("画像 12/30")
                && message.contains("経過")
        );
        w.click("cancel-download", cx);
        w.render_frame(cx);
        assert!(
            w.find("operation-progress")
                .label()
                .unwrap()
                .contains("キャンセルの完了")
        );
        manager.update(cx, |view, _| view.operation.finish(Status::Cancelled));
    })
    .unwrap();
}

#[test]
#[ignore = "専用プロファイルで版比較・見本・表示予定・進捗を実描画する"]
fn pack_guidance_visual_probe() {
    use gpui_kit::application;
    assert_eq!(std::env::var("WFI_GUI_PROBE").as_deref(), Ok("1"));
    let output = std::path::PathBuf::from(std::env::var("WFI_VISUAL_OUTPUT").unwrap());
    assert!(output.is_absolute() && home().unwrap().starts_with(output.parent().unwrap()));
    fs::create_dir_all(&output).unwrap();
    let registry = fixture();
    let settings = load().unwrap();
    let sources =
        file_icons_windows::inspect_existing_icons(&settings.packs[0], &settings).unwrap();
    drop(registry);
    application()
        .with_assets(crate::ui_assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            cx.set_global(
                gpui_kit::component::tooltip::TooltipDefaults::new()
                    .with_show_delay(std::time::Duration::from_secs(3600)),
            );
            let (window, manager) =
                gpui_kit::open_window(WindowOptions::default(), cx, Manager::open).unwrap();
            manager.update(cx, |this, _| this.protection.visual_sources(sources));
            cx.spawn(async move |cx| {
                for (language, mode, width, height, font) in [
                    ("ja", ThemeMode::Dark, 1120., 780., 16.),
                    ("en", ThemeMode::Light, 1120., 780., 16.),
                    ("ja", ThemeMode::Dark, 640., 640., 18.),
                    ("en", ThemeMode::Light, 640., 640., 18.),
                ] {
                    for page in [
                        "library",
                        "update",
                        "samples",
                        "planned",
                        "progress",
                        "cancelling",
                    ] {
                        window
                            .update(cx, |_, w, cx| {
                                theme::apply(mode, cx);
                                gpui_kit::component::theme::Theme::update(cx, |theme| {
                                    theme.font_size = px(font)
                                });
                                manager.update(cx, |this, cx| {
                                    this.operation = crate::Operation::default();
                                    this.settings.language = language.into();
                                    this.prepare_guidance_visual(page);
                                    this.preview.update(cx, |input, cx| {
                                        input.set_value("sample.mts", w, cx)
                                    });
                                    if matches!(page, "progress" | "cancelling") {
                                        let progress =
                                            Arc::new(file_icons_catalog::Progress::default());
                                        progress
                                            .stage
                                            .store(2, std::sync::atomic::Ordering::Relaxed);
                                        progress.begin_theme(2, 4);
                                        progress.converted(12, 30);
                                        if page == "cancelling" {
                                            progress.cancel();
                                        }
                                        this.operation
                                            .begin(Kind::Download("vscode-icons"), Some(progress));
                                    }
                                    cx.notify();
                                });
                                w.resize(size(px(width), px(height)));
                                // 最初のフレームで画像読み込みを要求し、完了を待ってから撮影する。
                                w.render_frame(cx);
                            })
                            .unwrap();
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(350))
                            .await;
                        window
                            .update(cx, |_, w, cx| {
                                w.render_frame(cx);
                                if matches!(page, "library" | "samples" | "progress" | "cancelling")
                                {
                                    w.scroll(
                                        "pack-scroll",
                                        gpui_kit::ScrollDelta::Pixels(gpui_kit::point(
                                            px(0.),
                                            px(10000.),
                                        )),
                                        cx,
                                    );
                                    w.render_frame(cx);
                                }
                                if page == "samples" {
                                    w.render_to_image()
                                        .unwrap()
                                        .save(
                                            output.join(format!(
                                                "samples-top-{language}-{width}.png"
                                            )),
                                        )
                                        .unwrap();
                                    super::protection_sources::reveal_in(
                                        w,
                                        "example-0",
                                        "pack-scroll",
                                        cx,
                                    );
                                    w.render_frame(cx);
                                    w.render_to_image()
                                        .unwrap()
                                        .save(
                                            output.join(format!(
                                                "samples-scope-{language}-{width}.png"
                                            )),
                                        )
                                        .unwrap();
                                    super::protection_sources::reveal_in(
                                        w,
                                        "check-typed-preview",
                                        "pack-scroll",
                                        cx,
                                    );
                                    w.render_frame(cx);
                                }
                                w.render_to_image()
                                    .unwrap()
                                    .save(output.join(format!("{page}-{language}-{width}.png")))
                                    .unwrap();
                            })
                            .unwrap();
                    }
                }
                cx.update(|cx| cx.quit());
            })
            .detach();
        });
}
