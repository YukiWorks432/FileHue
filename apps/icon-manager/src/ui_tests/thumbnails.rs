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
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("thumbnails-open", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("thumbnail-.ai", cx);
        window.click("thumbnail-.eps", cx);
        manager.update(cx, |view, _| {
            assert_eq!(
                view.thumbnails.draft,
                [".ai".into(), ".eps".into()].into_iter().collect()
            )
        });
        assert!(!home().unwrap().join("thumbnail-settings.json").exists());
        assert!(!home().unwrap().join("thumbnail-recovery.json").exists());
        window.scroll(
            "thumbnail-scroll",
            gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-900.))),
            cx,
        );
        window.render_frame(cx);
        window.click("thumbnail-.pdf", cx);
        manager.update(cx, |view, _| {
            assert!(view.thumbnails.draft.contains(".pdf"))
        });
        window.click("navigate-back", cx);
        window.render_frame(cx);
        window.click("thumbnails-open", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        manager.update(cx, |view, _| {
            assert_eq!(
                view.thumbnails.draft,
                [".ai".into(), ".eps".into(), ".pdf".into()]
                    .into_iter()
                    .collect()
            )
        });
        window.click("thumbnail-discard", cx);
        manager.update(cx, |view, _| assert!(view.thumbnails.draft.is_empty()));
        window.click("thumbnail-apply", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(!home().unwrap().join("thumbnail-recovery.json").exists());
    let settings = home().unwrap().join("thumbnail-settings.json");
    assert!(!settings.exists(), "確認だけでは設定を保存しない");
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.find("thumbnail-plan").visible());
        window.scroll(
            "thumbnail-scroll",
            gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-500.))),
            cx,
        );
        window.render_frame(cx);
        window.click("thumbnail-.ai", cx);
        window.click("thumbnail-discard", cx);
        window.click("thumbnail-apply", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(!settings.exists(), "選択の変更後は再確認を必要とする");
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.find("thumbnail-plan").visible());
        window.click("thumbnail-apply", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        manager.update(cx, |view, _| {
            assert!(matches!(
                view.operation.status(),
                crate::status::Status::Complete
            ));
        });
        assert!(!settings.exists(), "未選択の適用では不要な設定を作成しない");
        window.click("thumbnail-apply", cx);
    })
    .unwrap();
    cx.run_until_parked();
    let orphaned = br#"{"format_version":1,"selected":[".svg"]}"#;
    std::fs::write(&settings, orphaned).unwrap();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("thumbnail-apply", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(
        std::fs::read(&settings).unwrap(),
        orphaned,
        "確認後の外部変更を上書きしない"
    );
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("navigate-back", cx);
        window.render_frame(cx);
        window.click("thumbnails-open", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.scroll(
            "thumbnail-scroll",
            gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-900.))),
            cx,
        );
        window.render_frame(cx);
        assert!(window.find("thumbnail-missing-recovery").visible());
        window.click("thumbnail-apply", cx);
        window.click("thumbnail-restore", cx);
    })
    .unwrap();
    cx.run_until_parked();
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
