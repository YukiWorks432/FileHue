use super::*;
use crate::test_process::CommandOutput;
#[test]
fn target_editor_in_isolated_profile() {
    let profile = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "ui_tests::targets::target_editor_in_child",
            "--nocapture",
        ])
        .env("LOCALAPPDATA", profile.path())
        .env("WFI_GUI_PROBE", "1")
        // Windowsの通常起動と同じ1MiBで、描画と操作のスタック使用量を検証する。
        .env("RUST_MIN_STACK", "1048576")
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
fn target_editor_in_child(cx: &mut TestAppContext) {
    assert_eq!(std::env::var("WFI_GUI_PROBE").as_deref(), Ok("1"));
    let registry = IsolatedRegistryProfile::new();
    // 対象選択の試験は、開発PCの既存アイコンや既定アプリから独立させる。
    for root in [&registry._classes, &registry._effective] {
        let extensions = ["json".to_owned(), "md".into(), "lnk".into(), "exe".into()]
            .into_iter()
            .chain((0..101).map(|number| format!("zprobe{number:03}")));
        for extension in extensions {
            let class = match extension.as_str() {
                "lnk" => "lnkfile".to_owned(),
                "exe" => "exefile".to_owned(),
                _ => format!("Fixture.Target.{extension}"),
            };
            root.create_subkey(format!(".{extension}"))
                .unwrap()
                .0
                .set_value("", &class)
                .unwrap();
            root.create_subkey(&class)
                .unwrap()
                .0
                .set_value("", &class)
                .unwrap();
            for key in [format!(".{extension}"), class] {
                for suffix in ["DefaultIcon", "shellex\\IconHandler"] {
                    root.create_subkey(format!("{key}\\{suffix}"))
                        .unwrap()
                        .0
                        .set_value("", &"")
                        .unwrap();
                }
            }
        }
    }
    let root = home().unwrap();
    std::fs::create_dir_all(&root).unwrap();
    let icon = root.join("fixture.ico");
    std::fs::write(&icon, b"fixture").unwrap();
    let pack = file_icons_core::ImportedPack {
        id: "test/targets".into(),
        version: "1".into(),
        label: "Targets".into(),
        provenance: root.join("provenance.json"),
        warnings: Vec::new(),
        theme: file_icons_core::CompiledTheme::new(file_icons_core::ThemeRules {
            files: std::collections::BTreeMap::from([
                ("package.json".into(), icon.clone()),
                ("README.md".into(), icon.clone()),
            ]),
            extensions: std::collections::BTreeMap::from([
                ("json".into(), icon.clone()),
                ("md".into(), icon),
            ]),
            ..Default::default()
        })
        .unwrap(),
    };
    let mut pack = pack;
    let mut rules = pack.theme.rules().clone();
    let long_name = format!("{}.json", "very_long_configuration_".repeat(8));
    rules
        .files
        .insert(long_name.clone(), root.join("fixture.ico"));
    for extension in ["lnk", "exe"] {
        rules
            .extensions
            .insert(extension.into(), root.join("fixture.ico"));
    }
    for number in 0..101 {
        rules
            .extensions
            .insert(format!("zprobe{number:03}"), root.join("fixture.ico"));
    }
    pack.theme = file_icons_core::CompiledTheme::new(rules).unwrap();
    let mut other = pack.clone();
    other.id = "test/other".into();
    other.label = "Other".into();
    file_icons_windows::save(&file_icons_windows::Settings {
        packs: vec![pack, other],
        language: "ja".into(),
        ..Default::default()
    })
    .unwrap();
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
        // 初回・幅変更直後も、検索や絞り込みで再初期化せずページ末尾へ到達する。
        let last = manager.read(cx).settings.packs[0].theme.targets()[99]
            .id
            .clone();
        for (width, font) in [(1120., 16.), (640., 18.)] {
            w.resize(size(px(width), px(780.)));
            gpui_kit::component::theme::Theme::update(cx, |theme| theme.font_size = px(font));
            w.render_frame(cx);
            w.scroll(
                "target-rows",
                gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-100000.))),
                cx,
            );
            w.render_frame(cx);
            let id = format!("target-toggle-{last}");
            assert!(w.find(id.clone()).visible());
            w.click(id.clone(), cx);
            w.render_frame(cx);
            assert_eq!(w.find(id.clone()).checked(), Some(false));
            w.click("target-undo", cx);
            w.render_frame(cx);
            assert_eq!(w.find(id).checked(), Some(true));
            w.scroll(
                "target-rows",
                gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(100000.))),
                cx,
            );
            w.render_frame(cx);
        }
        w.resize(size(px(1120.), px(780.)));
        gpui_kit::component::theme::Theme::update(cx, |theme| theme.font_size = px(16.));
        w.render_frame(cx);
        // 移動だけでは絞り込みを変えず、Enterで確定する。カテゴリ操作では保存しない。
        w.click("target-category-data", cx);
        w.render_frame(cx);
        assert!(w.find("target-toggle-extension:json").visible());
        assert!(w.try_find("target-toggle-extension:md").is_none());
        w.press("down", cx);
        w.render_frame(cx);
        assert!(w.find("target-toggle-extension:json").visible());
        assert!(w.try_find("target-toggle-extension:md").is_none());
        w.press("enter", cx);
        w.render_frame(cx);
        assert!(w.find("target-toggle-extension:md").visible());
        assert!(w.try_find("target-toggle-extension:json").is_none());
        w.click("target-all-categories", cx);
        w.render_frame(cx);
        assert!(w.find("target-toggle-extension:json").visible());
        assert!(w.find("target-toggle-extension:md").visible());
        assert!(load().unwrap().selections.is_empty());
        // 日英カテゴリ名と大文字・ドット付き検索を、入力イベントと空結果からの復帰まで確認する。
        for (query, json, md) in [
            (".JSON", true, false),
            ("DATA AND CONFIGURATION", true, false),
            ("文書", false, true),
            ("not-a-kind", false, false),
        ] {
            w.click("target-search", cx);
            w.press("ctrl-a", cx);
            w.input(query, cx);
            w.render_frame(cx);
            assert_eq!(
                w.try_find("target-toggle-extension:json").is_some(),
                json,
                "{query}"
            );
            assert_eq!(
                w.try_find("target-toggle-extension:md").is_some(),
                md,
                "{query}"
            );
        }
        w.click("target-search", cx);
        w.press("ctrl-a", cx);
        w.press("backspace", cx);
        w.render_frame(cx);
        w.click("target-search", cx);
        w.input("lnk", cx);
        w.render_frame(cx);
        w.click("target-toggle-extension:lnk", cx);
        w.render_frame(cx);
        w.click("target-enable-results", cx);
        w.render_frame(cx);
        w.click("target-save", cx);
        w.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    let protected = load().unwrap();
    let selection = protected
        .selections
        .get("test/targets")
        .cloned()
        .unwrap_or_default();
    assert!(!selection.enabled("shortcut.LNK"));
    assert!(!selection.enabled("program.EXE"));
    assert_eq!(selection.explicit("extension:lnk"), None);
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("target-search", cx);
        w.press("ctrl-a", cx);
        w.press("backspace", cx);
        w.render_frame(cx);
        w.click("target-category-data", cx);
        w.render_frame(cx);
        w.click("target-disable-results", cx);
        w.render_frame(cx);
    })
    .unwrap();
    assert!(
        load()
            .unwrap()
            .selections
            .values()
            .all(|s| s.explicit("extension:lnk").is_none())
    );
    cx.update_window(window, |_, w, cx| {
        w.click("target-undo", cx);
        w.render_frame(cx);
        w.click("target-all-categories", cx);
        w.render_frame(cx);
        w.click("target-search", cx);
        w.input(".json", cx);
        w.render_frame(cx);
        w.click("target-disable-results", cx);
        w.render_frame(cx);
        w.click("target-toggle-file:package.json", cx);
        w.render_frame(cx);
        // 長い名前や個別指定の有無が、適用チェックの列をずらさない。
        let extension = w.find("target-toggle-extension:json").bounds();
        let filename = w.find("target-toggle-file:package.json").bounds();
        let long = w.find(format!("target-toggle-file:{long_name}")).bounds();
        assert_eq!(extension.left(), filename.left());
        assert_eq!(extension.left(), long.left());
        w.click("conflicts-open", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("recovery-close", cx);
        w.render_frame(cx);
        assert_eq!(
            w.find("target-toggle-extension:json").checked(),
            Some(false)
        );
        assert_eq!(
            w.find("target-toggle-file:package.json").checked(),
            Some(true)
        );
        w.click("target-details-open", cx);
        w.render_frame(cx);
        w.click("target-preview", cx);
        w.input("package.json", cx);
    })
    .unwrap();
    // 画像指定のないクラスも、実効関連付けの確認後に結果を評価する。
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        assert!(
            w.find("target-planned")
                .label()
                .unwrap()
                .contains("パックのアイコン")
        );
        w.click("target-auxiliary-back", cx);
        w.render_frame(cx);
        w.click("target-save", cx);
        w.render_frame(cx);
        assert!(w.find("target-save-progress").visible());
        assert_eq!(w.find("target-save-progress").label(), Some("保存中…"));
        let progress = w.find("target-save-progress").bounds();
        let save = w.find("target-save").bounds();
        assert!(progress.right() <= save.left());
        assert!(progress.top() < save.bottom() && save.top() < progress.bottom());
    })
    .unwrap();
    cx.run_until_parked();
    let settings = load().unwrap();
    let selection = &settings.selections["test/targets"];
    assert!(!selection.enabled("data.json"));
    assert!(selection.enabled("package.json"));
    assert!(selection.enabled("notes.md"));
    assert!(settings.active.is_none() && settings.journal.is_empty());
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        assert!(w.try_find("target-save-progress").is_none());
        w.click("en", cx);
        w.render_frame(cx);
        assert!(w.try_find("target-save-progress").is_none());
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        assert_eq!(w.find("target-save").label(), Some("Save selection"));
        assert!(w.try_find("target-save-progress").is_none());
        w.click(("target-filter", 2usize), cx);
        w.render_frame(cx);
        assert!(w.try_find("target-toggle-file:package.json").is_none());
        assert!(w.find("target-toggle-extension:json").visible());
        w.click(("target-filter", 0usize), cx);
        w.render_frame(cx);
        w.click("target-toggle-file:package.json", cx);
        w.render_frame(cx);
        w.click("target-undo", cx);
        w.render_frame(cx);
        w.click("target-discard", cx);
    })
    .unwrap();
    // 保存先の排他で失敗させ、下書きを保持して再試行できることを保証する。
    use std::os::windows::fs::OpenOptionsExt;
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(root.join("settings.json"))
        .unwrap();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("target-toggle-file:package.json", cx);
        w.render_frame(cx);
        w.click("target-save", cx);
        w.render_frame(cx);
        assert_eq!(w.find("target-save-progress").label(), Some("Saving…"));
        let checked = w.find("target-toggle-file:package.json").checked();
        w.click("target-toggle-file:package.json", cx);
        w.render_frame(cx);
        assert_eq!(w.find("target-toggle-file:package.json").checked(), checked);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(load().unwrap().selections["test/targets"].enabled("package.json"));
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        assert!(w.try_find("target-save-progress").is_none());
        assert!(w.find("target-status").label().unwrap().contains("failed"));
    })
    .unwrap();
    drop(lock);
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("target-save", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(!load().unwrap().selections["test/targets"].enabled("package.json"));
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("target-back", cx);
        w.render_frame(cx);
        let heights: Vec<_> = ["library", "browse", "import"]
            .iter()
            .map(|id| w.find(*id).bounds().size.height)
            .collect();
        assert_eq!(heights[0], heights[1]);
        assert_eq!(heights[1], heights[2]);
        w.click("browse", cx);
        w.render_frame(cx);
        assert!(manager.read(cx).browsing);
        w.click("browse", cx);
        w.render_frame(cx);
        assert!(manager.read(cx).browsing);
        w.click("library", cx);
        w.render_frame(cx);
        assert!(!manager.read(cx).browsing);
        w.click("library", cx);
        w.render_frame(cx);
        assert!(!manager.read(cx).browsing);
        w.click("browse", cx);
        w.render_frame(cx);
        w.press("back", cx);
        w.render_frame(cx);
        assert!(!manager.read(cx).browsing);
        w.click(pack_button(&manager, cx, "targets", 1), cx);
        w.render_frame(cx);
        assert!(w.find("target-toggle-extension:json").visible());
        w.click("target-selection-mode", cx);
        w.render_frame(cx);
        w.click("target-mark-extension:json", cx);
        w.render_frame(cx);
        w.click("target-search", cx);
        w.input(".md", cx);
        w.render_frame(cx);
        w.click("target-disable-marked", cx);
        w.render_frame(cx);
        w.click("target-selection-mode", cx);
        w.render_frame(cx);
        use gpui_kit::InputEvent;
        w.dispatch_event(
            gpui_kit::MouseDownEvent {
                position: gpui_kit::point(px(30.), px(30.)),
                button: gpui_kit::MouseButton::Navigate(gpui_kit::NavigationDirection::Back),
                modifiers: Default::default(),
                click_count: 1,
                first_mouse: false,
            }
            .to_platform_input(),
            cx,
        );
        w.render_frame(cx);
        assert!(w.try_find("target-search").is_none());
        w.click(pack_button(&manager, cx, "targets", 1), cx);
        w.render_frame(cx);
        w.click("target-search", cx);
        w.press("ctrl-a", cx);
        w.press("backspace", cx);
        w.render_frame(cx);
        w.click("target-save", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(!load().unwrap().selections["test/other"].enabled("data.json"));
    assert!(load().unwrap().selections["test/other"].enabled("notes.md"));
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("target-category-documents", cx);
        w.render_frame(cx);
        w.click("target-category-toggle-documents", cx);
        w.render_frame(cx);
        w.click("target-save", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(!load().unwrap().selections["test/other"].enabled("notes.md"));
    let saved_before_copy = std::fs::read(home().unwrap().join("settings.library.json")).unwrap();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("target-all-categories", cx);
        w.render_frame(cx);
        w.click("target-copy-open", cx);
        w.render_frame(cx);
        w.render_frame(cx);
        assert_eq!(
            std::fs::read(home().unwrap().join("settings.library.json")).unwrap(),
            saved_before_copy,
            "コピー候補の表示では保存しない"
        );
        w.press("alt-left", cx);
        w.render_frame(cx);
        assert!(w.find("target-search").visible());
        assert!(w.try_find("target-copy-test/targets").is_none());
        w.click("target-copy-open", cx);
        w.render_frame(cx);
        w.click("target-copy-test/targets", cx);
        w.render_frame(cx);
        assert_eq!(w.find("target-toggle-extension:md").checked(), Some(true));
        assert_eq!(
            std::fs::read(home().unwrap().join("settings.library.json")).unwrap(),
            saved_before_copy,
            "コピーの確定も保存までは下書きにとどめる"
        );
        w.click("target-undo", cx);
        w.render_frame(cx);
        assert_eq!(w.find("target-toggle-extension:md").checked(), Some(false));
        w.click("target-copy-open", cx);
        w.render_frame(cx);
        w.click("target-copy-test/targets", cx);
        w.render_frame(cx);
        w.click("target-save", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(load().unwrap().selections["test/other"].enabled("notes.md"));
    // 一覧をスクロールしても、検索と保存操作は画面内の同じ位置に残る。
    cx.update_window(window, |_, w, cx| {
        w.click("target-search", cx);
        w.press("ctrl-a", cx);
        w.press("backspace", cx);
        w.render_frame(cx);
        let search = w.find("target-search").bounds();
        let save = w.find("target-save").bounds();
        w.scroll(
            "target-rows",
            gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-900.))),
            cx,
        );
        w.render_frame(cx);
        let offset = manager.read(cx).targets.scroll_offset();
        assert!(offset.y < px(0.), "一覧がホイールで移動する");
        let area = w.find("target-rows").bounds();
        // 一覧の途中へ移動した後も、表示領域右上のバーを押して上へ戻れる。
        w.click_at(
            "target-rows",
            gpui_kit::point(area.size.width - px(5.), px(8.)),
            cx,
        );
        w.render_frame(cx);
        assert!(
            manager.read(cx).targets.scroll_offset().y > offset.y,
            "移動後もバーが表示領域に残る"
        );
        assert_eq!(w.find("target-search").bounds(), search);
        assert_eq!(w.find("target-save").bounds(), save);
        w.click("target-next", cx);
        w.render_frame(cx);
        assert!(w.try_find("target-toggle-extension:json").is_none());
        w.click("target-previous", cx);
        w.render_frame(cx);
        assert!(w.try_find("target-toggle-extension:json").is_some());
        w.click("target-search", cx);
        w.input("no-such-target", cx);
        w.render_frame(cx);
        assert!(w.find("target-empty").visible());
        w.click("target-clear-filters", cx);
        w.render_frame(cx);
        assert!(w.find("target-toggle-extension:json").visible());
    })
    .unwrap();
    cx.simulate_window_resize(window, size(px(640.), px(700.)));
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        cx.update_window(window, |_, w, cx| {
            theme::apply(mode, cx);
            gpui_kit::component::theme::Theme::update(cx, |theme| theme.font_size = px(18.));
            w.render_frame(cx);
            for control in [
                "target-search",
                "target-save",
                "target-categories-open",
                "target-details-open",
            ] {
                let bounds = w.find(control).bounds();
                assert!(w.find(control).visible(), "{control}");
                assert!(
                    bounds.left() >= px(0.) && bounds.right() <= px(640.),
                    "{control}"
                );
                assert!(bounds.bottom() <= px(700.), "{control}");
            }
            w.click("target-search", cx);
            w.press("ctrl-a", cx);
            w.input("very_long_configuration", cx);
            w.render_frame(cx);
            let long_control = w.find(format!("target-toggle-file:{long_name}"));
            assert!(long_control.visible());
            assert!(long_control.bounds().right() <= px(640.));
            w.click("target-search", cx);
            w.press("ctrl-a", cx);
            w.press("backspace", cx);
            w.render_frame(cx);
            let rows = w.find("target-rows").bounds();
            assert!(rows.size.height > px(0.));
            assert!(w.find("target-search").bounds().bottom() <= rows.top());
            assert!(rows.bottom() <= w.find("target-save").bounds().top());
            w.click("target-categories-open", cx);
            w.render_frame(cx);
            w.press("down", cx);
            w.render_frame(cx);
            assert!(w.find("target-category-data").visible());
            w.press("escape", cx);
            w.render_frame(cx);
            assert!(w.find("target-search").visible());
            w.click("target-categories-open", cx);
            w.render_frame(cx);
            w.click("target-category-data", cx);
            w.render_frame(cx);
            assert!(w.find("target-toggle-extension:json").visible());
            assert!(w.try_find("target-toggle-extension:md").is_none());
            assert_eq!(w.find("target-search").focused(), Some(true));
            w.click("target-details-open", cx);
            w.render_frame(cx);
            w.click("target-preview", cx);
            w.press("ctrl-a", cx);
            w.input("package.json", cx);
            w.render_frame(cx);
            assert!(
                w.find("target-planned")
                    .label()
                    .unwrap()
                    .contains("Original Windows icon")
            );
            w.press("escape", cx);
            w.render_frame(cx);
            assert!(w.find("target-search").visible());
        })
        .unwrap();
    }
    // チェックボックスのキーボード操作も下書きへ反映し、保存までは設定を変えない。
    let before = load().unwrap().selections["test/other"].clone();
    cx.update_window(window, |_, w, cx| {
        assert_eq!(
            w.find("target-toggle-extension:json").checked(),
            Some(false)
        );
        w.click("target-toggle-extension:json", cx);
        w.render_frame(cx);
        assert_eq!(w.find("target-toggle-extension:json").checked(), Some(true));
        assert_eq!(load().unwrap().selections["test/other"], before);
        for _ in 0..40 {
            if w.find("target-toggle-extension:json").focused() == Some(true) {
                break;
            }
            w.press("tab", cx);
            w.render_frame(cx);
        }
        assert_eq!(w.find("target-toggle-extension:json").focused(), Some(true));
        w.press("space", cx);
        w.render_frame(cx);
        assert_eq!(
            w.find("target-toggle-extension:json").checked(),
            Some(false)
        );
        w.click("target-save", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(load().unwrap().selections["test/other"], before);
    // 最後のページから対象が消えても、残ったページを表示する。
    // 一括操作と取り消しは、画面外の対象も含む検索結果全件へ作用する。
    cx.update_window(window, |_, w, cx| {
        w.resize(size(px(640.), px(700.)));
        gpui_kit::component::theme::Theme::update(cx, |theme| theme.font_size = px(18.));
        w.render_frame(cx);
        if w.try_find("target-all-categories").is_none() {
            w.click("target-categories-open", cx);
        }
        w.click("target-all-categories", cx);
        w.click("target-search", cx);
        w.press("ctrl-a", cx);
        w.input("zprobe", cx);
        w.click(("target-filter", 1usize), cx);
        w.scroll(
            "target-rows",
            gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-100000.))),
            cx,
        );
        w.render_frame(cx);
        assert!(w.find("target-toggle-extension:zprobe099").visible());
        w.click("target-toggle-extension:zprobe099", cx);
        w.click("target-undo", cx);
        w.click("target-next", cx);
        w.render_frame(cx);
        w.click("target-toggle-extension:zprobe100", cx);
        w.render_frame(cx);
        assert!(w.find("target-toggle-extension:zprobe000").visible());
        w.click("target-disable-results", cx);
        w.render_frame(cx);
        assert!(w.find("target-empty").visible());
        w.click("target-undo", cx);
        w.click("target-search", cx);
        w.press("ctrl-a", cx);
        w.input("zprobe099", cx);
        w.render_frame(cx);
        assert_eq!(
            w.find("target-toggle-extension:zprobe099").checked(),
            Some(true)
        );
    })
    .unwrap();
    assert_eq!(load().unwrap().selections["test/other"], before);
    drop(manager);
}
