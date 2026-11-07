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
    // 狭幅でも六つの表示を一つの選択欄から選べ、保存帯を見失わない。
    cx.simulate_window_resize(window, size(px(640.), px(640.)));
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("protection-view-select", cx);
        for _ in 0..5 {
            w.press("down", cx);
        }
        w.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        assert!(w.find("protection-changes").visible());
        let save = w.find("protection-save");
        assert!(save.visible() && save.bounds().bottom() <= w.viewport_size().height);
        w.click("protection-save", cx);
        assert!(!manager.read(cx).operation.busy());
        w.click("protection-view-select", cx);
        for _ in 0..5 {
            w.press("up", cx);
        }
        w.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("protection-view-select", cx);
        for _ in 0..3 {
            w.press("down", cx);
        }
        w.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        assert_eq!(w.find("protection-view-select").value(), Some("要確認"));
        w.click("protection-show-all", cx);
        w.render_frame(cx);
        assert_eq!(w.find("protection-view-select").value(), Some("すべて"));
        assert!(w.find("protection-row-").visible());
    })
    .unwrap();
    cx.simulate_window_resize(window, size(px(1120.), px(640.)));
    assert_eq!(serde_json::to_value(load().unwrap()).unwrap(), before);

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
        w.click("protection-view-changes", cx);
        w.render_frame(cx);
        w.click("protection-changes-basis", cx);
        w.press("down", cx);
        w.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        assert!(
            w.find("protection-change-row-mts")
                .label()
                .unwrap()
                .contains("保護 → パック許可")
        );
        // 操作記録へ現在の画像を流用しない。
        assert!(w.try_find("protection-result-current-mts").is_none());
        for (heading, cell) in [
            (
                "protection-change-before-heading",
                "protection-change-before-mts",
            ),
            (
                "protection-change-after-heading",
                "protection-change-after-mts",
            ),
        ] {
            let heading = w.find(heading).bounds().left();
            let cell = w.find(cell).bounds().left();
            assert!(
                cell >= heading - px(1.) && cell <= heading + px(1.),
                "比較列を見出しと揃える"
            );
        }
        if w.try_find("protection-change-row-mts").is_none() {
            w.click("protection-changes-show-same", cx);
            w.render_frame(cx);
        }
        if w.try_find("protection-change-edit-mts").is_none() {
            w.click("protection-change-details-mts", cx);
            w.render_frame(cx);
        }
        super::protection_sources::reveal_in(
            w,
            "protection-change-edit-mts",
            "protection-changes-rows",
            cx,
        );
        w.click("protection-change-edit-mts", cx);
        w.render_frame(cx);
        super::protection_sources::reveal(w, "original-icon-mts", cx);
        w.click("original-icon-mts", cx);
        w.render_frame(cx);
        w.click("protection-view-changes", cx);
        w.render_frame(cx);
        w.click("protection-changes-basis", cx);
        w.press("up", cx);
        w.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        assert!(w.try_find("protection-change-row-mts").is_none());
        assert!(w.try_find("protection-change-before-heading").is_none());
        w.click("protection-changes-show-same", cx);
        w.render_frame(cx);
        assert!(
            w.find("protection-change-row-mts")
                .label()
                .unwrap()
                .contains("指定更新")
        );
        assert_eq!(serde_json::to_value(load().unwrap()).unwrap(), before);
        w.click("protection-changes-basis", cx);
        w.press("down", cx);
        w.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        manager.update(cx, |this, cx| {
            this.protection.error = Some("inspection failed".into());
            cx.notify();
        });
        w.render_frame(cx);
        w.click("protection-changes-basis", cx);
        w.press("up", cx);
        w.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("protection-save", cx);
        assert!(!manager.read(cx).operation.busy());
        assert!(
            w.find("protection-changes-status")
                .label()
                .unwrap()
                .contains("確認できません")
        );
        manager.update(cx, |this, cx| {
            this.protection.error = None;
            cx.notify();
        });
        w.render_frame(cx);
        if w.try_find("protection-change-row-mts").is_none() {
            w.click("protection-changes-show-same", cx);
            w.render_frame(cx);
        }
        if w.try_find("protection-change-edit-mts").is_none() {
            w.click("protection-change-details-mts", cx);
            w.render_frame(cx);
        }
        super::protection_sources::reveal_in(
            w,
            "protection-change-edit-mts",
            "protection-changes-rows",
            cx,
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
        w.click("protection-view-changes", cx);
        w.render_frame(cx);
        if w.try_find("protection-change-row-m2ts").is_none() {
            w.click("protection-changes-show-same", cx);
            w.render_frame(cx);
        }
        assert!(
            w.find("protection-change-row-m2ts")
                .label()
                .unwrap()
                .contains("パック許可 → 保護")
        );
        w.click("protection-change-details-m2ts", cx);
        w.render_frame(cx);
        super::protection_sources::reveal_in(
            w,
            "protection-change-edit-m2ts",
            "protection-changes-rows",
            cx,
        );
        w.click("protection-change-edit-m2ts", cx);
        w.render_frame(cx);
        w.click("protection-recommended", cx);
        w.render_frame(cx);
        super::protection_sources::reveal(w, "original-icon-mts", cx);
        w.click("original-icon-mts", cx);
        w.render_frame(cx);
        w.click("protection-view-changes", cx);
        w.render_frame(cx);
        w.click("protection-changes-basis", cx);
        w.press("down", cx);
        w.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        assert!(w.find("protection-save").visible());
        w.click("protection-save", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(
        load().unwrap().icon_protection_rules.extensions["mts"].protect,
        "操作記録の許可ではなく、手動編集した現在の下書きを保存する"
    );
    let saved = serde_json::to_value(load().unwrap()).unwrap();
    cx.update_window(window, |_, w, cx| {
        manager.update(cx, |this, cx| {
            let template = this.protection.source("mts").unwrap().clone();
            let sources = (0..192)
                .map(|index| {
                    let mut source = template.clone();
                    source.registration.extension = format!("review-{index:03}");
                    source.provider_name =
                        Some("Example provider with a long display name for comparison".into());
                    source
                })
                .collect();
            this.settings.icon_protection_rules = Default::default();
            this.protection.rules = Default::default();
            this.protection.draft = true;
            this.protection.visual_sources(sources);
            cx.notify();
        });
        w.render_frame(cx);
        w.click("protection-planned", cx);
        w.render_frame(cx);
        w.click("protection-toggle", cx);
        w.render_frame(cx);
        w.click("protection-view-changes", cx);
        w.render_frame(cx);
        w.click("protection-changes-basis", cx);
        w.press("up", cx);
        w.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        assert!(w.try_find("protection-change-row-review-000").is_none());
        w.click("protection-changes-show-same", cx);
        w.render_frame(cx);
        w.click("protection-change-details-review-000", cx);
        w.render_frame(cx);
        w.press("end", cx);
        w.render_frame(cx);
        assert!(w.find("protection-change-row-review-191").visible());
        assert!(w.find("protection-save").visible());
        w.press("home", cx);
        w.render_frame(cx);
        assert!(w.find("protection-change-row-review-000").visible());
    })
    .unwrap();
    assert_eq!(serde_json::to_value(load().unwrap()).unwrap(), saved);
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
                manager.update(cx, |this, _| {
                    // 保存はせず、適用中パックと保護済み登録を再現する。
                    this.settings.active = Some(this.settings.packs[0].id.clone());
                    this.protection.visual_applied(&sources);
                    this.protection.visual_sources(sources);
                });
                window
                    .update(cx, |_, w, cx| {
                        w.render_frame(cx);
                        w.click("protection-recommended", cx);
                        w.render_frame(cx);
                        w.click("protection-view-changes", cx);
                        w.render_frame(cx);
                    })
                    .unwrap();
                for scenario in ["mixed", "choices"] {
                    if scenario == "choices" {
                        manager.update(cx, |this, cx| {
                            let template = this.protection.source("mts").unwrap().clone();
                            let sources = [
                                "c", "cc", "cpp", "go", "h", "java", "js", "jsx", "py", "rb", "tsx",
                            ]
                            .into_iter()
                            .map(|extension| {
                                let mut source = template.clone();
                                source.registration.extension = extension.into();
                                source.fingerprint = extension.into();
                                source.provider_id = Some("example-code".into());
                                source.provider_name = Some(
                                    "Visual Studio Code — Example development installation".into(),
                                );
                                source.provider_location = Some(file_icons_windows::IconLocation {
                                    path: r"C:\Apps\Code.exe".into(),
                                    index: 0,
                                });
                                source
                            })
                            .collect::<Vec<_>>();
                            let mut rules = file_icons_windows::IconProtectionRules::default();
                            rules.set_provider(&sources, "example-code", Some(false));
                            for source in &sources {
                                rules.set_extension(source, Some(false));
                            }
                            this.settings.icon_protection_rules = rules.clone();
                            this.settings.preserve_existing_icons = true;
                            this.protection.rules = rules;
                            this.protection.draft = true;
                            this.protection.visual_sources(sources);
                            cx.notify();
                        });
                        window
                            .update(cx, |_, w, cx| {
                                w.render_frame(cx);
                                w.click("protection-recommended", cx);
                                w.render_frame(cx);
                            })
                            .unwrap();
                    }
                    for (language, mode, width, font) in [
                        ("ja", ThemeMode::Dark, 1120., 16.),
                        ("ja", ThemeMode::Light, 1120., 16.),
                        ("en", ThemeMode::Dark, 1120., 16.),
                        ("en", ThemeMode::Light, 1120., 16.),
                        ("ja", ThemeMode::Dark, 640., 18.),
                        ("ja", ThemeMode::Light, 640., 18.),
                        ("en", ThemeMode::Dark, 640., 18.),
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
                                    cx.notify();
                                });
                                w.resize(size(px(width), px(640.)));
                                w.render_frame(cx);
                            })
                            .unwrap();
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(350))
                            .await;
                        window
                            .update(cx, |_, w, cx| {
                                w.render_frame(cx);
                                w.click("protection-changes-basis", cx);
                                w.press("down", cx);
                                w.press("enter", cx);
                            })
                            .unwrap();
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(100))
                            .await;
                        window
                            .update(cx, |_, w, cx| {
                                w.render_frame(cx);
                                assert_eq!(
                                    w.find("protection-changes-basis").value(),
                                    Some(if language == "ja" {
                                        "直前のおすすめ操作"
                                    } else {
                                        "Last recommended action"
                                    })
                                );
                                let save = w.find("protection-save");
                                assert!(
                                    save.visible()
                                        && save.bounds().bottom() <= w.viewport_size().height
                                );
                                if scenario == "choices" {
                                    assert!(
                                        w.find("protection-changes-summary")
                                            .label()
                                            .unwrap()
                                            .contains("11")
                                    );
                                }
                                w.render_to_image()
                                    .unwrap()
                                    .save(output.join(format!(
                                        "{scenario}-recommended-{language}-{mode:?}-{width}.png"
                                    )))
                                    .unwrap();
                                w.click("protection-changes-basis", cx);
                                w.press("up", cx);
                                w.press("enter", cx);
                            })
                            .unwrap();
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(100))
                            .await;
                        window
                            .update(cx, |_, w, cx| {
                                w.render_frame(cx);
                                assert_eq!(
                                    w.find("protection-changes-basis").value(),
                                    Some(if language == "ja" {
                                        "保存予定"
                                    } else {
                                        "Pending save"
                                    })
                                );
                                w.render_to_image()
                                    .unwrap()
                                    .save(output.join(format!(
                                        "{scenario}-pending-{language}-{mode:?}-{width}.png"
                                    )))
                                    .unwrap();
                                if scenario == "choices"
                                    && language == "ja"
                                    && mode == ThemeMode::Dark
                                    && width == 1120.
                                {
                                    assert!(
                                        w.try_find("protection-change-before-heading").is_none()
                                    );
                                    w.click("protection-changes-show-same", cx);
                                    w.render_frame(cx);
                                    w.render_to_image()
                                        .unwrap()
                                        .save(output.join("choices-expanded-ja-Dark-1120.png"))
                                        .unwrap();
                                    w.click("protection-changes-show-same", cx);
                                    w.render_frame(cx);
                                }
                                if scenario == "mixed"
                                    && language == "ja"
                                    && mode == ThemeMode::Dark
                                    && width == 1120.
                                {
                                    w.click("protection-change-details-mts", cx);
                                    w.render_frame(cx);
                                    w.scroll(
                                        "protection-change-row-mts",
                                        gpui_kit::ScrollDelta::Pixels(gpui_kit::point(
                                            px(0.),
                                            px(-250.),
                                        )),
                                        cx,
                                    );
                                    w.render_frame(cx);
                                    w.render_to_image()
                                        .unwrap()
                                        .save(output.join("mixed-detail-ja-Dark-1120.png"))
                                        .unwrap();
                                    w.click("protection-change-details-mts", cx);
                                    w.render_frame(cx);
                                }
                            })
                            .unwrap();
                    }
                }
                cx.update(|cx| cx.quit());
            })
            .detach();
        });
}
