//! パック定義、カテゴリ検索、元登録、下書き、保存を実際の入力から結合する。
use super::*;
use gpui_kit::point;
use std::{fs, process::Command};

pub(super) fn fixture() -> IsolatedRegistryProfile {
    let registry = IsolatedRegistryProfile::new();
    for root in [&registry._classes, &registry._effective] {
        for (extension, class) in [
            ("mts", "Fixture.Video"),
            ("m2ts", "Fixture.Video"),
            ("ts", "Fixture.Code"),
            ("cts", "Fixture.Unprotected"),
            ("wfi-unprotected", "Fixture.Unprotected"),
            ("json", "Fixture.Json"),
            ("rs", "Fixture.Rust"),
            ("lnk", "lnkfile"),
            ("exe", "exefile"),
        ] {
            root.create_subkey(format!(".{extension}"))
                .unwrap()
                .0
                .set_value("", &class)
                .unwrap();
            root.create_subkey(class)
                .unwrap()
                .0
                .set_value("", &class)
                .unwrap();
            if matches!(extension, "mts" | "m2ts" | "ts") {
                root.create_subkey(format!("{class}\\DefaultIcon"))
                    .unwrap()
                    .0
                    .set_value(
                        "",
                        &format!(
                            "{}\\System32\\shell32.dll,2",
                            std::env::var("SystemRoot").unwrap()
                        ),
                    )
                    .unwrap();
            }
        }
        root.create_subkey(".m2ts\\DefaultIcon")
            .unwrap()
            .0
            .set_value(
                "",
                &format!(
                    "{}\\System32\\shell32.dll,3",
                    std::env::var("SystemRoot").unwrap()
                ),
            )
            .unwrap();
        root.create_subkey("Unknown\\DefaultIcon")
            .unwrap()
            .0
            .set_value(
                "",
                &format!(
                    "{}\\System32\\shell32.dll,2",
                    std::env::var("SystemRoot").unwrap()
                ),
            )
            .unwrap();
    }
    let source = home().unwrap().join("target-protection-fixture");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("package.json"), r#"{"name":"target-protection","publisher":"test","version":"1.0.0","contributes":{"iconThemes":[{"id":"test","label":"TypeScript icons","path":"theme.json"},{"id":"other","label":"Other icons","path":"theme.json"}],"languages":[{"id":"fixture-pattern","filenamePatterns":["*.wfi-test","*.wfi-unprotected"]}]}}"#).unwrap();
    // mtsは直接指定しない。言語指定からの補完を、対象画面まで通して検証する。
    fs::write(source.join("theme.json"), r#"{"iconDefinitions":{"code":{"iconPath":"icon.svg"}},"languageIds":{"typescript":"code","fixture-pattern":"code"},"fileExtensions":{"json":"code","rs":"code","lnk":"code","exe":"code"},"fileNames":{"config.mts":"code"}}"#).unwrap();
    fs::write(source.join("icon.svg"), r##"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32"><rect width="32" height="32" rx="4" fill="#3178c6"/><path d="M7 10h18v4h-7v13h-4V14H7z" fill="white"/></svg>"##).unwrap();
    let service = crate::ManagerService::acquire().unwrap();
    service
        .import(&source, "test", &Default::default())
        .unwrap();
    service
        .import(&source, "other", &Default::default())
        .unwrap();
    registry
}

#[test]
fn target_protection_in_isolated_profile() {
    let profile = tempfile::tempdir().unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "ui_tests::target_protection::target_protection_in_child",
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
fn target_protection_in_child(cx: &mut TestAppContext) {
    assert_eq!(std::env::var("WFI_GUI_PROBE").as_deref(), Ok("1"));
    let _registry = fixture();
    let before = load().unwrap();
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
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click(pack_button(&manager, cx, "targets", 0), cx);
        w.render_frame(cx);
        // 調査中に保護画面を開閉しても、戻った対象画面が確認中のままにならない。
        w.click("protection-open", cx);
        w.render_frame(cx);
        w.click("protection-back", cx);
    })
    .unwrap();
    cx.run_until_parked();
    manager.update(cx, |this, _| {
        assert!(
            this.protection.error.is_none(),
            "{:?}",
            this.protection.error
        );
        assert!(
            !this.protection.sources.is_empty(),
            "loading={}",
            this.protection.loading
        );
    });
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        assert!(
            w.find("target-result-extension:mts")
                .label()
                .unwrap()
                .contains("保護中")
        );
        assert_eq!(w.find("target-toggle-extension:mts").checked(), Some(true));
        assert_eq!(
            w.find("target-toggle-extension:mts").bounds().left(),
            w.find("target-toggle-extension:cts").bounds().left(),
            "保護の編集可否によって対象チェックの列をずらさない"
        );
        // TypeScriptはWebに分類される。別カテゴリの検索から全対象へ移っても入力を保つ。
        w.click("target-category-language", cx);
        w.click("target-search", cx);
        w.input(".mts", cx);
        w.render_frame(cx);
        assert!(w.try_find("target-toggle-extension:mts").is_none());
        w.click("target-search-all", cx);
        w.render_frame(cx);
        assert_eq!(w.find("target-search").value(), Some(".mts"));
        assert!(w.find("target-toggle-extension:mts").visible());
        w.click(("target-filter", 5usize), cx);
        w.render_frame(cx);
        assert!(w.find("target-toggle-extension:mts").visible());
        w.click(("target-filter", 0usize), cx);
        w.render_frame(cx);
        w.click("target-protection-extension:mts", cx);
        w.render_frame(cx);
        w.click("pack-icon-mts", cx);
        w.render_frame(cx);
        assert!(
            w.find("default-icon-mts")
                .label()
                .unwrap()
                .contains("結果は元のアイコン")
        );
        // m2tsを保護したまま、mtsだけをパックへ切り替える。保存や再調査は起こさない。
        assert!(
            w.find("target-result-extension:mts")
                .label()
                .unwrap()
                .contains("パック")
        );
        assert!(manager.read(cx).protection.source("m2ts").unwrap().protect);
        assert!(load().unwrap().icon_protection_rules.extensions.is_empty());
        let inspection = manager.read(cx).protection.inspection;
        w.resize(size(px(640.), px(780.)));
        gpui_kit::component::theme::Theme::update(cx, |theme| theme.font_size = px(18.));
        w.render_frame(cx);
        super::protection_sources::reveal_in(w, "pack-icon-mts", "target-rows", cx);
        assert_eq!(w.find("pack-icon-mts").checked(), Some(true));
        w.click("original-icon-mts", cx);
        w.render_frame(cx);
        assert!(
            w.find("target-result-extension:mts")
                .label()
                .unwrap()
                .contains("保護中")
        );
        w.click("pack-icon-mts", cx);
        w.render_frame(cx);
        assert!(
            w.find("target-result-extension:mts")
                .label()
                .unwrap()
                .contains("パック")
        );
        assert_eq!(manager.read(cx).protection.inspection, inspection);
        w.resize(size(px(1120.), px(780.)));
        gpui_kit::component::theme::Theme::update(cx, |theme| theme.font_size = px(16.));
        w.render_frame(cx);
        w.click("target-protection-save", cx);
    })
    .unwrap();
    cx.run_until_parked();
    let saved = load().unwrap();
    assert!(!saved.icon_protection_rules.extensions["mts"].protect);
    assert!(!saved.icon_protection_rules.extensions.contains_key("m2ts"));
    assert!(
        file_icons_windows::protection_report(&saved.packs[0], &saved)
            .unwrap()
            .iter()
            .any(|r| r.extension == "m2ts")
    );
    assert_eq!(saved.selections, before.selections);
    assert!(saved.journal.is_empty() && saved.active.is_none());
    assert!(
        file_icons_windows::protection_report(&saved.packs[0], &saved)
            .unwrap()
            .iter()
            .all(|r| r.extension != "mts")
    );
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("original-icon-mts", cx);
        w.render_frame(cx);
        let result = w.find("target-result-extension:mts");
        let label = result.label().unwrap();
        assert!(
            label.contains("保護中") || label.contains("Original icon protected"),
            "{label}"
        );
        w.click("target-protection-discard", cx);
        w.render_frame(cx);
        assert!(
            w.find("target-result-extension:mts")
                .label()
                .unwrap()
                .contains("パック")
        );
        w.click("default-icon-mts", cx);
        w.render_frame(cx);
        assert!(
            w.find("target-result-extension:mts")
                .label()
                .unwrap()
                .contains("保護中")
        );
        w.click("target-details-open", cx);
        w.render_frame(cx);
        w.click("target-preview", cx);
        w.press("ctrl-a", cx);
        w.input("sample.mts", cx);
        w.render_frame(cx);
        assert!(
            w.find("target-current")
                .label()
                .unwrap()
                .contains("パックのアイコン")
        );
        assert!(
            w.find("target-planned")
                .label()
                .unwrap()
                .contains("Windowsの元の表示")
        );
        w.press("escape", cx);
        w.render_frame(cx);
        w.press("alt-left", cx);
        w.render_frame(cx);
    })
    .unwrap();
    assert_eq!(
        load().unwrap().icon_protection_rules,
        saved.icon_protection_rules
    );
    cx.update_window(window, |_, w, cx| {
        w.click(pack_button(&manager, cx, "targets", 0), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("target-search", cx);
        w.press("ctrl-a", cx);
        w.input("not-in-this-pack", cx);
        w.render_frame(cx);
        assert!(w.find("target-empty").label().unwrap().contains("対応定義"));
        assert!(w.try_find("target-search-all").is_none());
        w.click("target-clear-filters", cx);
        w.render_frame(cx);
        w.click("target-protection-extension:mts", cx);
        w.render_frame(cx);
        w.click("target-toggle-extension:mts", cx);
        w.render_frame(cx);
        assert!(
            w.find("target-result-extension:mts")
                .label()
                .unwrap()
                .contains("対象外")
        );
        w.click("target-toggle-extension:lnk", cx);
        w.click("target-toggle-extension:exe", cx);
        w.render_frame(cx);
        assert_eq!(w.find("target-toggle-extension:lnk").checked(), Some(false));
        assert_eq!(w.find("target-toggle-extension:exe").checked(), Some(false));
        w.click("en", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("target-search", cx);
        w.press("ctrl-a", cx);
        w.input(".mts", cx);
        w.render_frame(cx);
        w.click("target-toggle-extension:mts", cx);
        w.render_frame(cx);
        assert!(
            w.find("target-result-extension:mts")
                .label()
                .unwrap()
                .contains("pack")
        );
        w.click("default-icon-mts", cx);
        w.render_frame(cx);
        assert!(
            w.find("target-result-extension:mts")
                .label()
                .unwrap()
                .contains("protected")
        );
        w.click("protection-open", cx);
        w.render_frame(cx);
        w.click("protection-back", cx);
        w.render_frame(cx);
        assert!(
            w.find("target-result-extension:mts")
                .label()
                .unwrap()
                .contains("Checking protection")
        );
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        assert!(
            w.find("target-result-extension:mts")
                .label()
                .unwrap()
                .contains("pack")
        );
    })
    .unwrap();

    // 拡張子として未登録でも、名前パターンが一致すればUnknownの保護を引き継ぐ。
    cx.update_window(window, |_, w, cx| {
        w.click("target-details-open", cx);
        w.render_frame(cx);
        w.click("target-preview", cx);
        w.press("ctrl-a", cx);
        w.input("sample.wfi-test", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        for id in ["target-current", "target-planned"] {
            assert!(
                w.find(id)
                    .label()
                    .unwrap()
                    .contains("Original icon protected")
            );
        }
        w.click("target-preview", cx);
        w.press("ctrl-a", cx);
        w.input("sample.wfi-unprotected", cx);
    })
    .unwrap();
    cx.run_until_parked();
    // 別パックAが同じ拡張子を直接登録済みでも、BのパターンはBのUnknown保護に従う。
    // 登録計画だけを作り、DLLの読み込みやレジストリへの書き込みは行わない。
    let previous = manager.update(cx, |this, cx| {
        let previous = this.settings.clone();
        let mut other = this.settings.packs[1].clone();
        let mut rules = other.theme.rules().clone();
        rules
            .extensions
            .insert("wfi-test".into(), rules.extensions["json"].clone());
        other.theme.replace_rules(rules).unwrap();
        let executable = std::env::current_exe().unwrap();
        let active_plan = file_icons_windows::plan(&other, &executable).unwrap();
        assert!(active_plan.fallbacks.contains_key(".wfi-test"));
        let next_plan = file_icons_windows::plan(&this.settings.packs[0], &executable).unwrap();
        assert!(!next_plan.fallbacks.contains_key(".wfi-test"));
        assert!(
            next_plan
                .icon_exclusions
                .iter()
                .any(|item| item.class.eq_ignore_ascii_case("Unknown"))
        );
        this.settings.active = Some(other.id.clone());
        this.settings.packs[1] = other;
        this.settings.fallbacks = active_plan.fallbacks;
        cx.notify();
        previous
    });
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        // 既存の別クラスに属する種類へ、Unknownの保護を誤って広げない。
        for id in ["target-current", "target-planned"] {
            assert!(w.find(id).label().unwrap().contains("Pack icon"));
        }
        w.click("target-preview", cx);
        w.press("ctrl-a", cx);
        w.input("sample.wfi-test", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        for id in ["target-current", "target-planned"] {
            assert!(
                w.find(id)
                    .label()
                    .unwrap()
                    .contains("Original icon protected")
            );
        }
        w.click("target-preview", cx);
        w.press("ctrl-a", cx);
        w.input("sample.wfi-unprotected", cx);
        w.press("ctrl-a", cx);
        w.input("config.mts", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        for id in ["target-current", "target-planned"] {
            assert!(w.find(id).label().unwrap().contains("Pack icon"));
        }
        w.click("target-auxiliary-back", cx);
    })
    .unwrap();
    manager.update(cx, |this, cx| {
        this.settings = previous;
        cx.notify();
    });

    // 製品の最小サイズ・拡大文字でも、スクロール後に方針を選び、固定フッターから保存できる。
    for (language, mode, control, protected) in [
        ("en", ThemeMode::Light, "original-icon-mts", true),
        ("ja", ThemeMode::Dark, "pack-icon-mts", false),
    ] {
        cx.update_window(window, |_, w, cx| {
            w.resize(size(px(640.), px(640.)));
            theme::apply(mode, cx);
            gpui_kit::component::theme::Theme::update(cx, |theme| theme.font_size = px(18.));
            manager.update(cx, |this, cx| {
                this.settings.language = language.into();
                cx.notify();
            });
            w.render_frame(cx);
            let viewport = w.find("target-rows").bounds();
            let toggle = w.find(control).bounds();
            w.scroll(
                "target-rows",
                gpui_kit::ScrollDelta::Pixels(point(
                    px(0.),
                    viewport.center().y - toggle.center().y,
                )),
                cx,
            );
            w.render_frame(cx);
            let toggle = w.find(control).bounds();
            assert!(toggle.center().y >= viewport.top() && toggle.center().y <= viewport.bottom());
            w.click(control, cx);
            w.render_frame(cx);
            let save = w.find("target-protection-save");
            assert!(save.visible() && save.bounds().bottom() <= w.viewport_size().height);
            w.click("target-protection-save", cx);
        })
        .unwrap();
        cx.run_until_parked();
        let persisted = load().unwrap();
        assert_eq!(
            persisted.icon_protection_rules.extensions["mts"].protect,
            protected
        );
        assert_eq!(persisted.selections, before.selections);
        assert!(persisted.active.is_none() && persisted.journal.is_empty());
    }
    // 見本の到着を待たずに方針を編集でき、前の調査の見本は新しい調査へ混入しない。
    let (inspection, previous_previews) = manager.update(cx, |this, cx| {
        this.protection.previews_loading = true;
        let previews = this.protection.previews.take();
        let inspection = this.protection.inspection;
        this.accept_protection_previews(inspection.wrapping_sub(1), None, cx);
        assert!(this.protection.previews_loading);
        (inspection, previews)
    });
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("original-icon-mts", cx);
        w.render_frame(cx);
        let result = w.find("target-result-extension:mts");
        let label = result.label().unwrap();
        assert!(
            label.contains("保護中") || label.contains("Original icon protected"),
            "{label}"
        );
    })
    .unwrap();
    manager.update(cx, |this, cx| {
        this.protection.inspection += 1;
        this.accept_protection_previews(inspection, previous_previews, cx);
        assert!(this.protection.previews.is_none());
        assert!(this.protection.previews_loading);
        this.accept_protection_previews(this.protection.inspection, None, cx);
        assert!(!this.protection.previews_loading);
        assert!(this.protection.error.is_none());
    });
}
