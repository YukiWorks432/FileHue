use super::{Manager, theme};
mod about;
mod imports;
mod pack_guidance;
mod thumbnails;
use crate::test_process::CommandOutput;

fn pack_button(
    manager: &gpui_kit::Entity<Manager>,
    cx: &gpui_kit::App,
    action: &str,
    ix: usize,
) -> gpui_kit::SharedString {
    super::library_ui::pack_control(action, &manager.read(cx).settings.packs[ix].id)
}
use file_icons_windows::{home, load};
use gpui_kit::component::theme::{ActiveTheme, ThemeMode};
use gpui_kit::test::TestWindowExt;
use gpui_kit::{AppContext, Bounds, TestAppContext, WindowBounds, WindowOptions, px, size};

fn select_target_filter(window: gpui_kit::AnyWindowHandle, index: usize, cx: &mut TestAppContext) {
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("target-filter-open", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.within("popup-menu").click(index, cx);
    })
    .unwrap();
    cx.run_until_parked();
}

/// ユーザー設定と同時起動ロックを、子プロセス専用の保存先に隔離する。
#[test]
fn gui_contract_in_isolated_profile() {
    let profile = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "ui_tests::gui_contract_in_child",
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
fn gui_contract_in_child(cx: &mut TestAppContext) {
    assert_eq!(std::env::var("WFI_GUI_PROBE").as_deref(), Ok("1"));
    let fixture = home().unwrap().join("fixture");
    std::fs::create_dir_all(&fixture).unwrap();
    std::fs::write(
        fixture.join("package.json"),
        r#"{
        "name":"ui-fixture", "publisher":"test", "version":"1.0.0",
        "contributes":{"iconThemes":[{"id":"sample","label":"Sample","path":"theme.json"}]}
    }"#,
    )
    .unwrap();
    std::fs::write(
        fixture.join("theme.json"),
        r#"{
        "iconDefinitions":{"icon":{"iconPath":"icon.svg"}},
        "fileExtensions":{"json":"icon"},"fileNames":{"Cargo.toml":"icon"}
    }"#,
    )
    .unwrap();
    std::fs::write(fixture.join("icon.svg"), r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="16" height="16" fill="#ef1200"/></svg>"##).unwrap();
    let (window, manager) = cx.update(|cx| {
        gpui_kit::init(cx);
        theme::apply(ThemeMode::Dark, cx);
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(820.), px(700.)),
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
        window.click("restore", cx);
        manager.update(cx, |view, _| {
            assert!(matches!(view.operation.status(), super::Status::Idle))
        });
        window.press("tab", cx);
        window.press("tab", cx);
        assert_eq!(window.find("en").focused(), Some(true));
        window.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("import").label(), Some("Import pack…"));
        assert!(cx.theme().mode.is_dark());
        window.click("appearance", cx);
        assert!(!cx.theme().mode.is_dark());
        window.click("appearance", cx);
        assert!(cx.theme().mode.is_dark());
        window.click("import", cx);
        window.click("ja", cx);
        manager.update(cx, |view, _| {
            assert!(view.operation.busy());
            assert_eq!(view.settings.language, "en");
        });
    })
    .unwrap();
    assert_eq!(load().unwrap().language, "en");
    assert!(cx.did_prompt_for_paths());
    cx.simulate_path_prompt_response(|options| {
        assert!(options.directories && !options.files && !options.multiple);
        Some(vec![fixture.clone()])
    });
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("cancel-import", cx);
        assert!(window.try_find(("theme", 0usize)).is_none());
        assert!(load().unwrap().packs.is_empty());
        assert!(fixture.join("package.json").is_file());
        window.click("import", cx);
    })
    .unwrap();
    cx.simulate_path_prompt_response(|_| Some(vec![fixture.clone()]));
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(("theme", 0usize), cx);
        window.click("ja", cx);
        manager.update(cx, |view, _| {
            assert!(view.operation.busy());
            assert_eq!(view.settings.language, "en");
        });
    })
    .unwrap();
    cx.run_until_parked();
    let settings = load().unwrap();
    assert_eq!(settings.packs.len(), 1);
    assert!(
        settings.packs[0]
            .theme
            .resolve("Cargo.toml")
            .unwrap()
            .is_file()
    );
    assert!(settings.active.is_none() && settings.journal.is_empty());
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.find(pack_button(&manager, cx, "apply", 0)).visible());
        window.click("ja", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.find("status").label(),
            Some("パックを取り込みました。")
        );
        // 復帰済みのバックエンド応答を、失敗・未復帰の案内と区別して表示する。
        manager.update(cx, |view, cx| {
            view.run(
                super::Kind::General,
                || {
                    Err(file_icons_windows::PreviousPackRestored {
                        cause: anyhow::anyhow!("invalid test theme"),
                    }
                    .into())
                },
                cx,
            )
        });
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window
                .find("status")
                .label()
                .unwrap()
                .contains("以前のアイコンパックへ戻しました")
        );
        window.click("en", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let message = window.find("status");
        assert!(
            message
                .label()
                .unwrap()
                .contains("previous icon pack has been restored")
        );
        assert!(message.label().unwrap().contains("invalid test theme"));
        window.click("ja", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("import", cx);
    })
    .unwrap();
    assert_eq!(load().unwrap().packs.len(), 1);
    assert_eq!(load().unwrap().language, "ja");
    // 読み込み失敗でも既存パックを失わず、再試行できる。
    cx.simulate_path_prompt_response(|_| Some(vec![fixture.join("missing")]));
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("en", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let message = window.find("status");
        assert!(message.label().unwrap().starts_with("Cannot read theme."));
        assert!(message.label().unwrap().contains("missing"));
        window.click("import", cx);
    })
    .unwrap();
    assert!(cx.did_prompt_for_paths());
    cx.simulate_path_prompt_response(|_| Some(vec![fixture.clone()]));
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.find(("theme", 0usize)).visible());
        assert_eq!(window.find("status").label(), Some(""));
    })
    .unwrap();
    assert_eq!(load().unwrap().packs.len(), 1);
    // 変換失敗でも選択を失わず、無関係な取得ボタンを再試行表示にしない。
    let valid_svg = std::fs::read(fixture.join("icon.svg")).unwrap();
    std::fs::write(fixture.join("icon.svg"), b"not an SVG").unwrap();
    cx.update_window(window, |_, w, cx| {
        w.click(("theme", 0usize), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        assert!(w.find(("theme", 0usize)).visible());
        assert!(
            w.find("status")
                .label()
                .unwrap()
                .starts_with("Could not import this theme.")
        );
        w.click("browse", cx);
        w.click("browse", cx);
        assert!(w.try_find(pack_button(&manager, cx, "apply", 0)).is_none());
        assert_eq!(w.find(("download", 0usize)).label(), Some("Download"));
        assert_eq!(w.find(("download", 1usize)).label(), Some("Download"));
        w.press("tab", cx);
        w.press("alt-left", cx);
        w.render_frame(cx);
        assert!(w.find(("theme", 0usize)).visible());
        assert!(w.try_find(("download", 0usize)).is_none());
        assert!(w.find(pack_button(&manager, cx, "apply", 0)).visible());
    })
    .unwrap();
    std::fs::write(fixture.join("icon.svg"), valid_svg).unwrap();
    cx.update_window(window, |_, w, cx| {
        w.click(("theme", 0usize), cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(load().unwrap().packs.len(), 1);
    // 保存失敗後は、保存済みの言語と画面の選択を再び一致させる。
    use std::os::windows::fs::OpenOptionsExt as _;
    let settings_file = home().unwrap().join("settings.json");
    let locked = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&settings_file)
        .unwrap();
    cx.update_window(window, |_, w, cx| {
        w.click("ja", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        assert_eq!(w.find("import").label(), Some("Import pack…"));
        assert!(
            w.find("status")
                .label()
                .unwrap()
                .starts_with("Cannot save display language.")
        );
    })
    .unwrap();
    assert_eq!(load().unwrap().language, "en");
    drop(locked);
    // 破損した設定で起動した画面は、言語だけ変えてもファイルを上書きしない。
    cx.update_window(window, |_, _, cx| {
        manager.update(cx, |view, _| {
            view._guard.take();
        })
    })
    .unwrap();
    let settings_path = home().unwrap().join("settings.json");
    let valid_settings = std::fs::read(&settings_path).unwrap();
    let mut unsupported: serde_json::Value = serde_json::from_slice(&valid_settings).unwrap();
    unsupported["format_version"] = 3.into();
    let unsupported = serde_json::to_vec(&unsupported).unwrap();
    std::fs::write(&settings_path, &unsupported).unwrap();
    let (unsupported_window, unsupported_manager) = cx.update(|cx| {
        gpui_kit::open_window(WindowOptions::default(), cx, Manager::prepare()).unwrap()
    });
    cx.update_window(unsupported_window, |_, window, cx| {
        window.render_frame(cx);
        window.click("en", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(unsupported_window, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window
                .find("status")
                .label()
                .unwrap()
                .contains("settings format is unsupported")
        );
        assert!(window.try_find("browse-empty").is_none());
        window.click("import", cx);
    })
    .unwrap();
    assert!(!cx.did_prompt_for_paths());
    assert_eq!(std::fs::read(&settings_path).unwrap(), unsupported);
    cx.update(|cx| {
        unsupported_manager.update(cx, |view, _| {
            view._guard.take();
        });
    });
    std::fs::write(&settings_path, &valid_settings).unwrap();
    std::fs::write(&settings_path, b"broken settings").unwrap();
    let (invalid_window, invalid) = cx.update(|cx| {
        gpui_kit::open_window(WindowOptions::default(), cx, Manager::prepare()).unwrap()
    });
    assert!(file_icons_manager_service::Manager::acquire().is_err());
    cx.update_window(invalid_window, |_, window, cx| {
        window.render_frame(cx);
        window.click("en", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(invalid_window, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window
                .find("status")
                .label()
                .unwrap()
                .starts_with("Cannot open settings.")
        );
        window.click("import", cx);
    })
    .unwrap();
    assert!(!cx.did_prompt_for_paths());
    assert_eq!(std::fs::read(&settings_path).unwrap(), b"broken settings");
    cx.update(|cx| {
        invalid.update(cx, |view, _| {
            view._guard.take();
        })
    });
    // 登録前に停止した場合と同じ、レジストリへ一度も書いていない記録を使う。
    // 現在値が記録のbefore(None)と等しいため、復元でレジストリの書き込みは発生しない。
    let key = format!(
        "Software\\Classes\\WfiGuiRecovery-{}\\DefaultIcon",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    use file_icons_core::registry::Registry as _;
    assert!(
        file_icons_windows::UserRegistry
            .read(&key, "")
            .unwrap()
            .is_none()
    );
    let record = serde_json::json!({
        "version":1, "active":null, "fallbacks":{}, "original_handlers":{}, "created_keys":[],
        "journal":[{"key":key,"name":"","before":null,"after":{"kind":1,"bytes":[0,0]}}]
    });
    std::fs::write(
        home().unwrap().join("recovery.json"),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();
    std::fs::write(&settings_path, &unsupported).unwrap();
    let (unsupported_recovery_window, unsupported_recovery) = cx.update(|cx| {
        gpui_kit::open_window(WindowOptions::default(), cx, Manager::prepare()).unwrap()
    });
    cx.update_window(unsupported_recovery_window, |_, window, cx| {
        window.render_frame(cx);
        window.click("en", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(unsupported_recovery_window, |_, window, cx| {
        window.render_frame(cx);
        let status_element = window.find("status");
        let status = status_element.label().unwrap();
        assert!(status.contains("settings format is unsupported"));
        assert!(status.contains("backs up and resets"));
    })
    .unwrap();
    assert_eq!(std::fs::read(&settings_path).unwrap(), unsupported);
    cx.update(|cx| {
        unsupported_recovery.update(cx, |view, _| {
            view._guard.take();
        });
    });
    std::fs::write(&settings_path, b"broken settings").unwrap();
    let (recovery_window, recovered_manager) =
        cx.update(|cx| gpui_kit::open_window(WindowOptions::default(), cx, Manager::open).unwrap());
    cx.update_window(recovery_window, |_, window, cx| {
        window.render_frame(cx);
        window.click("en", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(recovery_window, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window
                .find("status")
                .label()
                .unwrap()
                .contains("saved recovery record")
        );
        window.click("import", cx);
        window.click("restore", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(!home().unwrap().join("recovery.json").exists());
    assert!(load().unwrap().journal.is_empty());
    let backups = std::fs::read_dir(home().unwrap())
        .unwrap()
        .map(Result::unwrap)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("settings-damaged-")
        })
        .collect::<Vec<_>>();
    assert_eq!(backups.len(), 1);
    assert_eq!(
        std::fs::read(backups[0].path()).unwrap(),
        b"broken settings"
    );
    cx.update_window(recovery_window, |_, window, cx| {
        window.render_frame(cx);
        window.click("import", cx);
    })
    .unwrap();
    assert!(cx.did_prompt_for_paths());
    cx.simulate_path_prompt_response(|_| None);
    cx.run_until_parked();
    // 通常の取り込みから、素材削除の保留とGUIでの再試行までを操作する。
    cx.update_window(recovery_window, |_, window, cx| window.click("import", cx))
        .unwrap();
    cx.simulate_path_prompt_response(|_| Some(vec![fixture.clone()]));
    cx.run_until_parked();
    cx.update_window(recovery_window, |_, window, cx| {
        window.render_frame(cx);
        window.click(("theme", 0usize), cx);
    })
    .unwrap();
    cx.run_until_parked();
    let pack = load().unwrap().packs[0].clone();
    let folder = pack.provenance.parent().unwrap().to_path_buf();
    let note = folder.join("user-notes.txt");
    std::fs::write(&note, b"preserve this file").unwrap();
    cx.update_window(recovery_window, |_, window, cx| {
        window.render_frame(cx);
        let remove = pack_button(&recovered_manager, cx, "remove-pack", 0);
        window.focus(&recovered_manager.read(cx).focus.clone(), cx);
        for _ in 0..32 {
            if window.find(remove.clone()).focused() == Some(true) {
                break;
            }
            window.press("tab", cx);
        }
        assert_eq!(window.find(remove.clone()).focused(), Some(true));
        window.press("enter", cx);
        assert_eq!(load().unwrap().packs.len(), 1);
        window.press("escape", cx);
        assert_eq!(load().unwrap().packs.len(), 1);
    })
    .unwrap();
    cx.run_until_parked();
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(300));
    cx.run_until_parked();
    cx.update_window(recovery_window, |_, window, cx| {
        window.render_frame(cx);
        let remove = pack_button(&recovered_manager, cx, "remove-pack", 0);
        assert_eq!(window.find(remove.clone()).focused(), Some(true));
        window.click(remove, cx);
        window.within("dialog").click("ok", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(load().unwrap().packs.is_empty());
    assert_eq!(load().unwrap().pending_pack_removals.len(), 1);
    assert_eq!(std::fs::read(&note).unwrap(), b"preserve this file");
    cx.update_window(recovery_window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.find("cleanup-packs").visible());
        window.click("en", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(recovery_window, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window
                .find("status")
                .label()
                .unwrap()
                .contains("removal request was saved")
        );
    })
    .unwrap();
    std::fs::remove_file(note).unwrap();
    cx.update_window(recovery_window, |_, window, cx| {
        window.click("cleanup-packs", cx)
    })
    .unwrap();
    cx.run_until_parked();
    assert!(!folder.exists());
    assert!(load().unwrap().pending_pack_removals.is_empty());
    assert!(fixture.join("package.json").is_file());
    cx.update_window(recovery_window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("cleanup-packs").is_none());
    })
    .unwrap();

    // 対象編集を離れるときだけ変更を確認し、保存せずに破棄できる。
    cx.update_window(recovery_window, |_, w, cx| {
        w.render_frame(cx);
        w.click("import", cx);
    })
    .unwrap();
    cx.simulate_path_prompt_response(|_| Some(vec![fixture.clone()]));
    cx.run_until_parked();
    cx.update_window(recovery_window, |_, w, cx| {
        w.render_frame(cx);
        w.click(("theme", 0usize), cx);
    })
    .unwrap();
    cx.run_until_parked();
    let id = load().unwrap().packs[0].id.clone();
    cx.update_window(recovery_window, |_, w, cx| {
        w.render_frame(cx);
        w.click(pack_button(&recovered_manager, cx, "targets", 0), cx);
        w.render_frame(cx);
        w.click("target-toggle-extension:json", cx);
        w.render_frame(cx);
        w.click("target-back", cx);
        w.render_frame(cx);
        w.within("dialog").click("cancel", cx);
        w.render_frame(cx);
        assert_eq!(
            w.find("target-toggle-extension:json").checked(),
            Some(false)
        );
        w.click("target-back", cx);
        w.render_frame(cx);
        w.within("dialog").click("ok", cx);
        w.render_frame(cx);
        w.click(pack_button(&recovered_manager, cx, "targets", 0), cx);
        w.render_frame(cx);
        assert_eq!(w.find("target-toggle-extension:json").checked(), Some(true));
        w.click("target-toggle-file:cargo.toml", cx);
        w.render_frame(cx);
        w.click("target-save", cx);
    })
    .unwrap();
    cx.run_until_parked();
    let saved = load().unwrap();
    assert!(saved.selections[&id].enabled("data.json"));
    assert!(!saved.selections[&id].enabled("Cargo.toml"));
    cx.update_window(recovery_window, |_, w, cx| {
        w.render_frame(cx);
        assert_eq!(
            w.find("target-status").label(),
            Some("Target selection saved.")
        );
        w.click("target-back", cx);
        w.render_frame(cx);
        assert!(
            w.try_find("status")
                .is_none_or(|status| status.label().unwrap_or("").is_empty())
        );
    })
    .unwrap();
}

#[test]
fn download_gui_in_isolated_profile() {
    let profile = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "ui_tests::download_gui_in_child",
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
fn download_gui_in_child(cx: &mut TestAppContext) {
    assert_eq!(std::env::var("WFI_GUI_PROBE").as_deref(), Ok("1"));
    let (window, manager) = cx.update(|cx| {
        gpui_kit::init(cx);
        theme::apply(ThemeMode::Dark, cx);
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1000.), px(1800.)),
                    cx,
                ))),
                ..Default::default()
            },
            cx,
            Manager::open,
        )
        .unwrap()
    });
    cx.update_window(window, |_, window, cx| {
        manager.update(cx, |view, _| {
            view.download_source = file_icons_catalog::test_support::download_fixture
        });
        window.render_frame(cx);
        window.click("browse", cx);
        window.click(("download", 0usize), cx);
        window.click("import", cx);
        window.click("cancel-download", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(load().unwrap().packs.is_empty());
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window
                .find("status")
                .label()
                .unwrap()
                .contains("キャンセル")
        );
        window.click("en", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window
                .find("status")
                .label()
                .unwrap()
                .starts_with("Cancelled.")
        );
        // 失敗後にも次の取得を開始でき、通信以降は実際のZIP検証・ICO変換を通る。
        manager.update(cx, |view, _| {
            view.download_source = |_, _, _| Err(anyhow::anyhow!("fixture network failure"))
        });
        window.click(("download", 0usize), cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(load().unwrap().packs.is_empty());
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.find(("download", 0usize)).label(),
            Some("Retry download")
        );
        assert_eq!(window.find(("download", 1usize)).label(), Some("Download"));
        assert!(
            window
                .find("status")
                .label()
                .unwrap()
                .contains("fixture network failure")
        );
        manager.update(cx, |view, _| {
            view.download_source = file_icons_catalog::test_support::download_fixture
        });
        window.click(("download", 0usize), cx);
    })
    .unwrap();
    cx.run_until_parked();
    let settings = load().unwrap();
    assert_eq!(settings.packs.len(), 1);
    assert!(settings.active.is_none() && settings.journal.is_empty());
    assert_eq!(settings.packs[0].warnings.len(), 1);
    let id = settings.packs[0].id.clone();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.find(("download", 0usize)).label(),
            Some("Downloaded")
        );
        window.click(("download", 0usize), cx);
        manager.update(cx, |view, _| assert!(!view.operation.busy()));
        window.click("library", cx);
        window.render_frame(cx);
        let provenance = manager.read(cx).settings.packs[0].provenance.clone();
        let mut record: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&provenance).unwrap()).unwrap();
        record["licenses"]["NOTICE"] =
            serde_json::json!(format!("{}END OF LICENSE", "License line.\n".repeat(2500)));
        std::fs::write(&provenance, serde_json::to_vec(&record).unwrap()).unwrap();
        window.click(pack_button(&manager, cx, "details", 0), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("toggle-rights", cx);
        for pair in [(0, 1), (2, 3), (4, 5)] {
            let left = window
                .find(gpui_kit::SharedString::from(format!("example-{}", pair.0)))
                .label()
                .unwrap()
                .to_owned();
            let right = window
                .find(gpui_kit::SharedString::from(format!("example-{}", pair.1)))
                .label()
                .unwrap()
                .to_owned();
            assert_ne!(left.split(" · ").last(), right.split(" · ").last());
        }
        let rights = manager.read(cx).rights.as_ref().unwrap().clone();
        assert!(rights.read(cx).value().contains("Copyright fixture author"));
        assert!(rights.read(cx).value().contains("END OF LICENSE"));
        window.render_frame(cx);
        window.scroll(
            "pack-rights",
            gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-500.))),
            cx,
        );
        window.render_frame(cx);
        assert!(rights.read(cx).scroll_offset().y < px(0.));
        let before = rights.read(cx).value();
        window.click("pack-rights", cx);
        window.press("ctrl-a", cx);
        window.input("This license must remain unchanged", cx);
        assert_eq!(rights.read(cx).value(), before);
        window.click("preview-name", cx);
        window.input("package.json", cx);
        assert!(
            window
                .find("typed-preview")
                .label()
                .unwrap()
                .contains("package.json")
        );
        assert!(
            window
                .within("typed-preview")
                .try_find("fallback-hint")
                .is_none()
        );
        window.press("ctrl-a", cx);
        window.input("unknown.unmapped", cx);
        let fallback = window.within("typed-preview").find("fallback-hint");
        assert!(fallback.visible());
        assert!(fallback.bounds().size.width > px(0.) && fallback.bounds().size.height > px(0.));
        assert_eq!(fallback.label(), Some("Windows default icon"));
        assert!(
            window
                .find("typed-preview")
                .label()
                .unwrap()
                .contains("Windows default icon")
        );
        window.click("ja", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window
                .find("typed-preview")
                .label()
                .unwrap()
                .contains("Windowsの通常のアイコン")
        );
        let fallback = window.within("typed-preview").find("fallback-hint");
        assert!(fallback.visible());
        assert!(fallback.bounds().size.width > px(0.) && fallback.bounds().size.height > px(0.));
        assert_eq!(fallback.label(), Some("Windowsの通常のアイコン"));
        assert_eq!(
            window.find(pack_button(&manager, cx, "apply", 0)).label(),
            Some("適用する")
        );
        // 実機への登録はせず、適用中のパックの操作制限だけを確認する。
        manager.update(cx, |view, _| view.settings.active = Some(id.clone()));
        window.render_frame(cx);
        assert_eq!(
            window.find(pack_button(&manager, cx, "apply", 0)).label(),
            Some("適用中")
        );
        window.click(pack_button(&manager, cx, "apply", 0), cx);
        window.click(pack_button(&manager, cx, "remove-pack", 0), cx);
        manager.update(cx, |view, _| assert!(!view.operation.busy()));
        // 複数パックでも、見本が選んだパックの直下に現れ、次のパックより前に読める。
        manager.update(cx, |view, _| {
            view.settings.active = None;
            view.selected = None;
            let mut second = view.settings.packs[0].clone();
            second.id = "test.second/theme".into();
            second.label = "Second pack".into();
            view.settings.packs.push(second);
        });
        window.render_frame(cx);
        window.click(pack_button(&manager, cx, "details", 0), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let preview = window.find("example-0");
        let selected = window.find(pack_button(&manager, cx, "details", 0));
        let following = window.find(pack_button(&manager, cx, "details", 1));
        assert!(preview.visible());
        assert!(preview.bounds().top() > selected.bounds().bottom());
        assert!(preview.bounds().bottom() < following.bounds().top());
        // 同じトグルで閉じ、キーボードでも再展開できる。
        window.click(pack_button(&manager, cx, "details", 0), cx);
        manager.update(cx, |view, _| assert!(view.selected.is_none()));
        window.focus(&manager.read(cx).focus.clone(), cx);
        for _ in 0..30 {
            if window
                .find(pack_button(&manager, cx, "details", 0))
                .focused()
                == Some(true)
            {
                break;
            }
            window.press("tab", cx);
        }
        assert_eq!(
            window
                .find(pack_button(&manager, cx, "details", 0))
                .focused(),
            Some(true)
        );
        window.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.find("example-0").visible());
        window.press("escape", cx);
        manager.update(cx, |view, _| assert!(view.selected.is_none()));
        // 後着の権利表示で閉じたプレビューが復活しない。
        window.click(pack_button(&manager, cx, "details", 1), cx);
        window.click(pack_button(&manager, cx, "details", 1), cx);
        manager.update(cx, |view, _| assert!(view.selected.is_none()));
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        manager.update(cx, |view, _| {
            assert!(view.selected.is_none());
            assert!(view.rights.is_none());
        });
    })
    .unwrap();
    // 小さなウィンドウと文字拡大でも、主要操作と状態表示を一覧の外に保持する。
    cx.simulate_window_resize(window, size(px(640.), px(640.)));
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        cx.update_window(window, |_, window, cx| {
            theme::apply(mode, cx);
            gpui_kit::component::theme::Theme::update(cx, |theme| theme.font_size = px(18.));
            window.render_frame(cx);
            let toolbar = window.find("import").bounds();
            let footer = window.find("status").bounds();
            for target in ["import", "appearance", "restore"] {
                let control = window.find(target);
                assert!(control.visible(), "{target}");
                assert!(control.bounds().right() <= px(640.), "{target}");
            }
            window.scroll(
                "pack-scroll",
                gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-500.))),
                cx,
            );
            assert_eq!(window.find("import").bounds(), toolbar);
            assert_eq!(window.find("status").bounds(), footer);
        })
        .unwrap();
    }
    assert_eq!(load().unwrap().packs[0].id, id);
    assert!(load().unwrap().journal.is_empty());
}

