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
        assert!(w.try_find("thumbnail-apply").is_none());
        w.click("thumbnail-restore", cx);
        w.render_frame(cx);
        assert!(w.find("dialog").visible());
        w.within("dialog").click("cancel", cx);
        w.render_frame(cx);
        manager.update(cx, |view, _| assert!(!view.operation.busy()));
        let card = w.find("thumbnail-card-.ai").bounds();
        w.click_at(
            "thumbnail-card-.ai",
            gpui_kit::point(px(8.), card.size.height - px(8.)),
            cx,
        );
        w.render_frame(cx);
        assert_eq!(w.find("thumbnail-.ai").checked(), Some(true));
        let status_position = w.find("thumbnail-state-.ai").bounds().origin;
        w.click_at("thumbnail-card-.ai", gpui_kit::point(px(8.), px(8.)), cx);
        w.render_frame(cx);
        assert_eq!(w.find("thumbnail-.ai").checked(), Some(false));
        w.click_at(
            "thumbnail-card-.ai",
            gpui_kit::point(card.size.width / 2., card.size.height / 2.),
            cx,
        );
        w.render_frame(cx);
        assert_eq!(w.find("thumbnail-.ai").checked(), Some(true));
        w.click("thumbnail-.ai", cx);
        w.render_frame(cx);
        assert_eq!(w.find("thumbnail-.ai").checked(), Some(false));
        assert_eq!(
            w.find("thumbnail-state-.ai").bounds().origin,
            status_position
        );
        w.press("space", cx);
        w.render_frame(cx);
        assert_eq!(w.find("thumbnail-.ai").checked(), Some(true));
        w.press("space", cx);
        w.render_frame(cx);
        assert_eq!(w.find("thumbnail-.ai").checked(), Some(false));
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
    let orphaned = br#"{"format_version":1,"selected":[".svg"]}"#;
    std::fs::write(&settings, orphaned).unwrap();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        manager.update(cx, |view, _| assert!(view.thumbnails.draft.is_empty()));
        w.click("thumbnail-.ai", cx);
        w.render_frame(cx);
        w.click("thumbnail-apply", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(
        std::fs::read(&settings).unwrap(),
        orphaned,
        "適用時に外部変更を検出し、保存済み設定を上書きしない"
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
        w.click("thumbnail-error-details", cx);
        w.render_frame(cx);
        assert!(w.find("thumbnail-missing-recovery").visible());
        assert!(w.try_find("thumbnail-apply").is_none());
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
    let _registry = IsolatedRegistryProfile::new();
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
                for (name, mode, scenario) in [
                    ("enabled-dark", ThemeMode::Dark, "enabled"),
                    ("disabled-dark", ThemeMode::Dark, "disabled"),
                    ("changed-dark", ThemeMode::Dark, "changed"),
                    ("attention-dark", ThemeMode::Dark, "attention"),
                    ("enabled-light", ThemeMode::Light, "enabled"),
                    ("disabled-light", ThemeMode::Light, "disabled"),
                    ("changed-light", ThemeMode::Light, "changed"),
                    ("attention-light", ThemeMode::Light, "attention"),
                ] {
                    let selected: Vec<_> = match scenario {
                        "enabled" => file_icons_windows::thumbnails::Format::ALL.to_vec(),
                        "changed" => vec![file_icons_windows::thumbnails::Format::Ai],
                        _ => Vec::new(),
                    };
                    let groups = selected
                        .iter()
                        .map(|format| {
                            file_icons_windows::thumbnails::plan_extension_group(
                                &file_icons_windows::UserRegistry,
                                *format,
                            )
                            .unwrap()
                        })
                        .collect::<Vec<_>>();
                    window
                        .update(cx, |_, w, cx| {
                            theme::apply(mode, cx);
                            gpui_kit::component::theme::Theme::update(cx, |theme| {
                                theme.font_size = px(16.)
                            });
                            manager.update(cx, |view, cx| {
                                let recovery = (!groups.is_empty()).then_some(
                                    file_icons_windows::thumbnails::Recovery {
                                        format_version: 1,
                                        groups,
                                        transition: None,
                                    },
                                );
                                let current = recovery
                                    .as_ref()
                                    .map(|r| r.selected_extensions().unwrap())
                                    .unwrap_or_default();
                                view.settings.language = "ja".into();
                                view.thumbnails.status =
                                    Some(file_icons_manager_service::ThumbnailStatus {
                                        settings: file_icons_windows::thumbnails::Settings::default(
                                        ),
                                        recovery,
                                        missing_recovery: scenario == "attention",
                                        group_errors: Vec::new(),
                                    });
                                view.thumbnails.draft = if scenario == "changed" {
                                    [".eps".into()].into_iter().collect()
                                } else {
                                    current
                                };
                                cx.notify();
                            });
                            w.resize(size(px(1120.), px(780.)));
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
                                view.thumbnails.status =
                                    Some(file_icons_manager_service::ThumbnailStatus {
                                        settings: file_icons_windows::thumbnails::Settings::default(
                                        ),
                                        recovery: None,
                                        missing_recovery: false,
                                        group_errors: Vec::new(),
                                    });
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
                window
                    .update(cx, |_, w, cx| {
                        theme::apply(ThemeMode::Dark, cx);
                        manager.update(cx, |view, cx| {
                            view.settings.language = "ja".into();
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
