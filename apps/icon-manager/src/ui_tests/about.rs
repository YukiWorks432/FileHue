use super::*;

#[test]
fn about_in_isolated_profile() {
    let profile = tempfile::tempdir().unwrap();
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "ui_tests::about::about_in_child",
            "--nocapture",
        ])
        .env("LOCALAPPDATA", profile.path())
        .env("WFI_GUI_PROBE", "1")
        .current_dir(profile.path())
        .test_output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

#[gpui_kit::test]
#[ignore = "専用プロファイルを持つ親試験からのみ起動する"]
fn about_in_child(cx: &mut TestAppContext) {
    assert_eq!(std::env::var("WFI_GUI_PROBE").as_deref(), Ok("1"));
    let root = home().unwrap().join("about-fixture");
    std::fs::create_dir_all(root.join("licenses")).unwrap();
    std::fs::write(root.join("LICENSE"), "fixture license").unwrap();
    std::fs::write(
        root.join("licenses/THIRD_PARTY_LICENSES.html"),
        "fixture notices",
    )
    .unwrap();
    let (window, manager) = cx.update(|cx| {
        gpui_kit::init(cx);
        theme::apply(ThemeMode::Dark, cx);
        gpui_kit::open_window(WindowOptions::default(), cx, Manager::open).unwrap()
    });
    cx.update_window(window, |_, w, cx| {
        assert_eq!(
            manager.read(cx).about.executable_dir.as_deref(),
            std::env::current_exe().unwrap().parent()
        );
        manager.update(cx, |m, _| m.about.executable_dir = Some(root.clone()));
        w.render_frame(cx);
        w.click("about-open", cx);
        w.render_frame(cx);
        assert!(manager.read(cx).about.open);
        assert!(
            w.find("about-distribution-unavailable")
                .label()
                .unwrap()
                .contains("公開準備中")
        );
        assert!(w.try_find("about-license-missing").is_none());
        w.click("about-license", cx);
        assert_eq!(
            manager.read(cx).about.license_text.as_deref(),
            Some("fixture license")
        );
        w.press("escape", cx);
        w.render_frame(cx);
        assert!(manager.read(cx).about.open);
        assert!(manager.read(cx).about.license_text.is_none());
    })
    .unwrap();
    assert_eq!(cx.opened_url(), None);
    cx.update_window(window, |_, w, cx| {
        w.click("about-third-party", cx);
    })
    .unwrap();
    assert_eq!(
        cx.opened_url().as_deref(),
        root.join("licenses/THIRD_PARTY_LICENSES.html").to_str()
    );
    cx.update_window(window, |_, w, cx| {
        manager.update(cx, |m, cx| {
            m.about.distribution_url = Some("https://example.org/releases");
            cx.notify();
        });
        w.render_frame(cx);
        w.click("about-distribution", cx);
    })
    .unwrap();
    assert_eq!(
        cx.opened_url().as_deref(),
        Some("https://example.org/releases")
    );
    std::fs::remove_file(root.join("LICENSE")).unwrap();
    cx.update_window(window, |_, w, cx| {
        manager.update(cx, |m, cx| {
            m.about.distribution_url = None;
            m.settings.language = "en".into();
            cx.notify();
        });
        w.render_frame(cx);
        assert!(
            w.find("about-distribution-unavailable")
                .label()
                .unwrap()
                .contains("not available")
        );
        assert!(
            w.find("about-license-missing")
                .label()
                .unwrap()
                .contains("not found")
        );
        assert!(w.try_find("about-third-party-missing").is_none());
        w.click("about-license", cx);
        w.click("about-distribution", cx);
    })
    .unwrap();
    assert_eq!(
        cx.opened_url().as_deref(),
        Some("https://example.org/releases")
    );
    cx.update_window(window, |_, w, cx| {
        w.press("escape", cx);
        w.render_frame(cx);
        assert!(!manager.read(cx).about.open);
        w.click("about-open", cx);
        w.render_frame(cx);
        w.click("navigate-back", cx);
        assert!(!manager.read(cx).about.open);
    })
    .unwrap();
}

#[test]
#[ignore = "隔離した保存先でアプリ情報画面を実描画する"]
fn about_visual_probe() {
    assert_eq!(std::env::var("WFI_GUI_PROBE").as_deref(), Ok("1"));
    let output = std::path::PathBuf::from(std::env::var("WFI_VISUAL_OUTPUT").unwrap());
    assert!(output.is_absolute());
    std::fs::create_dir_all(&output).unwrap();
    let files = home().unwrap().join("about-visual-fixture");
    std::fs::create_dir_all(files.join("licenses")).unwrap();
    std::fs::write(files.join("LICENSE"), include_str!("../../../../LICENSE")).unwrap();
    std::fs::write(files.join("licenses/THIRD_PARTY_LICENSES.html"), "notices").unwrap();
    gpui_kit::application()
        .with_assets(crate::ui_assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            let (window, manager) =
                gpui_kit::open_window(WindowOptions::default(), cx, Manager::open).unwrap();
            cx.spawn(async move |cx| {
                for (name, lang, mode, width, height, font, present) in [
                    ("ja-dark", "ja", ThemeMode::Dark, 1120., 780., 16., true),
                    ("en-light", "en", ThemeMode::Light, 1120., 780., 16., true),
                    (
                        "ja-narrow-missing",
                        "ja",
                        ThemeMode::Dark,
                        640.,
                        700.,
                        18.,
                        false,
                    ),
                    (
                        "en-narrow-missing",
                        "en",
                        ThemeMode::Light,
                        640.,
                        700.,
                        18.,
                        false,
                    ),
                ] {
                    window
                        .update(cx, |_, w, cx| {
                            theme::apply(mode, cx);
                            gpui_kit::component::theme::Theme::update(cx, |theme| {
                                theme.font_size = px(font)
                            });
                            manager.update(cx, |m, cx| {
                                m.about.open = true;
                                m.about.executable_dir = Some(if present {
                                    files.clone()
                                } else {
                                    files.join("missing")
                                });
                                m.settings.language = lang.into();
                                m.about.scroll.set_offset(gpui_kit::point(px(0.), px(0.)));
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
                            if !present {
                                w.scroll(
                                    "about-scroll",
                                    gpui_kit::ScrollDelta::Pixels(gpui_kit::point(
                                        px(0.),
                                        px(-1200.),
                                    )),
                                    cx,
                                );
                                w.render_frame(cx);
                                w.render_to_image()
                                    .unwrap()
                                    .save(output.join(format!("{name}-scrolled.png")))
                                    .unwrap();
                            }
                        })
                        .unwrap();
                }
                window
                    .update(cx, |_, w, cx| {
                        theme::apply(ThemeMode::Dark, cx);
                        manager.update(cx, |m, cx| {
                            m.settings.language = "ja".into();
                            m.about.license_text =
                                Some(include_str!("../../../../LICENSE").to_owned());
                            cx.notify();
                        });
                        w.resize(size(px(1120.), px(780.)));
                        w.render_frame(cx);
                        w.render_to_image()
                            .unwrap()
                            .save(output.join("ja-license.png"))
                            .unwrap();
                    })
                    .unwrap();
                cx.update(|cx| cx.quit());
            })
            .detach();
        });
}
