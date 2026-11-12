//! 手動の描画品質確認用。通常の利用者設定を使わず、実DirectX描画をPNGへ出力する。
use super::*;
use gpui_kit::application;

#[test]
#[ignore = "隔離した設定と復元記録の非対応形式を実描画で確認する"]
fn unsupported_settings_visual_probe() {
    assert_eq!(std::env::var("WFI_GUI_PROBE").as_deref(), Ok("1"));
    let output = std::path::PathBuf::from(std::env::var("WFI_VISUAL_OUTPUT").unwrap());
    assert!(output.is_absolute() && home().unwrap().starts_with(output.parent().unwrap()));
    std::fs::create_dir_all(&output).unwrap();
    let root = home().unwrap();
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("settings.json");
    file_icons_windows::save_to(&file_icons_windows::Settings::default(), &path).unwrap();
    let mut settings: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    settings["format_version"] = 3.into();
    std::fs::write(&path, serde_json::to_vec(&settings).unwrap()).unwrap();
    let record = serde_json::json!({
        "version": 2, "active": null, "fallbacks": {}, "original_handlers": {}, "created_keys": [],
        "journal": [{"key":"Software\\Classes\\VisualProbe\\DefaultIcon", "name":"", "before":null, "after":{"kind":1, "bytes":[0,0]}}]
    });
    std::fs::write(
        root.join("recovery.json"),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();
    application()
        .with_assets(crate::ui_assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            let (window, manager) =
                gpui_kit::open_window(WindowOptions::default(), cx, Manager::open).unwrap();
            cx.spawn(async move |cx| {
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
                                cx.notify();
                            });
                            w.resize(size(px(width), px(780.)));
                        })
                        .unwrap();
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(400))
                        .await;
                    window
                        .update(cx, |_, w, cx| {
                            w.render_frame(cx);
                            w.render_to_image()
                                .unwrap()
                                .save(output.join(format!("unsupported-{language}-{width}.png")))
                                .unwrap();
                        })
                        .unwrap();
                }
                cx.update(|cx| cx.quit());
            })
            .detach();
        });
}

