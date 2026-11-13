use super::*;

#[test]
fn selection_is_a_draft_in_an_isolated_profile() {
    let profile = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "ui_tests::thumbnails::selection_in_child",
            "--nocapture",
        ])
        .env("LOCALAPPDATA", profile.path())
        .env("WFI_THUMBNAIL_GUI_PROBE", "1")
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
fn selection_in_child(cx: &mut TestAppContext) {
    assert_eq!(std::env::var("WFI_THUMBNAIL_GUI_PROBE").as_deref(), Ok("1"));
    let _registry = IsolatedRegistryProfile::new();
    std::fs::create_dir_all(home().unwrap()).unwrap();
    std::fs::write(
        home().unwrap().join("settings.json"),
        b"invalid icon settings",
    )
    .unwrap();
    let (window, manager) = cx.update(|cx| {
        gpui_kit::init(cx);
        theme::apply(ThemeMode::Dark, cx);
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(640.), px(640.)),
                    cx,
                ))),
                ..Default::default()
            },
            cx,
            Manager::prepare(),
        )
        .unwrap()
    });
    let brand_origin = cx
        .update_window(window, |_, w, cx| {
            w.resize(size(px(1120.), px(780.)));
            w.render_frame(cx);
            let origin = w.find("app-brand").bounds().origin;
            w.click("thumbnails-open", cx);
            origin
        })
        .unwrap();
    cx.run_until_parked();
    let settings = home().unwrap().join("thumbnail-settings.json");
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        assert_eq!(w.find("app-brand").bounds().origin, brand_origin);
        w.click("thumbnail-apply", cx);
        manager.update(cx, |view, _| assert!(view.thumbnails.plan.is_none()));
        w.click("thumbnail-.ai", cx);
        super::protection_sources::reveal_in(w, "thumbnail-.eps", "thumbnail-scroll", cx);
        w.click("thumbnail-.eps", cx);
        w.render_frame(cx);
        assert_eq!(
            w.find("thumbnail-state-.ai").label(),
            Some("プレビュー無効")
        );
        assert_eq!(w.find("thumbnail-change-.ai").label(), Some("変更あり"));
        assert_eq!(w.find("thumbnail-.ai").checked(), Some(true));
        manager.update(cx, |view, _| {
            assert_eq!(
                view.thumbnails.draft,
                [".ai".into(), ".eps".into()].into_iter().collect()
            );
        });
        assert!(!settings.exists());
        // 画面を離れる前に確認し、キャンセルなら編集を続ける。
        w.click("about-open", cx);
        w.render_frame(cx);
        w.within("dialog").click("cancel", cx);
        w.render_frame(cx);
        assert!(w.find("thumbnail-.ai").visible());
        w.click("about-open", cx);
        w.render_frame(cx);
        w.within("dialog").click("ok", cx);
        w.render_frame(cx);
        w.click("navigate-back", cx);
        w.render_frame(cx);
        manager.update(cx, |view, _| assert!(view.thumbnails.draft.is_empty()));
        // 一括切り替えも適用するまでは保存・登録しない。
        w.click("thumbnail-enable-all", cx);
        manager.update(cx, |view, _| assert_eq!(view.thumbnails.draft.len(), 6));
        w.click("thumbnail-disable-all", cx);
        manager.update(cx, |view, _| assert!(view.thumbnails.draft.is_empty()));
        // 有効な登録がある場合も、無効化の下書きで現在の状態を隠さない。
        let group = file_icons_windows::thumbnails::plan_extension_group(
            &file_icons_windows::UserRegistry,
            file_icons_windows::thumbnails::Format::Ai,
        )
        .unwrap();
        let previous_status = manager.update(cx, |view, cx| {
            let previous_status = view.thumbnails.status.take();
            view.thumbnails.status = Some(file_icons_manager_service::ThumbnailStatus {
                settings: file_icons_windows::thumbnails::Settings {
                    selected: [".ai".into()].into_iter().collect(),
                    ..Default::default()
                },
                recovery: Some(file_icons_windows::thumbnails::Recovery {
                    format_version: 1,
                    groups: vec![group],
                    transition: None,
                }),
                missing_recovery: false,
                group_errors: vec![None],
            });
            view.thumbnails.draft = [".ai".into()].into_iter().collect();
            cx.notify();
            previous_status
        });
        w.render_frame(cx);
        assert_eq!(
            w.find("thumbnail-state-.ai").label(),
            Some("プレビュー有効")
        );
        assert!(w.try_find("thumbnail-change-.ai").is_none());
        w.click("thumbnail-.ai", cx);
        w.render_frame(cx);
        assert_eq!(w.find("thumbnail-.ai").checked(), Some(false));
        assert_eq!(
            w.find("thumbnail-state-.ai").label(),
            Some("プレビュー有効")
        );
        assert_eq!(w.find("thumbnail-change-.ai").label(), Some("変更あり"));
        w.click("thumbnail-.ai", cx);
        w.render_frame(cx);
        assert!(w.try_find("thumbnail-change-.ai").is_none());
        assert!(!settings.exists(), "切り替えだけでは登録を保存しない");
        manager.update(cx, |view, cx| {
            view.thumbnails.status = previous_status;
            view.thumbnails.draft.clear();
            cx.notify();
        });
        w.render_frame(cx);
        w.click("thumbnail-.ai", cx);
        w.click("navigate-back", cx);
        w.render_frame(cx);
        w.within("dialog").click("ok", cx);
        w.render_frame(cx);
        w.click("thumbnails-open", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        manager.update(cx, |view, _| assert!(view.thumbnails.draft.is_empty()));
        w.click("thumbnail-.ai", cx);
        w.click("thumbnail-apply", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(!settings.exists(), "変更内容の確認では保存しない");
    assert!(!home().unwrap().join("thumbnail-recovery.json").exists());
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        assert!(w.find("thumbnail-plan").visible());
        // 選択を変えたら、確認済みの変更予定は使い回さない。
        super::protection_sources::reveal_in(w, "thumbnail-.eps", "thumbnail-scroll", cx);
        w.click("thumbnail-.eps", cx);
        w.render_frame(cx);
        assert!(w.try_find("thumbnail-plan").is_none());
        w.click("thumbnail-apply", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(!settings.exists());
    let orphaned = br#"{"format_version":1,"selected":[".svg"]}"#;
    std::fs::write(&settings, orphaned).unwrap();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("thumbnail-apply", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(
        std::fs::read(&settings).unwrap(),
        orphaned,
        "確認後の外部変更を上書きしない"
    );
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("navigate-back", cx);
        w.render_frame(cx);
        if w.try_find("dialog").is_some() {
            w.within("dialog").click("ok", cx);
        }
        w.render_frame(cx);
        w.click("thumbnails-open", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.scroll(
            "thumbnail-scroll",
            gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-900.))),
            cx,
        );
        w.render_frame(cx);
        assert!(w.find("thumbnail-missing-recovery").visible());
        w.click("thumbnail-apply", cx);
        manager.update(cx, |view, _| assert!(view.thumbnails.plan.is_none()));
        w.click("thumbnail-restore", cx);
        manager.update(cx, |view, _| assert!(!view.operation.busy()));
    })
    .unwrap();
    assert_eq!(std::fs::read(settings).unwrap(), orphaned);
}