// この子プロセスのHKCU/HKCRだけを専用キーへ振り向け、実際のインストールと独立に検証する。
struct IsolatedRegistryProfile {
    physical: winreg::RegKey,
    name: String,
    _user: winreg::RegKey,
    _classes: winreg::RegKey,
    _effective: winreg::RegKey,
}
impl IsolatedRegistryProfile {
    fn new() -> Self {
        use windows::Win32::System::Registry::{
            HKEY, HKEY_CLASSES_ROOT as HKCR, HKEY_CURRENT_USER as HKCU, RegOverridePredefKey,
        };
        use winreg::enums::*;
        let physical = winreg::RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags("", KEY_ALL_ACCESS)
            .unwrap();
        let name = format!(
            "Software\\WindowsFileIconsDev\\GuiRegistryTests\\{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let (user, disposition) = physical.create_subkey(&name).unwrap();
        assert_eq!(disposition, REG_CREATED_NEW_KEY);
        let classes = user.create_subkey(r"Software\Classes").unwrap().0;
        let effective = user.create_subkey("EffectiveClasses").unwrap().0;
        let profile = Self {
            physical,
            name,
            _user: user,
            _classes: classes,
            _effective: effective,
        };
        unsafe {
            RegOverridePredefKey(HKCU, Some(HKEY(profile._user.raw_handle())))
                .ok()
                .unwrap();
            RegOverridePredefKey(HKCR, Some(HKEY(profile._effective.raw_handle())))
                .ok()
                .unwrap();
        }
        profile
    }
}
impl Drop for IsolatedRegistryProfile {
    fn drop(&mut self) {
        use windows::Win32::System::Registry::{
            HKEY_CLASSES_ROOT, HKEY_CURRENT_USER, RegOverridePredefKey,
        };
        unsafe {
            let _ = RegOverridePredefKey(HKEY_CLASSES_ROOT, None);
            let _ = RegOverridePredefKey(HKEY_CURRENT_USER, None);
        }
        let _ = self.physical.delete_subkey_all(&self.name);
    }
}

#[test]
fn shell_cleanup_gui_in_isolated_profile() {
    let profile = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "ui_tests::shell_cleanup_gui_in_child",
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
fn shell_cleanup_gui_in_child(cx: &mut TestAppContext) {
    use std::os::windows::fs::OpenOptionsExt;
    assert_eq!(std::env::var("WFI_GUI_PROBE").as_deref(), Ok("1"));
    let _registry = IsolatedRegistryProfile::new();
    let root = home().unwrap();
    std::fs::create_dir_all(&root).unwrap();
    let source = root.join("source.dll");
    std::fs::write(&source, b"test module contents").unwrap();
    let dll = file_icons_windows::stage_shell(&source).unwrap();
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&dll)
        .unwrap();
    let (window, manager) = cx.update(|cx| {
        gpui_kit::init(cx);
        theme::apply(ThemeMode::Dark, cx);
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1000.), px(900.)),
                    cx,
                ))),
                ..Default::default()
            },
            cx,
            Manager::open,
        )
        .unwrap()
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("en", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("about-open", cx);
        window.render_frame(cx);
        window.click("cleanup-shell", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(dll.is_file());
    assert_eq!(std::fs::read(&source).unwrap(), b"test module contents");
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window
                .find("status")
                .label()
                .unwrap()
                .contains("Could not finish cleaning up app files")
        );
        window.click("ja", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window
                .find("status")
                .label()
                .unwrap()
                .contains("アプリ用ファイルの整理を完了できません")
        );
    })
    .unwrap();
    drop(lock);
    cx.update_window(window, |_, window, cx| window.click("cleanup-shell", cx))
        .unwrap();
    cx.run_until_parked();
    assert!(!dll.parent().unwrap().exists());
    assert_eq!(std::fs::read(&source).unwrap(), b"test module contents");
    assert!(load().unwrap().journal.is_empty());
    // 画面で適用中を示す間は、誤って削除を開始しない。
    let dll = file_icons_windows::stage_shell(&source).unwrap();
    cx.update_window(window, |_, window, cx| {
        manager.update(cx, |view, _| {
            view.settings.active = Some("active/theme".into())
        });
        window.render_frame(cx);
        window.click("cleanup-shell", cx);
        manager.update(cx, |view, _| assert!(!view.operation.busy()));
    })
    .unwrap();
    assert!(dll.is_file());
    // 記録が消失しても、COM登録が存在する間はバックエンドも削除を拒否する。
    cx.update_window(window, |_, _, cx| {
        manager.update(cx, |view, _| view.settings.active = None)
    })
    .unwrap();
    let key = format!("CLSID\\{}\\InprocServer32", file_icons_windows::CLSID_TEXT);
    _registry
        ._classes
        .create_subkey(&key)
        .unwrap()
        .0
        .set_value("", &dll.to_string_lossy().as_ref())
        .unwrap();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("cleanup-shell", cx)
    })
    .unwrap();
    cx.run_until_parked();
    assert!(dll.is_file());
    _registry._classes.delete_subkey(&key).unwrap();
    // 利用者側にキーがなくても、有効なクラスの登録が残る場合は保持する。
    _registry
        ._effective
        .create_subkey(&key)
        .unwrap()
        .0
        .set_value("", &dll.to_string_lossy().as_ref())
        .unwrap();
    cx.update_window(window, |_, window, cx| window.click("cleanup-shell", cx))
        .unwrap();
    cx.run_until_parked();
    assert!(dll.is_file());
    _registry._effective.delete_subkey(&key).unwrap();
    cx.update_window(window, |_, window, cx| window.click("cleanup-shell", cx))
        .unwrap();
    cx.run_until_parked();
    assert!(!dll.exists());
}

mod registration;

mod targets;

mod target_visual;

mod extensionless_protection;
mod performance;
mod protection_recommendations;
mod protection_sources;
mod target_protection;