#[test]
#[ignore = "隔離した保存先でパック見本と設定コピーの実描画を確認する"]
fn auxiliary_visual_probe() {
    assert_eq!(std::env::var("WFI_GUI_PROBE").as_deref(), Ok("1"));
    let output = std::path::PathBuf::from(std::env::var("WFI_VISUAL_OUTPUT").unwrap());
    assert!(output.is_absolute() && home().unwrap().starts_with(output.parent().unwrap()));
    std::fs::create_dir_all(&output).unwrap();
    let registry = super::target_protection::fixture();
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
            manager.update(cx, |this, _| {
                this.settings.packs[1].label =
                    "A long icon pack name / 日本語の長いアイコンパック名".into();
                let mut selection = file_icons_core::selection::Selection::default();
                selection.set("extension:json", Some(false)).unwrap();
                this.settings
                    .selections
                    .insert(this.settings.packs[0].id.clone(), selection);
            });
            cx.spawn(async move |cx| {
                for (language, mode, width, font) in [
                    ("ja", ThemeMode::Dark, 1120., 16.),
                    ("en", ThemeMode::Light, 1120., 16.),
                    ("ja", ThemeMode::Dark, 640., 18.),
                    ("en", ThemeMode::Light, 640., 18.),
                ] {
                    for copy in [false, true] {
                        window
                            .update(cx, |_, w, cx| {
                                theme::apply(mode, cx);
                                gpui_kit::component::theme::Theme::update(cx, |theme| {
                                    theme.font_size = px(font)
                                });
                                manager.update(cx, |this, cx| {
                                    this.settings.language = language.into();
                                    this.prepare_auxiliary_visual(copy);
                                    cx.notify();
                                });
                                w.resize(size(px(width), px(780.)));
                            })
                            .unwrap();
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(400))
                            .await;
                        window
                            .update(cx, |_, w, cx| {
                                w.render_frame(cx);
                                w.render_to_image()
                                    .unwrap()
                                    .save(output.join(format!(
                                        "{}-{language}-{width}.png",
                                        if copy { "copy" } else { "preview" }
                                    )))
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

#[test]
#[ignore = "専用プロファイルと隔離レジストリで対象選択・保護の実描画を確認する"]
fn target_protection_visual_probe() {
    assert_eq!(std::env::var("WFI_GUI_PROBE").as_deref(), Ok("1"));
    let output = std::path::PathBuf::from(std::env::var("WFI_VISUAL_OUTPUT").unwrap());
    assert!(output.is_absolute() && home().unwrap().starts_with(output.parent().unwrap()));
    std::fs::create_dir_all(&output).unwrap();
    let registry = super::target_protection::fixture();
    let settings = load().unwrap();
    let sources =
        file_icons_windows::inspect_existing_icons(&settings.packs[0], &settings).unwrap();
    // 実ウィンドウの初期化には通常のWindows環境を使い、描画には隔離調査の結果を渡す。
    drop(registry);
    application()
        .with_assets(crate::ui_assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            // 一覧の配置確認中に、前の画面のツールチップを重ねない。
            cx.set_global(
                gpui_kit::component::tooltip::TooltipDefaults::new()
                    .with_show_delay(std::time::Duration::from_secs(3600)),
            );
            theme::apply(ThemeMode::Dark, cx);
            let (window, manager) =
                gpui_kit::open_window(WindowOptions::default(), cx, Manager::open).unwrap();
            manager.update(cx, |this, cx| {
                let id = this.settings.packs[0].id.clone();
                this.open_targets(&id, cx);
                this.protection.visual_sources(sources);
            });
            cx.spawn(async move |cx| {
                for (name, width, height, font, mode, language, state) in [
                    (
                        "overview-ja-dark",
                        1120.,
                        780.,
                        16.,
                        ThemeMode::Dark,
                        "ja",
                        "overview",
                    ),
                    (
                        "protected-en-light",
                        1120.,
                        780.,
                        16.,
                        ThemeMode::Light,
                        "en",
                        "protected",
                    ),
                    (
                        "mts-search-filtered",
                        1120.,
                        780.,
                        16.,
                        ThemeMode::Dark,
                        "ja",
                        "filtered",
                    ),
                    (
                        "mts-edit-ja-dark",
                        1120.,
                        780.,
                        16.,
                        ThemeMode::Dark,
                        "ja",
                        "edit",
                    ),
                    (
                        "mts-independent-en-light",
                        1120.,
                        780.,
                        16.,
                        ThemeMode::Light,
                        "en",
                        "allow",
                    ),
                    (
                        "mts-allow-ja-dark",
                        1120.,
                        780.,
                        16.,
                        ThemeMode::Dark,
                        "ja",
                        "allow",
                    ),
                    (
                        "mts-narrow-ja",
                        640.,
                        640.,
                        18.,
                        ThemeMode::Dark,
                        "ja",
                        "edit",
                    ),
                    (
                        "mts-narrow-en",
                        640.,
                        640.,
                        18.,
                        ThemeMode::Light,
                        "en",
                        "edit",
                    ),
                    (
                        "unsupported-en",
                        1120.,
                        780.,
                        16.,
                        ThemeMode::Light,
                        "en",
                        "unsupported",
                    ),
                    (
                        "mts-narrow-allow-ja",
                        640.,
                        640.,
                        18.,
                        ThemeMode::Dark,
                        "ja",
                        "allow",
                    ),
                    (
                        "mts-narrow-allow-en",
                        640.,
                        640.,
                        18.,
                        ThemeMode::Light,
                        "en",
                        "allow",
                    ),
                    (
                        "mts-fallback-ja",
                        1120.,
                        780.,
                        16.,
                        ThemeMode::Dark,
                        "ja",
                        "fallback",
                    ),
                    (
                        "mts-pending-ja",
                        1120.,
                        780.,
                        16.,
                        ThemeMode::Dark,
                        "ja",
                        "pending",
                    ),
                    (
                        "mts-pending-en",
                        640.,
                        640.,
                        18.,
                        ThemeMode::Light,
                        "en",
                        "pending",
                    ),
                ] {
                    window
                        .update(cx, |_, w, cx| {
                            theme::apply(mode, cx);
                            gpui_kit::component::theme::Theme::update(cx, |theme| {
                                theme.font_size = px(font)
                            });
                            manager.update(cx, |this, cx| {
                                assert!(
                                    !this.protection.inspection.loading
                                        && this.protection.inspection.error.is_none()
                                );
                                this.settings.language = language.into();
                                this.prepare_target_protection_visual(state, w, cx);
                            });
                            w.resize(size(px(width), px(height)));
                        })
                        .unwrap();
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(400))
                        .await;
                    window
                        .update(cx, |_, w, cx| {
                            w.render_frame(cx);
                            w.render_to_image()
                                .unwrap()
                                .save(output.join(format!("{name}.png")))
                                .unwrap();
                            if state == "allow" || width == 640. {
                                let delta = if width == 640. {
                                    (w.find("target-rows").bounds().bottom()
                                        - w.find("default-icon-mts").bounds().bottom()
                                        - px(8.))
                                    .min(px(0.))
                                } else {
                                    px(-450.)
                                };
                                w.scroll(
                                    "target-rows",
                                    gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), delta)),
                                    cx,
                                );
                                w.render_frame(cx);
                                if width == 640. {
                                    let tile = w.find("default-icon-mts").bounds();
                                    let viewport = w.find("target-rows").bounds();
                                    assert!(
                                        tile.top() >= viewport.top()
                                            && tile.bottom() <= viewport.bottom()
                                    );
                                }
                                w.render_to_image()
                                    .unwrap()
                                    .save(output.join(format!("{name}-scrolled.png")))
                                    .unwrap();
                            }
                        })
                        .unwrap();
                }
                cx.update(|cx| cx.quit());
            })
            .detach();
        });
}

#[test]
#[ignore = "隔離した保存先でソフト単位の個別指定・初期化確認を実描画する"]
fn provider_reset_visual_probe() {
    assert_eq!(std::env::var("WFI_GUI_PROBE").as_deref(), Ok("1"));
    let output = std::path::PathBuf::from(std::env::var("WFI_VISUAL_OUTPUT").unwrap());
    assert!(output.is_absolute() && home().unwrap().starts_with(output.parent().unwrap()));
    std::fs::create_dir_all(&output).unwrap();
    let registry = super::target_protection::fixture();
    let active = std::env::var("WFI_VISUAL_ACTIVE").as_deref() == Ok("1");
    if active {
        let service = crate::ManagerService::acquire().unwrap();
        service.set_icon_protection(false, false).unwrap();
        let pack = service.settings().unwrap().packs[0].id.clone();
        let dll = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .join("file_icons_shell.dll");
        service.apply(&pack, &dll).unwrap();
        assert!(!service.settings().unwrap().journal.is_empty());
    }
    let settings = load().unwrap();
    let mut rows = file_icons_windows::inspect_existing_icons(&settings.packs[0], &settings)
        .unwrap()
        .into_iter()
        .filter(|row| matches!(row.registration.extension.as_str(), "mts" | "ts" | "m2ts"))
        .collect::<Vec<_>>();
    rows.sort_by_key(|row| match row.registration.extension.as_str() {
        "mts" => 0,
        "ts" => 1,
        _ => 2,
    });
    for row in &mut rows {
        row.provider_id = Some("visual:media".into());
        row.provider_name = Some("Example Media Player / 日本語の長いメディアアプリ名".into());
    }
    let mut other = rows[0].clone();
    if active {
        // パターンに一致するが、パックの直接の登録対象にはない種類。
        other.registration.extension = "wfi-test".into();
        other.registration.class = ".wfi-test".into();
    } else {
        other.registration.extension = "json".into();
        other.registration.class = "Fixture.Json".into();
    }
    other.provider_id = Some("visual:other".into());
    other.provider_name = Some("Another application".into());
    rows.push(other);
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
            window
                .update(cx, |_, w, cx| {
                    manager.update(cx, |this, cx| {
                        // 調査済みの隔離データを使うため、通常のレジストリを再調査しない。
                        let guard = this._guard.take();
                        this.open_protection(w, cx);
                        this._guard = guard;
                        this.protection.visual_sources(rows);
                    });
                })
                .unwrap();
            cx.spawn(async move |cx| {
                for (name, width, height, font, mode, lang) in [
                    ("provider-ja-dark", 1120., 780., 16., ThemeMode::Dark, "ja"),
                    (
                        "provider-en-light",
                        1120.,
                        780.,
                        16.,
                        ThemeMode::Light,
                        "en",
                    ),
                    ("provider-small-ja", 640., 640., 18., ThemeMode::Dark, "ja"),
                    ("provider-small-en", 640., 640., 18., ThemeMode::Light, "en"),
                ] {
                    window
                        .update(cx, |_, w, cx| {
                            theme::apply(mode, cx);
                            gpui_kit::component::theme::Theme::update(cx, |theme| {
                                theme.font_size = px(font)
                            });
                            manager.update(cx, |this, cx| {
                                this.settings.language = lang.into();
                                this.protection
                                    .visual_sources(this.protection.inspection.sources.clone());
                                this.protection.draft.rules = Default::default();
                                this.protection.draft.enabled = true;
                                this.protection.draft.rules.set_provider(
                                    &this.protection.inspection.sources,
                                    "visual:media",
                                    Some(false),
                                );
                                for row in &this.protection.inspection.sources {
                                    if matches!(row.registration.extension.as_str(), "ts" | "m2ts")
                                    {
                                        this.protection.draft.rules.set_extension(row, Some(true));
                                    }
                                }
                                // 保存済みの個別指定を解除する場面として、下書きとの差を実描画する。
                                this.settings.preserve_existing_icons = true;
                                this.settings.icon_protection_rules =
                                    this.protection.draft.rules.clone();
                                this.refresh_protection_policy(cx);
                            });
                            w.resize(size(px(width), px(height)));
                            w.render_frame(cx);
                            if w.try_find("protection-planned").is_some() {
                                w.click("protection-planned", cx);
                            } else {
                                w.click("protection-view-select", cx);
                                for _ in 0..5 {
                                    w.press("up", cx);
                                }
                                w.press("enter", cx);
                            }
                            w.render_frame(cx);
                            w.scroll(
                                "protection-rows",
                                gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(10000.))),
                                cx,
                            );
                        })
                        .unwrap();
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(400))
                        .await;
                    for state in [
                        "overrides",
                        "overrides-scrolled",
                        "global",
                        "warning",
                        "reset",
                        "reset-scrolled",
                        "filtered",
                        "filtered-edited",
                    ] {
                        window
                            .update(cx, |_, w, cx| {
                                w.render_frame(cx);
                                match state {
                                    "global" => {
                                        w.scroll(
                                            "protection-rows",
                                            gpui_kit::ScrollDelta::Pixels(gpui_kit::point(
                                                px(0.),
                                                px(10000.),
                                            )),
                                            cx,
                                        );
                                        w.render_frame(cx);
                                        w.click("provider-default-visual:media", cx);
                                    }
                                    "warning" => {
                                        for _ in 0..4 {
                                            if w.find("provider-reset-visual:media")
                                                .bounds()
                                                .bottom()
                                                <= w.find("protection-rows").bounds().bottom()
                                            {
                                                break;
                                            }
                                            w.scroll(
                                                "protection-rows",
                                                gpui_kit::ScrollDelta::Pixels(gpui_kit::point(
                                                    px(0.),
                                                    px(-100.),
                                                )),
                                                cx,
                                            );
                                            w.render_frame(cx);
                                        }
                                        w.click("provider-reset-visual:media", cx);
                                    }
                                    "reset" => w.within("dialog").click("ok", cx),
                                    "filtered" => {
                                        if w.try_find("protection-protected").is_some() {
                                            w.click("protection-protected", cx);
                                        } else {
                                            w.click("protection-view-select", cx);
                                            for _ in 0..5 {
                                                w.press("up", cx);
                                            }
                                            w.press("down", cx);
                                            w.press("enter", cx);
                                        }
                                        w.scroll(
                                            "protection-rows",
                                            gpui_kit::ScrollDelta::Pixels(gpui_kit::point(
                                                px(0.),
                                                px(10000.),
                                            )),
                                            cx,
                                        );
                                    }
                                    "filtered-edited" => {
                                        for _ in 0..30 {
                                            w.render_frame(cx);
                                            let visible =
                                                w.try_find("pack-icon-mts").is_some_and(|node| {
                                                    let viewport =
                                                        w.find("protection-rows").bounds();
                                                    node.visible()
                                                        && node.bounds().top() >= viewport.top()
                                                        && node.bounds().bottom()
                                                            <= viewport.bottom()
                                                });
                                            if visible {
                                                break;
                                            }
                                            w.scroll(
                                                "protection-rows",
                                                gpui_kit::ScrollDelta::Pixels(gpui_kit::point(
                                                    px(0.),
                                                    px(-100.),
                                                )),
                                                cx,
                                            );
                                        }
                                        w.render_frame(cx);
                                        w.click("pack-icon-mts", cx);
                                        assert!(w.try_find("protection-row-mts").is_some());
                                    }
                                    "overrides-scrolled" | "reset-scrolled" => w.scroll(
                                        "protection-rows",
                                        gpui_kit::ScrollDelta::Pixels(gpui_kit::point(
                                            px(0.),
                                            px(-600.),
                                        )),
                                        cx,
                                    ),
                                    _ => {}
                                }
                                w.render_frame(cx);
                                if state == "overrides" {
                                    w.hover("protection-back", cx);
                                    w.render_frame(cx);
                                    assert_eq!(
                                        w.find("provider-pack-visual:media").checked(),
                                        Some(true)
                                    );
                                } else if state == "global" {
                                    assert_eq!(
                                        w.find("provider-default-visual:media").checked(),
                                        Some(true)
                                    );
                                    assert!(
                                        manager.read(cx).protection.draft.rules.extensions["ts"]
                                            .protect
                                    );
                                } else if state == "warning" {
                                    let dialog = w.find("dialog");
                                    assert!(dialog.visible());
                                    for id in ["ok", "cancel"] {
                                        let button = w.within("dialog").find(id);
                                        assert!(
                                            button.visible()
                                                && button.bounds().bottom()
                                                    <= w.viewport_size().height
                                        );
                                    }
                                } else if state == "reset" {
                                    assert_eq!(
                                        w.find("provider-default-visual:media").checked(),
                                        Some(true)
                                    );
                                }
                            })
                            .unwrap();
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(200))
                            .await;
                        window
                            .update(cx, |_, w, cx| {
                                w.render_frame(cx);
                                w.render_to_image()
                                    .unwrap()
                                    .save(output.join(format!("{name}-{state}.png")))
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

#[test]
#[ignore = "隔離した保存先でパック一覧と確認ダイアログを実描画する"]
fn library_visual_probe() {
    assert_eq!(std::env::var("WFI_GUI_PROBE").as_deref(), Ok("1"));
    let output = std::path::PathBuf::from(std::env::var("WFI_VISUAL_OUTPUT").unwrap());
    assert!(output.is_absolute() && home().unwrap().starts_with(output.parent().unwrap()));
    std::fs::create_dir_all(&output).unwrap();
    let fixture = home().unwrap().join("visual-fixture");
    std::fs::create_dir_all(&fixture).unwrap();
    std::fs::write(fixture.join("package.json"), r#"{
        "name":"visual-library", "publisher":"test", "version":"1.0.0",
        "contributes":{"iconThemes":[
            {"id":"sample","label":"Sample icons","path":"theme.json"},
            {"id":"long","label":"A very long icon pack name with Japanese 日本語の長いアイコンパック名 and version details","path":"theme.json"}
        ]}}
    "#).unwrap();
    std::fs::write(
        fixture.join("theme.json"),
        r#"{
        "iconDefinitions":{"icon":{"iconPath":"icon.svg"}},
        "fileExtensions":{"json":"icon","md":"icon","js":"icon","ts":"icon","py":"icon","rs":"icon","html":"icon"},
        "fileNames":{"Cargo.toml":"icon","package.json":"icon","README.md":"icon"}}
    "#,
    )
    .unwrap();
    std::fs::write(fixture.join("icon.svg"), r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="16" height="16" rx="3" fill="#7760dd"/></svg>"##).unwrap();
    {
        let manager = crate::ManagerService::acquire().unwrap();
        for id in ["sample", "long"] {
            manager.import(&fixture, id, &Default::default()).unwrap();
        }
    }
    application()
        .with_assets(crate::ui_assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            let (window, manager) =
                gpui_kit::open_window(WindowOptions::default(), cx, Manager::open).unwrap();
            cx.spawn(async move |cx| {
                for (name, width, height, font, mode, language, page) in [
                    (
                        "library-ja-dark",
                        1000.,
                        760.,
                        16.,
                        ThemeMode::Dark,
                        "ja",
                        "library",
                    ),
                    (
                        "library-en-light",
                        1000.,
                        760.,
                        16.,
                        ThemeMode::Light,
                        "en",
                        "library",
                    ),
                    (
                        "library-small-ja",
                        640.,
                        640.,
                        18.,
                        ThemeMode::Dark,
                        "ja",
                        "library",
                    ),
                    (
                        "library-small-en",
                        640.,
                        640.,
                        18.,
                        ThemeMode::Light,
                        "en",
                        "library",
                    ),
                    (
                        "catalog-small-en",
                        640.,
                        640.,
                        18.,
                        ThemeMode::Light,
                        "en",
                        "browse",
                    ),
                    (
                        "remove-small-en",
                        640.,
                        640.,
                        18.,
                        ThemeMode::Light,
                        "en",
                        "remove",
                    ),
                    (
                        "library-conflicts-ja-dark",
                        1000.,
                        760.,
                        16.,
                        ThemeMode::Dark,
                        "ja",
                        "conflicts",
                    ),
                    (
                        "import-small-ja-dark",
                        640.,
                        640.,
                        18.,
                        ThemeMode::Dark,
                        "ja",
                        "import",
                    ),
                    (
                        "import-small-en-light",
                        640.,
                        640.,
                        18.,
                        ThemeMode::Light,
                        "en",
                        "import",
                    ),
                    (
                        "import-cancelling-small-ja-dark",
                        640.,
                        640.,
                        18.,
                        ThemeMode::Dark,
                        "ja",
                        "import-cancelling",
                    ),
                    (
                        "import-cancelling-small-en-light",
                        640.,
                        640.,
                        18.,
                        ThemeMode::Light,
                        "en",
                        "import-cancelling",
                    ),
                ] {
                    window
                        .update(cx, |_, w, cx| {
                            theme::apply(mode, cx);
                            gpui_kit::component::theme::Theme::update(cx, |theme| {
                                theme.font_size = px(font)
                            });
                            manager.update(cx, |view, cx| {
                                view.settings.language = language.into();
                                view.browsing = page == "browse";
                                view.conflict_attention = page == "conflicts";
                                if page.starts_with("import") {
                                    let progress = std::sync::Arc::new(
                                        file_icons_catalog::Progress::default(),
                                    );
                                    if page == "import-cancelling" {
                                        progress.cancel();
                                    }
                                    view.pending = Some((
                                        fixture.clone(),
                                        file_icons_core::discover(&fixture).unwrap(),
                                    ));
                                    view.operation.begin(crate::Kind::Import, Some(progress));
                                }
                                cx.notify();
                            });
                            w.resize(size(px(width), px(height)));
                        })
                        .unwrap();
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(300))
                        .await;
                    if page == "remove" {
                        window
                            .update(cx, |_, w, cx| {
                                w.render_frame(cx);
                                w.click(pack_button(&manager, cx, "remove-pack", 0), cx);
                            })
                            .unwrap();
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(300))
                            .await;
                    }
                    window
                        .update(cx, |_, w, cx| {
                            w.render_frame(cx);
                            w.render_to_image()
                                .unwrap()
                                .save(output.join(format!("{name}.png")))
                                .unwrap();
                            if page == "remove" {
                                w.press("escape", cx);
                                w.render_frame(cx);
                            }
                        })
                        .unwrap();
                }
                for active in [false, true, false] {
                    window
                        .update(cx, |_, w, cx| {
                            theme::apply(ThemeMode::Dark, cx);
                            manager.update(cx, |view, cx| {
                                view.settings.language = "ja".into();
                                view.browsing = false;
                                view.conflict_attention = false;
                                view.pending = None;
                                view.operation = crate::operation::Operation::default();
                                view.settings.active =
                                    active.then(|| view.settings.packs[0].id.clone());
                                cx.notify();
                            });
                            w.resize(size(px(1000.), px(760.)));
                            w.render_frame(cx);
                            w.hover("app-brand", cx);
                            w.render_frame(cx);
                            w.hover(pack_button(&manager, cx, "remove-pack", 0), cx);
                            w.render_frame(cx);
                        })
                        .unwrap();
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(1000))
                        .await;
                    window
                        .update(cx, |_, w, cx| {
                            w.render_frame(cx);
                            let name = if active {
                                "remove-tooltip-active"
                            } else {
                                "remove-tooltip-normal"
                            };
                            w.render_to_image()
                                .unwrap()
                                .save(output.join(format!("{name}.png")))
                                .unwrap();
                        })
                        .unwrap();
                }
                cx.update(|cx| cx.quit());
            })
            .detach();
        });
}

