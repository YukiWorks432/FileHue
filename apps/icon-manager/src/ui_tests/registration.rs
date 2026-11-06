//! GUIの操作から、本番の登録・切り替え・復元を子プロセス専用キーで検証する。
use super::*;
use crate::test_process::CommandOutput;
use std::{collections::BTreeMap, fs};
use winreg::{RegKey, RegValue, enums::*};

type Snapshot = BTreeMap<String, BTreeMap<String, (u32, Vec<u8>)>>;
fn snapshot(key: &RegKey) -> Snapshot {
    fn visit(key: &RegKey, prefix: &str, result: &mut Snapshot) {
        let values = key
            .enum_values()
            .map(|item| {
                let (name, value) = item.unwrap();
                (name, (value.vtype as u32, value.bytes.into_owned()))
            })
            .collect();
        result.insert(prefix.to_owned(), values);
        for name in key.enum_keys() {
            let name = name.unwrap();
            visit(
                &key.open_subkey(&name).unwrap(),
                &format!("{prefix}\\{name}"),
                result,
            );
        }
    }
    let mut result = Snapshot::new();
    visit(key, "", &mut result);
    result
}

#[test]
fn registration_gui_in_isolated_profile() {
    let profile = tempfile::tempdir().unwrap();
    // DLLの読取失敗を他の並行試験へ波及させないよう、実行元も専用領域へ分ける。
    let executable = std::env::current_exe().unwrap();
    let copied = profile.path().join("registration-tests.exe");
    fs::copy(&executable, &copied).unwrap();
    fs::copy(
        executable.parent().unwrap().join("file_icons_shell.dll"),
        profile.path().join("file_icons_shell.dll"),
    )
    .unwrap();
    let output = std::process::Command::new(copied)
        .args([
            "--ignored",
            "--exact",
            "ui_tests::registration::registration_gui_in_child",
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
fn registration_gui_in_child(cx: &mut TestAppContext) {
    assert_eq!(std::env::var("WFI_GUI_PROBE").as_deref(), Ok("1"));
    let registry = IsolatedRegistryProfile::new();
    // 利用者側と有効クラス側のどちらにも、既存の既定アプリと空キーを置く。
    for root in [&registry._classes, &registry._effective] {
        for suffix in ["a", "b"] {
            let class = format!("WfiGui.{suffix}");
            root.create_subkey(format!(".wfi-gui-{suffix}"))
                .unwrap()
                .0
                .set_value("", &class)
                .unwrap();
            root.create_subkey(format!(r"{class}\shell\open\command"))
                .unwrap()
                .0
                .set_value("", &"notepad.exe \"%1\"")
                .unwrap();
            root.create_subkey(format!(r"{class}\shellex")).unwrap();
        }
        root.create_subkey(r"Unknown\shellex").unwrap();
        root.create_subkey(r"WfiGui.a\DefaultIcon")
            .unwrap()
            .0
            .set_value("", &"shell32.dll,2")
            .unwrap();
        root.create_subkey(r"Unknown\DefaultIcon")
            .unwrap()
            .0
            .set_value("", &"shell32.dll,0")
            .unwrap();
    }
    let baseline = snapshot(&registry._classes);
    let effective_baseline = snapshot(&registry._effective);
    let fixture = home().unwrap().join("source-extension");
    fs::create_dir_all(&fixture).unwrap();
    fs::write(fixture.join("package.json"), r#"{"name":"gui-registration","publisher":"test","version":"1.0.0","contributes":{"iconThemes":[{"id":"first","label":"First pack","path":"a.json"},{"id":"second","label":"Second pack","path":"b.json"}]}}"#).unwrap();
    for (suffix, color) in [("a", "#e41200"), ("b", "#0044e1")] {
        let extension = format!("wfi-gui-{suffix}");
        let theme = serde_json::json!({"iconDefinitions":{"icon":{"iconPath":format!("{suffix}.svg")}},"fileExtensions":{(extension):"icon"},"fileNames":{"Dockerfile":"icon"}});
        fs::write(
            fixture.join(format!("{suffix}.json")),
            serde_json::to_vec(&theme).unwrap(),
        )
        .unwrap();
        fs::write(fixture.join(format!("{suffix}.svg")), format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="16" height="16" fill="{color}"/></svg>"#)).unwrap();
    }
    let sources = fs::read_dir(&fixture)
        .unwrap()
        .map(|e| {
            let p = e.unwrap().path();
            (p.clone(), fs::read(p).unwrap())
        })
        .collect::<Vec<_>>();
    let (window, manager) = cx.update(|cx| {
        gpui_kit::init(cx);
        theme::apply(ThemeMode::Dark, cx);
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1000.), px(1200.)),
                    cx,
                ))),
                ..Default::default()
            },
            cx,
            Manager::open,
        )
        .unwrap()
    });
    for index in 0..2usize {
        cx.update_window(window, |_, w, cx| {
            w.render_frame(cx);
            w.click("import", cx);
        })
        .unwrap();
        cx.simulate_path_prompt_response(|options| {
            assert!(options.directories && !options.files && !options.multiple);
            Some(vec![fixture.clone()])
        });
        cx.run_until_parked();
        cx.update_window(window, |_, w, cx| {
            w.render_frame(cx);
            w.click(("theme", index), cx);
        })
        .unwrap();
        cx.run_until_parked();
    }
    let imported = load().unwrap();
    assert_eq!(imported.packs.len(), 2);
    assert!(imported.active.is_none() && imported.journal.is_empty());
    // 狭い画面でもホイールだけで全件の末尾へ到達し、詳細の全値を取得できる。
    cx.simulate_window_resize(window, size(px(640.), px(640.)));
    cx.update_window(window, |_, w, cx| {
        gpui_kit::component::theme::Theme::update(cx, |t| t.font_size = px(18.));
        w.render_frame(cx);
        w.click("protection-open", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        manager.update(cx, |this, cx| {
            this.protection.visual_rows(
                (0..160)
                    .map(|ix| file_icons_windows::IconExclusion {
                        extension: format!("scroll-{ix:03}"),
                        class: format!("Scrolling.Type{ix}"),
                        source: format!("Scrolling.Type{ix}\\DefaultIcon"),
                        reason: file_icons_windows::ProtectionReason::DefaultIcon,
                    })
                    .collect(),
            );
            cx.notify();
        });
        w.render_frame(cx);
        let save = w.find("protection-save").bounds();
        for _ in 0..30 {
            w.scroll(
                "protection-rows",
                gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-1000.))),
                cx,
            );
            w.render_frame(cx);
        }
        assert!(
            w.try_find("protection-row-scroll-159")
                .is_some_and(|row| row.visible()),
            "ホイールで160件目まで到達できること"
        );
        assert_eq!(w.find("protection-save").bounds(), save);
        w.click("protection-details", cx);
        w.render_frame(cx);
        for _ in 0..40 {
            w.scroll(
                "protection-rows",
                gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-1000.))),
                cx,
            );
            w.render_frame(cx);
        }
        w.click("protection-copy-scroll-159", cx);
        let copied = cx.read_from_clipboard().unwrap().text().unwrap();
        assert!(copied.contains("Scrolling.Type159\\DefaultIcon"));
        w.press("home", cx);
        w.render_frame(cx);
        assert!(w.find("protection-row-scroll-000").visible());
        // スクロールバーには観測用IDがないため、一覧右端の軌道をクリックする。
        let rows = w.find("protection-rows").bounds();
        for _ in 0..2 {
            w.click_at(
                "protection-rows",
                gpui_kit::point(rows.size.width - px(8.), rows.size.height - px(8.)),
                cx,
            );
            w.render_frame(cx);
        }
        assert!(
            w.try_find("protection-row-scroll-159")
                .is_some_and(|row| row.visible())
        );
        assert_eq!(w.find("protection-save").bounds(), save);
        w.click("navigate-back", cx);
    })
    .unwrap();
    assert!(load().unwrap().preserve_existing_icons);
    assert_eq!(snapshot(&registry._classes), baseline);
    // 狭い画面・拡大文字でも保存と種類の確認へ到達し、詳細をコピーできる。
    cx.simulate_window_resize(window, size(px(640.), px(640.)));
    for (mode, language) in [(ThemeMode::Dark, "ja"), (ThemeMode::Light, "en")] {
        cx.update_window(window, |_, w, cx| {
            theme::apply(mode, cx);
            gpui_kit::component::theme::Theme::update(cx, |t| t.font_size = px(18.));
            manager.update(cx, |v, _| v.settings.language = language.into());
            w.render_frame(cx);
            w.click("protection-open", cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(window, |_, w, cx| {
            w.render_frame(cx);
            for id in [
                "protection-toggle",
                "protection-view-select",
                "protection-pack",
                "protection-details",
                "protection-save",
                "protection-back",
            ] {
                let b = w.find(id).bounds();
                assert!(b.left() >= px(0.) && b.right() <= px(640.), "{id}: {b:?}");
                assert!(b.top() >= px(0.) && b.bottom() <= px(640.), "{id}: {b:?}");
            }
            w.hover("protection-rows", cx);
            w.render_frame(cx);
            let rows = w.find("protection-rows").bounds();
            assert!(rows.size.height > px(48.));
            assert!(rows.bottom() <= w.find("protection-save").bounds().top());
            w.click("protection-details", cx);
            w.render_frame(cx);
            w.press("end", cx);
            w.render_frame(cx);
            w.click("protection-copy-wfi-gui-a", cx);
            let copied = cx.read_from_clipboard().unwrap().text().unwrap();
            assert!(
                copied.to_ascii_lowercase().contains("wfigui.a") && copied.contains("DefaultIcon"),
                "{copied}"
            );
            // Tabでcheckboxへ移動してSpaceで下書きを変え、Escで破棄する。
        })
        .unwrap();
        let mut reached = false;
        for _ in 0..24 {
            reached = cx
                .update_window(window, |_, w, cx| {
                    w.render_frame(cx);
                    if w.find("protection-toggle").focused() == Some(true) {
                        true
                    } else {
                        w.press("tab", cx);
                        false
                    }
                })
                .unwrap();
            if reached {
                break;
            }
        }
        assert!(reached, "保護設定にTabで到達できない");
        cx.update_window(window, |_, w, cx| w.press("space", cx))
            .unwrap();
        cx.run_until_parked();
        cx.update_window(window, |_, w, cx| {
            w.render_frame(cx);
            assert_eq!(w.find("protection-toggle").checked(), Some(false));
            w.press("escape", cx);
            w.render_frame(cx);
            assert!(w.try_find("protection-toggle").is_none());
        })
        .unwrap();
        assert!(load().unwrap().preserve_existing_icons);
        assert_eq!(snapshot(&registry._classes), baseline);
    }
    cx.simulate_window_resize(window, size(px(1000.), px(760.)));
    cx.update_window(window, |_, _, cx| {
        theme::apply(ThemeMode::Dark, cx);
        gpui_kit::component::theme::Theme::update(cx, |t| t.font_size = px(14.));
        manager.update(cx, |v, _| v.settings.language = "ja".into());
    })
    .unwrap();
    // 標準の選択メニューをキーボードで操作しても、確認先だけが変わる。
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("protection-open", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("protection-pack", cx);
        w.render_frame(cx);
        w.press("escape", cx);
        w.render_frame(cx);
        assert!(w.try_find("protection-toggle").is_some());
        w.click("protection-pack", cx);
        w.render_frame(cx);
        w.press("down", cx);
        w.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        assert!(w.try_find("protection-row-wfi-gui-a").is_none());
        w.click("protection-back", cx);
    })
    .unwrap();
    assert!(load().unwrap().preserve_existing_icons);
    assert_eq!(snapshot(&registry._classes), baseline);
    // 新規設定は保護が有効。下書き変更とキャンセルで登録・保存を変更しない。
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("protection-open", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        assert_eq!(w.find("protection-toggle").checked(), Some(true));
        assert!(
            w.find("protection-row-wfi-gui-a")
                .label()
                .unwrap()
                .contains(".wfi-gui-a")
        );
        assert!(
            w.find("protection-row-wfi-gui-a")
                .label()
                .unwrap()
                .contains("既存のアイコン画像")
        );
        w.click("protection-toggle", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(load().unwrap().preserve_existing_icons);
    assert_eq!(snapshot(&registry._classes), baseline);
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("protection-back", cx);
        w.render_frame(cx);
        w.click("protection-open", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        assert_eq!(w.find("protection-toggle").checked(), Some(true));
        w.click("protection-toggle", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("protection-save", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(!load().unwrap().preserve_existing_icons);
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("protection-back", cx);
    })
    .unwrap();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click(pack_button(&manager, cx, "apply", 0), cx);
    })
    .unwrap();
    cx.run_until_parked();
    let applied = load().unwrap();
    assert_eq!(applied.active.as_ref(), Some(&imported.packs[0].id));
    assert!(!applied.journal.is_empty());
    assert!(home().unwrap().join("recovery.json").is_file());
    let dll: String = registry
        ._classes
        .open_subkey(format!(
            r"CLSID\{}\InprocServer32",
            file_icons_windows::CLSID_TEXT
        ))
        .unwrap()
        .get_value("")
        .unwrap();
    assert!(std::path::Path::new(&dll).is_file());
    assert!(
        std::path::Path::new(&dll)
            .starts_with(home().unwrap().canonicalize().unwrap().join("shell"))
    );
    assert_ne!(snapshot(&registry._classes), baseline);
    let registered = snapshot(&registry._classes);
    // 適用中の保護を有効にすると元登録へ戻り、除外理由を実際の保存結果から読める。
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("protection-open", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        assert!(w.find("protection-row-wfi-gui-a").visible());
        assert!(
            w.find("protection-result-current-wfi-gui-a")
                .label()
                .unwrap()
                .contains("パックのアイコン")
        );
        w.click("protection-allowed", cx);
        w.render_frame(cx);
        assert!(w.find("protection-row-wfi-gui-a").visible());
        w.click("protection-toggle", cx);
        w.render_frame(cx);
        assert!(
            w.find("protection-row-wfi-gui-a").visible(),
            "変更後も絞り込んだ行を保持する"
        );
        assert!(
            w.find("protection-result-current-wfi-gui-a")
                .label()
                .unwrap()
                .contains("パックのアイコン")
        );
        assert!(
            w.find("protection-result-after-wfi-gui-a")
                .label()
                .unwrap()
                .contains("元の表示")
        );
    })
    .unwrap();
    cx.run_until_parked();
    assert!(!load().unwrap().preserve_existing_icons);
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("protection-save", cx);
    })
    .unwrap();
    cx.run_until_parked();
    let protected = load().unwrap();
    assert!(protected.preserve_existing_icons && protected.journal.is_empty());
    assert_eq!(snapshot(&registry._classes), baseline);
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        assert!(
            w.find("protection-row-wfi-gui-a").visible(),
            "保存後も変更した行を保持する"
        );
        assert!(
            w.find("protection-result-current-wfi-gui-a")
                .label()
                .unwrap()
                .contains("元の表示")
        );
        w.click("protection-current", cx);
        w.render_frame(cx);
        assert!(
            w.find("protection-row-wfi-gui-a")
                .label()
                .unwrap()
                .contains(".wfi-gui-a")
        );
        w.click("protection-back", cx);
        w.render_frame(cx);
        w.click("restore", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(load().unwrap().active.is_none());
    assert_eq!(snapshot(&registry._classes), baseline);
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click(pack_button(&manager, cx, "apply", 0), cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(load().unwrap().active.is_some());
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("en", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("protection-open", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        assert_eq!(
            w.find("protection-toggle").label(),
            Some("Protect existing icons by default")
        );
        assert!(
            w.find("protection-row-wfi-gui-a")
                .label()
                .unwrap()
                .contains("An existing icon image")
        );
        w.click("protection-toggle", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("protection-save", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(snapshot(&registry._classes), registered);
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("protection-back", cx);
        w.render_frame(cx);
        w.click("ja", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click(pack_button(&manager, cx, "targets", 0), cx);
        w.render_frame(cx);
        w.click("target-toggle-file:dockerfile", cx);
        w.render_frame(cx);
        assert_eq!(w.find("target-save").label(), Some("変更を適用"));
        assert!(w.try_find("target-apply").is_none());
    })
    .unwrap();
    cx.run_until_parked();
    let previous_selection = load().unwrap().active_selection;
    let recovery_before = fs::read(home().unwrap().join("recovery.json")).unwrap();
    let source_dll = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .join("file_icons_shell.dll");
    use std::os::windows::fs::OpenOptionsExt;
    let locked_dll = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&source_dll)
        .unwrap();
    cx.update_window(window, |_, w, cx| {
        w.click("target-save", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(snapshot(&registry._classes), registered);
    assert_eq!(load().unwrap().active_selection, previous_selection);
    assert_eq!(
        fs::read(home().unwrap().join("recovery.json")).unwrap(),
        recovery_before
    );
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        assert_eq!(
            w.find("target-toggle-file:dockerfile").checked(),
            Some(false)
        );
        assert!(w.find("target-status").label().unwrap().contains("失敗"));
    })
    .unwrap();
    drop(locked_dll);
    cx.update_window(window, |_, w, cx| {
        w.click("target-save", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(!load().unwrap().active_selection.enabled("Dockerfile"));
    assert_eq!(load().unwrap().active, applied.active);
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("target-reset-file:dockerfile", cx);
        w.render_frame(cx);
        w.click("target-save", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(load().unwrap().active_selection.enabled("Dockerfile"));
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("target-disable-results", cx);
        w.render_frame(cx);
        w.click("target-save", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(load().unwrap().active.is_none());
    assert_eq!(snapshot(&registry._classes), baseline);
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("target-enable-results", cx);
        w.render_frame(cx);
        w.click("target-apply", cx);
        w.render_frame(cx);
        assert_eq!(w.find("target-save-progress").label(), Some("適用中…"));
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(load().unwrap().active, applied.active);

    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        assert!(w.try_find("target-save-progress").is_none());
        w.click("target-back", cx);
    })
    .unwrap();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        assert_eq!(
            w.find(pack_button(&manager, cx, "apply", 0)).label(),
            Some("適用中")
        );
        w.click(pack_button(&manager, cx, "apply", 0), cx);
        w.click(pack_button(&manager, cx, "remove-pack", 0), cx);
        w.click("cleanup-shell", cx);
        manager.update(cx, |v, _| assert!(!v.operation.busy()));
        w.click("en", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        assert_eq!(
            w.find(pack_button(&manager, cx, "apply", 0)).label(),
            Some("Applied")
        );
    })
    .unwrap();
    assert_eq!(snapshot(&registry._classes), registered);
    cx.update_window(window, |_, w, cx| {
        w.click(pack_button(&manager, cx, "apply", 1), cx)
    })
    .unwrap();
    cx.run_until_parked();
    let switched = load().unwrap();
    assert_eq!(switched.active.as_ref(), Some(&imported.packs[1].id));
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        assert_eq!(
            w.find(pack_button(&manager, cx, "apply", 0)).label(),
            Some("Apply")
        );
        assert_eq!(
            w.find(pack_button(&manager, cx, "apply", 1)).label(),
            Some("Applied")
        );
    })
    .unwrap();
    // 他のアプリが後から変えた値は、GUIから復元を試しても上書きしない。
    let conflict = switched
        .journal
        .iter()
        .find(|c| c.key.ends_with(r"Unknown\DefaultIcon"))
        .unwrap();
    let key = registry
        ._user
        .open_subkey_with_flags(&conflict.key, KEY_ALL_ACCESS)
        .unwrap();
    key.set_value("", &"external replacement").unwrap();
    cx.update_window(window, |_, w, cx| w.click("restore", cx))
        .unwrap();
    cx.run_until_parked();
    assert_eq!(
        key.get_value::<String, _>("").unwrap(),
        "external replacement"
    );
    assert!(!load().unwrap().journal.is_empty());
    assert!(home().unwrap().join("recovery.json").is_file());
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        assert!(
            w.find("status")
                .label()
                .unwrap()
                .starts_with("Icon settings have changed outside this app.")
        );
        let detail = w.find("status").label().unwrap().to_owned();
        assert!(detail.contains(&format!("HKCU\\{}", conflict.key)));
        assert!(detail.contains(&home().unwrap().join("settings.json").display().to_string()));
        w.click("ja", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        assert!(
            w.find("status")
                .label()
                .unwrap()
                .starts_with("外部アプリによってアイコン設定が変更されています。")
        );
    })
    .unwrap();
    cx.update_window(window, |_, w, cx| w.click("restore", cx))
        .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        let detail = w.find("status").label().unwrap().to_owned();
        assert!(detail.contains("登録状態が保存済みの記録と異なる"));
        assert!(
            w.find("conflicts-open")
                .label()
                .unwrap()
                .contains("競合あり")
        );
        assert!(manager.read(cx).conflict_attention);
        assert!(detail.contains(&format!("HKCU\\{}", conflict.key)));
    })
    .unwrap();
    assert_eq!(conflict.after.kind, REG_SZ as u32);
    // 競合確認を開閉しても設定と登録を変えず、実行は別の最終確認を必要とする。
    let mut inactive = load().unwrap();
    inactive.active = None;
    file_icons_windows::save(&inactive).unwrap();
    assert!(load().unwrap().active.is_none());
    let before_review = snapshot(&registry._classes);
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("conflicts-open", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.simulate_window_resize(window, size(px(640.), px(700.)));
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        assert!(
            w.find("recovery-summary")
                .label()
                .unwrap()
                .contains("外部変更: 1")
        );
        for id in ["recovery-close", "recovery-review", "recovery-refresh"] {
            let bounds = w.find(id).bounds();
            assert!(bounds.left() >= px(0.) && bounds.right() <= px(640.));
            assert!(bounds.top() >= px(0.) && bounds.bottom() <= px(700.));
        }
        assert!(w.try_find("recovery-execute").is_none());
        assert_eq!(w.find("recovery-preserve").checked(), Some(true));
        w.click("recovery-preserve", cx);
    })
    .unwrap();
    // Tabで実際に到達したフォーカスへSpaceを送り、選択の結果を確かめる。
    for id in ["recovery-reapply", "recovery-preserve"] {
        let mut reached = false;
        for _ in 0..24 {
            reached = cx
                .update_window(window, |_, w, cx| {
                    w.render_frame(cx);
                    if w.find(id).focused() == Some(true) {
                        true
                    } else {
                        w.press("tab", cx);
                        false
                    }
                })
                .unwrap();
            cx.run_until_parked();
            if reached {
                break;
            }
        }
        assert!(reached, "Tabから到達できない復旧方法: {id}");
        cx.update_window(window, |_, w, cx| w.press("space", cx))
            .unwrap();
        cx.run_until_parked();
        cx.update_window(window, |_, w, cx| {
            w.render_frame(cx);
            assert_eq!(w.find(id).checked(), Some(true));
        })
        .unwrap();
    }
    cx.update_window(window, |_, w, cx| w.click("navigate-back", cx))
        .unwrap();
    assert_eq!(snapshot(&registry._classes), before_review);
    for (mode, language) in [(ThemeMode::Dark, "ja"), (ThemeMode::Light, "en")] {
        cx.update_window(window, |_, w, cx| {
            theme::apply(mode, cx);
            gpui_kit::component::theme::Theme::update(cx, |t| t.font_size = px(18.));
            manager.update(cx, |v, _| v.settings.language = language.into());
            w.render_frame(cx);
            w.click("conflicts-open", cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(window, |_, w, cx| {
            w.render_frame(cx);
            assert_eq!(
                w.find("recovery-preserve").bounds().left(),
                w.find("recovery-summary").bounds().left()
            );
            w.click("recovery-review", cx);
            w.render_frame(cx);
            for id in ["recovery-close", "recovery-execute"] {
                let bounds = w.find(id).bounds();
                assert!(bounds.left() >= px(0.) && bounds.right() <= px(640.));
                assert!(bounds.top() >= px(0.) && bounds.bottom() <= px(700.));
            }
            w.click("recovery-close", cx);
            w.render_frame(cx);
            assert!(w.find("recovery-preserve").visible());
            w.click("recovery-close", cx);
        })
        .unwrap();
        assert_eq!(snapshot(&registry._classes), before_review);
    }
    cx.update_window(window, |_, _, cx| {
        theme::apply(ThemeMode::Dark, cx);
        gpui_kit::component::theme::Theme::update(cx, |t| t.font_size = px(14.));
        manager.update(cx, |v, _| v.settings.language = "ja".into());
    })
    .unwrap();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("conflicts-open", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("recovery-reapply", cx);
        w.render_frame(cx);
        assert_eq!(w.find("recovery-reapply").checked(), Some(true));
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("recovery-review", cx);
        w.render_frame(cx);
        assert!(
            w.find("recovery-execute")
                .label()
                .unwrap()
                .contains("上書きして再適用")
        );
        w.click("recovery-execute", cx);
        w.render_frame(cx);
        assert_eq!(w.find("recovery-progress").label(), Some("復旧中…"));
        w.click("recovery-close", cx);
        assert!(w.find("recovery-progress").visible());
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(key.get_value::<String, _>("").unwrap(), "%1");
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        assert_eq!(w.find("recovery-status").label(), Some("完了しました。"));
        assert!(w.try_find("recovery-progress").is_none());
        w.click("recovery-close", cx);
        w.render_frame(cx);
        w.click("restore", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(
        key.get_value::<String, _>("").unwrap(),
        "external replacement"
    );
    assert!(load().unwrap().journal.is_empty());
    // 次の適用中に別の外部変更を作り、保持して解除する経路もGUIから実行する。
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click(pack_button(&manager, cx, "apply", 1), cx);
    })
    .unwrap();
    cx.run_until_parked();
    key.set_value("", &"another external icon").unwrap();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("en", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("conflicts-open", cx);
    })
    .unwrap();
    cx.run_until_parked();
    // 確認後の外部変更では失敗し、再取得するまで再実行できない。
    key.set_value("", &"latest external icon").unwrap();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("recovery-review", cx);
        w.render_frame(cx);
        w.click("recovery-execute", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(
        key.get_value::<String, _>("").unwrap(),
        "latest external icon"
    );
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        assert!(
            w.find("recovery-status")
                .label()
                .unwrap()
                .contains("State changed")
        );
        w.click("recovery-review", cx);
        w.render_frame(cx);
        assert!(w.try_find("recovery-execute").is_none());
        w.click("recovery-refresh", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("recovery-review", cx);
        w.render_frame(cx);
        assert_eq!(
            w.find("recovery-execute").label(),
            Some("Restore and preserve external changes")
        );
        w.click("recovery-execute", cx);
        w.render_frame(cx);
        assert_eq!(w.find("recovery-progress").label(), Some("Recovering…"));
    })
    .unwrap();
    cx.run_until_parked();
    assert!(load().unwrap().journal.is_empty());
    assert_eq!(
        key.get_value::<String, _>("").unwrap(),
        "latest external icon"
    );
    // 外部値以外の全キーが元どおりであることを、完全なスナップショットで照合する。
    let mut expected = baseline.clone();
    let current = key.get_raw_value("").unwrap();
    expected.get_mut(r"\Unknown\DefaultIcon").unwrap().insert(
        String::new(),
        (current.vtype as u32, current.bytes.into_owned()),
    );
    assert_eq!(snapshot(&registry._classes), expected);
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        assert!(w.try_find("recovery-review").is_none());
        assert!(w.try_find("recovery-execute").is_none());
        w.click("recovery-close", cx);
        w.render_frame(cx);
        w.click("conflicts-open", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("recovery-reapply", cx);
        w.render_frame(cx);
        assert_eq!(w.find("recovery-preserve").checked(), Some(true));
        assert!(
            w.find("recovery-summary")
                .label()
                .unwrap()
                .contains("Entries to change: 0")
        );
    })
    .unwrap();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("recovery-close", cx);
        w.render_frame(cx);
        w.click("ja", cx);
    })
    .unwrap();
    cx.run_until_parked();
    // 検証用の外部変更だけを片付け、後続の削除検証へ元の基準を戻す。
    key.set_raw_value(
        "",
        &RegValue {
            bytes: baseline[r"\Unknown\DefaultIcon"][""].1.clone().into(),
            vtype: REG_SZ,
        },
    )
    .unwrap();
    drop(key);
    cx.update_window(window, |_, w, cx| w.click("restore", cx))
        .unwrap();
    cx.run_until_parked();
    assert_eq!(snapshot(&registry._classes), baseline);
    assert_eq!(snapshot(&registry._effective), effective_baseline);
    let restored = load().unwrap();
    assert!(restored.active.is_none() && restored.journal.is_empty());
    assert!(!home().unwrap().join("recovery.json").exists());
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("cleanup-shell", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(
        fs::read_dir(home().unwrap().join("shell"))
            .unwrap()
            .next()
            .is_none()
    );
    for _ in 0..2 {
        cx.update_window(window, |_, w, cx| {
            w.render_frame(cx);
            w.click(pack_button(&manager, cx, "remove-pack", 0), cx);
            w.within("dialog").click("ok", cx);
        })
        .unwrap();
        cx.run_until_parked();
    }
    let removed = load().unwrap();
    assert!(removed.packs.is_empty() && removed.pending_pack_removals.is_empty());
    assert!(
        fs::read_dir(home().unwrap().join("packs"))
            .unwrap()
            .next()
            .is_none()
    );
    assert_eq!(snapshot(&registry._classes), baseline);
    for (path, contents) in sources {
        assert_eq!(fs::read(path).unwrap(), contents);
    }
}