#[test]
#[ignore = "隔離した保存先でサムネイル画面を実描画する"]
fn thumbnail_visual_probe() {
    assert_eq!(std::env::var("WFI_THUMBNAIL_GUI_PROBE").as_deref(), Ok("1"));
    let output = std::path::PathBuf::from(std::env::var("WFI_VISUAL_OUTPUT").unwrap());
    assert!(output.is_absolute());
    std::fs::create_dir_all(&output).unwrap();
    gpui_kit::application()
        .with_assets(crate::ui_assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            let (window, manager) =
                gpui_kit::open_window(WindowOptions::default(), cx, Manager::open).unwrap();
            manager.update(cx, |view, cx| view.open_thumbnails(cx));
            cx.spawn(async move |cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(350))
                    .await;
                for (name, lang, mode, width, height, font) in [
                    ("ja-dark", "ja", ThemeMode::Dark, 1120., 780., 16.),
                    ("en-light", "en", ThemeMode::Light, 1120., 780., 16.),
                    ("ja-light", "ja", ThemeMode::Light, 1120., 780., 16.),
                    ("en-dark", "en", ThemeMode::Dark, 1120., 780., 16.),
                    ("ja-narrow", "ja", ThemeMode::Dark, 640., 640., 18.),
                    ("en-narrow", "en", ThemeMode::Light, 640., 640., 18.),
                ] {
                    window
                        .update(cx, |_, w, cx| {
                            theme::apply(mode, cx);
                            gpui_kit::component::theme::Theme::update(cx, |theme| {
                                theme.font_size = px(font)
                            });
                            manager.update(cx, |view, cx| {
                                view.settings.language = lang.into();
                                view.thumbnails.draft =
                                    [".ai".into(), ".eps".into()].into_iter().collect();
                                cx.notify();
                            });
                            w.resize(size(px(width), px(height)));
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
                                .save(output.join(format!("{name}.png")))
                                .unwrap();
                        })
                        .unwrap();
                }
                for (name, lang, mode, width, height, font) in [
                    ("ja-shared-review", "ja", ThemeMode::Dark, 1120., 780., 16.),
                    ("en-shared-review", "en", ThemeMode::Light, 640., 640., 18.),
                ] {
                    window
                        .update(cx, |_, w, cx| {
                            theme::apply(mode, cx);
                            gpui_kit::component::theme::Theme::update(cx, |theme| {
                                theme.font_size = px(font)
                            });
                            manager.update(cx, |view, cx| {
                                use file_icons_core::registry::{Change, Value};
                                use file_icons_windows::thumbnails::{Group, Recovery};
                                let selected: std::collections::BTreeSet<String> =
                                    [".ai".into(), ".eps".into()].into_iter().collect();
                                let group = Group {
                                    class: Some("Adobe.Illustrator.30".into()),
                                    members: [".ai".into(), ".ait".into(), ".eps".into()].into_iter().collect(),
                                    dependencies: None,
                                    previous_providers: None,
                                    changes: [".ai", ".ait", ".eps", "Adobe.Illustrator.30"]
                                        .into_iter()
                                        .map(|name| Change {
                                            key: format!("Software\\Classes\\{name}\\shellex\\{{e357fccd-a995-4576-b01f-234630154e96}}"),
                                            name: String::new(), before: None,
                                            after: Value { kind: 1, bytes: vec![] },
                                        })
                                        .collect(),
                                    created_keys: Vec::new(),
                                    keys_captured: false,
                                };
                                view.settings.language = lang.into();
                                view.thumbnails.plan = Some(file_icons_manager_service::ThumbnailSelectionPlan {
                                    selected: selected.clone(), current: Default::default(),
                                    additions: selected, removals: Default::default(), existing_groups: None,
                                    new_groups: Some(Recovery { format_version: 1, groups: vec![group], transition: None }),
                                    recheck_on_execution: true,
                                });
                                cx.notify();
                            });
                            w.resize(size(px(width), px(height)));
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
                                .save(output.join(format!("{name}.png")))
                                .unwrap();
                            if name == "en-shared-review" {
                                w.scroll(
                                    "thumbnail-scroll",
                                    gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-300.))),
                                    cx,
                                );
                                w.render_frame(cx);
                                w.render_to_image()
                                    .unwrap()
                                    .save(output.join("en-shared-review-scrolled.png"))
                                    .unwrap();
                            }
                        })
                        .unwrap();
                }
                window
                    .update(cx, |_, w, cx| {
                        theme::apply(ThemeMode::Dark, cx);
                        manager.update(cx, |view, cx| {
                            view.settings.language = "ja".into();
                            view.thumbnails.plan = None;
                            view.thumbnails.status =
                                Some(file_icons_manager_service::ThumbnailStatus {
                                    settings: file_icons_windows::thumbnails::Settings {
                                        selected: [".svg".into()].into_iter().collect(),
                                        ..Default::default()
                                    },
                                    recovery: None,
                                    missing_recovery: true,
                                    group_errors: Vec::new(),
                                });
                            cx.notify();
                        });
                        w.resize(size(px(640.), px(640.)));
                        w.render_frame(cx);
                        w.scroll(
                            "thumbnail-scroll",
                            gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-1200.))),
                            cx,
                        );
                        w.render_frame(cx);
                        w.render_to_image()
                            .unwrap()
                            .save(output.join("ja-narrow-recovery.png"))
                            .unwrap();
                    })
                    .unwrap();
                cx.update(|cx| cx.quit());
            })
            .detach();
        });
}