#[test]
#[ignore = "隔離したプロファイルとWFI_VISUAL_OUTPUTを明示する描画確認専用"]
fn target_editor_visual_probe() {
    assert_eq!(std::env::var("WFI_GUI_PROBE").as_deref(), Ok("1"));
    let output = std::path::PathBuf::from(std::env::var("WFI_VISUAL_OUTPUT").unwrap());
    assert!(output.is_absolute());
    // 出力と同じ親の下に隔離保存先を用意し、通常の利用者設定での誤実行を拒否する。
    assert!(home().unwrap().starts_with(output.parent().unwrap()));
    std::fs::create_dir_all(&output).unwrap();
    application()
        .with_assets(crate::ui_assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            theme::apply(ThemeMode::Dark, cx);
            let (window, manager) = gpui_kit::open_window(
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
            .unwrap();
            manager.update(cx, |this, cx| {
                let id = this
                    .settings
                    .packs
                    .iter()
                    .find(|pack| pack.label.contains("Material"))
                    .or_else(|| this.settings.packs.first())
                    .unwrap()
                    .id
                    .clone();
                this.open_targets(&id, cx);
            });
            cx.spawn(async move |cx| {
                for (name, width, height, font, mode, language, category_view) in [
                    (
                        "targets-ja-dark",
                        1120.,
                        780.,
                        16.,
                        ThemeMode::Dark,
                        "ja",
                        "normal",
                    ),
                    (
                        "targets-en-light",
                        1120.,
                        780.,
                        16.,
                        ThemeMode::Light,
                        "en",
                        "normal",
                    ),
                    (
                        "targets-small-ja",
                        640.,
                        640.,
                        18.,
                        ThemeMode::Dark,
                        "ja",
                        "normal",
                    ),
                    (
                        "targets-small-en",
                        640.,
                        640.,
                        18.,
                        ThemeMode::Light,
                        "en",
                        "normal",
                    ),
                    (
                        "categories-small-ja",
                        640.,
                        640.,
                        18.,
                        ThemeMode::Dark,
                        "ja",
                        "categories",
                    ),
                    (
                        "categories-small-en",
                        640.,
                        640.,
                        18.,
                        ThemeMode::Light,
                        "en",
                        "categories",
                    ),
                    (
                        "categories-focus-ja",
                        1120.,
                        780.,
                        16.,
                        ThemeMode::Dark,
                        "ja",
                        "focus",
                    ),
                ] {
                    window
                        .update(cx, |_, w, cx| {
                            theme::apply(mode, cx);
                            gpui_kit::component::theme::Theme::update(cx, |theme| {
                                theme.font_size = px(font)
                            });
                            manager.update(cx, |this, cx| {
                                this.settings.language = language.into();
                                this.prepare_category_visual(category_view, w, cx);
                                cx.notify();
                            });
                            w.resize(size(px(width), px(height)));
                        })
                        .unwrap();
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(500))
                        .await;
                    window
                        .update(cx, |_, w, cx| {
                            w.render_frame(cx);
                            w.render_to_image()
                                .unwrap()
                                .save(output.join(format!("{name}.png")))
                                .unwrap();
                            if category_view == "normal" {
                                w.scroll(
                                    "target-rows",
                                    gpui_kit::ScrollDelta::Pixels(gpui_kit::point(
                                        px(0.),
                                        px(-900.),
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
                cx.update(|cx| cx.quit());
            })
            .detach();
        });
}

#[test]
#[ignore = "隔離プロファイルで復旧画面を実描画し、PNGへ保存する"]
fn recovery_visual_probe() {
    assert_eq!(std::env::var("WFI_GUI_PROBE").as_deref(), Ok("1"));
    let output = std::path::PathBuf::from(std::env::var("WFI_VISUAL_OUTPUT").unwrap());
    assert!(output.is_absolute() && home().unwrap().starts_with(output.parent().unwrap()));
    std::fs::create_dir_all(&output).unwrap();
    application().with_assets(crate::ui_assets::Assets).run(move |cx| {
        gpui_kit::init(cx);
        theme::apply(ThemeMode::Dark, cx);
        let (window, manager) = gpui_kit::open_window(WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(1120.), px(780.)), cx))),
            ..Default::default()
        }, cx, Manager::open).unwrap();
        manager.update(cx, |this, cx| {
            use file_icons_windows::conflict::{Entry, Plan, Strategy};
            let value = |s: &str| Some(file_icons_core::registry::Value { kind: 1, bytes: s.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect() });
            let entries = (0..1864).map(|ix| Entry {
                key: format!("Software\\Classes\\Adobe.Illustrator.Dictionary{ix}\\DefaultIcon"),
                name: String::new(), extensions: vec!["docx".into(), "ai".into(), "eps".into()],
                state: if ix < 17 { "external" } else if ix == 17 { "applied" } else { "original" }.into(),
                original: value("C:\\Program Files\\Adobe\\Adobe Illustrator 2026\\Support Files\\Contents\\Windows\\Illustrator.exe,1"),
                recorded: value("%1"), current: if ix == 17 { value("%1") } else { value("C:\\Program Files\\Adobe\\Adobe Illustrator 2026\\Support Files\\Contents\\Windows\\Illustrator.exe,1") },
                planned: value("C:\\Program Files\\Adobe\\Adobe Illustrator 2026\\Support Files\\Contents\\Windows\\Illustrator.exe,1"),
            }).collect();
            let plan = Plan {
                schema_version: 1, confirmation: "0".repeat(64), strategy: Strategy::PreserveExternal,
                pack_id: None, pack_label: None, pack_version: None, selection: Default::default(),
                backup: home().unwrap().join("conflict-backups").join("0".repeat(64)),
                resuming: false, conflicts: 17, writes: 1, overwrites: 1, entries,
            };
            // 描画用の値だけを設定する。登録の読み書きや復旧の実行は行わない。
            this.recovery = crate::recovery_ui::Editor::visual(plan);
            this.screen = crate::Screen::Recovery;


            cx.notify();
        });
        cx.spawn(async move |cx| {
            for (name, width, height, font, mode, lang, reviewed, completed) in [
                ("recovery-ja-dark", 1120., 780., 16., ThemeMode::Dark, "ja", false, false),
                ("recovery-en-light", 1120., 780., 16., ThemeMode::Light, "en", false, false),
                ("recovery-small-ja", 640., 700., 18., ThemeMode::Dark, "ja", false, false),
                ("recovery-confirm-small-en", 640., 700., 18., ThemeMode::Light, "en", true, false),
                ("recovery-completed", 1120., 780., 16., ThemeMode::Dark, "ja", false, true),
            ] {
                window.update(cx, |_, w, cx| {
                    theme::apply(mode, cx);
                    gpui_kit::component::theme::Theme::update(cx, |t| t.font_size = px(font));
                    manager.update(cx, |this, cx| {
                        this.settings.language = lang.into(); this.recovery.visual_stage(reviewed, completed);
                         cx.notify();
                    });
                    w.resize(size(px(width), px(height)));
                }).unwrap();
                cx.background_executor().timer(std::time::Duration::from_millis(500)).await;
                window.update(cx, |_, w, cx| {
                    w.render_frame(cx);
                    w.render_to_image().unwrap().save(output.join(format!("{name}.png"))).unwrap();
                }).unwrap();
            }
            cx.update(|cx| cx.quit());
        }).detach();
    });
}

#[test]
#[ignore = "隔離した保存先で拡張子なしの独立表示と対応名の見本を実描画する"]
fn extensionless_protection_visual_probe() {
    assert_eq!(std::env::var("WFI_GUI_PROBE").as_deref(), Ok("1"));
    let output = std::path::PathBuf::from(std::env::var("WFI_VISUAL_OUTPUT").unwrap());
    assert!(output.is_absolute() && home().unwrap().starts_with(output.parent().unwrap()));
    std::fs::create_dir_all(&output).unwrap();
    let registry = super::extensionless_protection::fixture();
    let settings = load().unwrap();
    let mut rows = file_icons_windows::inspect_existing_icons(&settings.packs[0], &settings)
        .unwrap()
        .into_iter()
        .filter(|row| row.registration.extension.is_empty() || row.registration.extension == "mts")
        .collect::<Vec<_>>();
    for row in &mut rows {
        if !row.registration.extension.is_empty() {
            row.provider_id = Some("application:order-fixture".into());
            row.provider_name = Some("Example Editor".into());
        }
    }
    application()
        .with_assets(crate::ui_assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            let (window, manager) =
                gpui_kit::open_window(WindowOptions::default(), cx, Manager::open).unwrap();
            window
                .update(cx, |_, w, cx| {
                    manager.update(cx, |this, cx| {
                        let guard = this._guard.take();
                        this.open_protection(w, cx);
                        this._guard = guard;
                        this.accept_protection_sources(rows);
                        this.refresh_protection_policy(cx);
                    })
                })
                .unwrap();
            cx.spawn(async move |cx| {
                let _registry = registry;
                for (name, width, height, font, mode, language) in [
                    (
                        "extensionless-ja-dark",
                        1120.,
                        900.,
                        16.,
                        ThemeMode::Dark,
                        "ja",
                    ),
                    (
                        "extensionless-en-light",
                        1120.,
                        900.,
                        16.,
                        ThemeMode::Light,
                        "en",
                    ),
                    (
                        "extensionless-small-ja",
                        640.,
                        640.,
                        18.,
                        ThemeMode::Dark,
                        "ja",
                    ),
                    (
                        "extensionless-small-en",
                        640.,
                        640.,
                        18.,
                        ThemeMode::Light,
                        "en",
                    ),
                ] {
                    window
                        .update(cx, |_, w, cx| {
                            theme::apply(mode, cx);
                            gpui_kit::component::theme::Theme::update(cx, |theme| {
                                theme.font_size = px(font)
                            });
                            manager.update(cx, |this, cx| {
                                this.settings.language = language.into();
                                let sources = this.protection.inspection.sources.clone();
                                let guard = this._guard.take();
                                this.open_protection(w, cx);
                                this._guard = guard;
                                this.accept_protection_sources(sources);
                                this.protection
                                    .visual_sources(this.protection.inspection.sources.clone());
                                this.protection.draft.rules = Default::default();
                                this.refresh_protection_policy(cx);
                            });
                            w.resize(size(px(width), px(height)));
                        })
                        .unwrap();
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(400))
                        .await;
                    window
                        .update(cx, |_, w, cx| {
                            w.render_frame(cx);
                            w.press("home", cx);
                            w.render_frame(cx);
                            w.render_to_image()
                                .unwrap()
                                .save(output.join(format!("{name}-protected.png")))
                                .unwrap();
                            w.click("protection-collapse-all", cx);
                            w.render_frame(cx);
                            assert!(w.try_find("protection-extensionless-scope").is_none());
                            w.render_to_image()
                                .unwrap()
                                .save(output.join(format!("{name}-collapsed.png")))
                                .unwrap();
                            w.click("protection-expand-all", cx);
                            w.render_frame(cx);
                            super::protection_sources::reveal(w, "pack-icon-", cx);
                            w.click("pack-icon-", cx);
                            w.render_frame(cx);
                            assert!(w.find("protection-back").visible());
                            w.render_to_image()
                                .unwrap()
                                .save(output.join(format!("{name}-allowed.png")))
                                .unwrap();
                            w.click("protection-details", cx);
                            w.render_frame(cx);
                            super::protection_sources::reveal(
                                w,
                                "protection-extensionless-provider",
                                cx,
                            );
                            w.render_to_image()
                                .unwrap()
                                .save(output.join(format!("{name}-details.png")))
                                .unwrap();
                        })
                        .unwrap();
                }
                cx.update(|cx| cx.quit());
            })
            .detach();
        });
}

#[test]
#[ignore = "隔離した保存先で既存アイコンの保護画面を実描画する"]
fn protection_visual_probe() {
    assert_eq!(std::env::var("WFI_GUI_PROBE").as_deref(), Ok("1"));
    let output = std::path::PathBuf::from(std::env::var("WFI_VISUAL_OUTPUT").unwrap());
    assert!(output.is_absolute() && home().unwrap().starts_with(output.parent().unwrap()));
    std::fs::create_dir_all(&output).unwrap();
    let fixture = home().unwrap().join("visual-fixture");
    std::fs::create_dir_all(&fixture).unwrap();
    std::fs::write(fixture.join("package.json"), r#"{
        "name":"visual-library", "publisher":"test", "version":"1.0.0",
        "contributes":{"iconThemes":[
            {"id":"sample","label":"Sample icons","path":"theme.json"},
            {"id":"long","label":"A very long icon pack name with Japanese 日本語の長いアイコンパック名 and version details","path":"theme.json"}
        ]}}
    "#).unwrap();
    std::fs::write(
        fixture.join("theme.json"),
        r#"{
        "iconDefinitions":{"icon":{"iconPath":"icon.svg"}},
        "fileExtensions":{"json":"icon","md":"icon","rs":"icon","document-0":"icon"},
        "fileNames":{"Cargo.toml":"icon","package.json":"icon","README.md":"icon"}}
    "#,
    )
    .unwrap();
    std::fs::write(fixture.join("icon.svg"), r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="16" height="16" rx="3" fill="#7760dd"/></svg>"##).unwrap();
    {
        let manager = crate::ManagerService::acquire().unwrap();
        for id in ["sample", "long"] {
            manager.import(&fixture, id, &Default::default()).unwrap();
        }
    }
    application().with_assets(crate::ui_assets::Assets).run(move |cx| {
        gpui_kit::init(cx);
        let (window, manager) = gpui_kit::open_window(WindowOptions::default(), cx, Manager::open).unwrap();
        window.update(cx, |_, w, cx| {
          manager.update(cx, |this, cx| {
            this.open_protection(w, cx);
            use file_icons_windows::{IconExclusion, ProtectionReason};
            this.protection.visual_sources((0..136).map(|ix| file_icons_windows::ExistingIcon {
                registration: IconExclusion { extension: format!("document-{ix}"), class: format!("Applications.Example.Document.{ix}"),
                    reason: ProtectionReason::DefaultIcon, source: format!("Applications.Example.Document.{ix}\\DefaultIcon") },
                provider_id: Some("module:windows-shell".into()),
                provider_name: Some("A very long application name 日本語の長いソフト名 with icon resources".into()),
                provider_inferred: false, open_app: Some("別の既定アプリ / A different default app".into()),
                fingerprint: format!("visual-{ix}"), reference: "shell32.dll,2".into(), location: if ix == 0 { None } else { Some(file_icons_windows::IconLocation { path: "shell32.dll".into(), index: 2 }) },
                provider_location: Some(file_icons_windows::IconLocation { path: "shell32.dll".into(), index: 0 }),
                protect: true, needs_review: false, always_protected: false,
            }).collect());
            this.refresh_protection_policy(cx);
            cx.notify();
          });
        }).unwrap();
        cx.spawn(async move |cx| {
            for (name, width, height, font, mode, lang, details) in [
                ("protection-ja-dark", 1000., 760., 16., ThemeMode::Dark, "ja", false),
                ("protection-en-light", 1000., 760., 16., ThemeMode::Light, "en", true),
                ("protection-small-ja", 640., 640., 18., ThemeMode::Dark, "ja", false),
                ("protection-small-en", 640., 640., 18., ThemeMode::Light, "en", true),
            ] {
                window.update(cx, |_, w, cx| {
                    theme::apply(mode, cx);
                    gpui_kit::component::theme::Theme::update(cx, |t| t.font_size = px(font));
                    manager.update(cx, |this, cx| { this.settings.language = lang.into(); cx.notify(); });
                    w.resize(size(px(width), px(height)));
                }).unwrap();
                cx.background_executor().timer(std::time::Duration::from_millis(400)).await;
                window.update(cx, |_, w, cx| {
                    w.render_frame(cx);
                    if details { w.click("protection-details", cx); w.render_frame(cx); }
                    w.render_to_image().unwrap().save(output.join(format!("{name}.png"))).unwrap();
                    let original = w.find("original-icon-document-0").bounds();
                    let pack = w.find("pack-icon-document-0").bounds();
                    assert_eq!(original.size.width, pack.size.width, "代替画像とパック画像の列幅を揃えること");
                    // 狭い画面では欄が画面外になるため、通常幅でツールチップを確認する。
                    if width >= 1000. { w.hover("original-icon-document-0", cx); }
                }).unwrap();
                cx.background_executor().timer(std::time::Duration::from_millis(1000)).await;
                window.update(cx, |_, w, cx| {
                    w.render_frame(cx);
                    if width >= 1000. {
                        w.render_to_image().unwrap().save(output.join(format!("{name}-tooltip.png"))).unwrap();
                        w.hover("protection-back", cx);
                        w.render_frame(cx);
                    }
                    w.click("protection-collapse-all", cx);
                    w.render_frame(cx);
                    assert!(w.try_find("original-icon-document-0").is_none());
                    w.render_to_image().unwrap().save(output.join(format!("{name}-collapsed.png"))).unwrap();
                    w.click("protection-expand-all", cx);
                    w.render_frame(cx);
                    for _ in 0..100 {
                        if w.try_find("protection-row-document-135").is_some_and(|row| row.visible() && row.bounds().bottom() <= w.find("protection-rows").bounds().bottom()) { break; }
                        w.scroll("protection-rows", gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-1000.))), cx);
                        w.render_frame(cx);
                    }
                    assert!(w.find("protection-row-document-135").visible());
                    assert!(w.find("protection-row-document-135").bounds().bottom() <= w.find("protection-rows").bounds().bottom());
                    w.render_to_image().unwrap().save(output.join(format!("{name}-bottom.png"))).unwrap();
                    w.press("home", cx);
                    w.render_frame(cx);
                    if details { w.click("protection-details", cx); }
                }).unwrap();
            }
            cx.update(|cx| cx.quit());
        }).detach();
    });
}
