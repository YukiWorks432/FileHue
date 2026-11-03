use super::*;
use crate::{Kind, ManagerService, Status};
use std::{fs, path::Path, sync::Arc};

fn fixture(root: &Path) {
    fs::create_dir_all(root).unwrap();
    fs::write(root.join("package.json"), r#"{"name":"local-import","publisher":"test","version":"1","license":"MIT","contributes":{"iconThemes":[{"id":"existing","label":"Existing pack","path":"theme.json"},{"id":"new","label":"Local pack","path":"theme.json"}]}}"#).unwrap();
    fs::write(root.join("theme.json"), r#"{"iconDefinitions":{"x":{"iconPath":"icon.svg"},"y":{"iconPath":"second.svg"}},"fileExtensions":{"json":"x","md":"y"}}"#).unwrap();
    for (file, color) in [("icon.svg", "#ff0000"), ("second.svg", "#0000ff")] {
        fs::write(root.join(file), format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="16" height="16" fill="{color}"/></svg>"#)).unwrap();
    }
}

fn source_bytes(root: &Path) -> Vec<Vec<u8>> {
    ["package.json", "theme.json", "icon.svg", "second.svg"]
        .iter()
        .map(|name| fs::read(root.join(name)).unwrap())
        .collect()
}

fn preferences() -> Vec<Vec<u8>> {
    ["settings.json", "settings.library.json"]
        .iter()
        .map(|name| fs::read(home().unwrap().join(name)).unwrap())
        .collect()
}

#[test]
fn local_import_gui_in_isolated_profile() {
    let profile = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "ui_tests::imports::local_import_gui_in_child",
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
fn local_import_gui_in_child(cx: &mut TestAppContext) {
    assert_eq!(std::env::var("WFI_GUI_PROBE").as_deref(), Ok("1"));
    let source = home().unwrap().join("local-fixture");
    fixture(&source);
    let original = source_bytes(&source);
    let existing_id = {
        let service = ManagerService::acquire().unwrap();
        let id = service
            .import(&source, "existing", &Default::default())
            .unwrap();
        let mut settings = load().unwrap();
        settings
            .selections
            .entry(id.clone())
            .or_default()
            .set("extension:json", Some(false))
            .unwrap();
        file_icons_windows::save(&settings).unwrap();
        id
    };
    let (window, manager) = cx.update(|cx| {
        gpui_kit::init(cx);
        theme::apply(ThemeMode::Dark, cx);
        gpui_kit::component::theme::Theme::update(cx, |t| t.font_size = px(18.));
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
            Manager::open,
        )
        .unwrap()
    });
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        w.click("import", cx);
    })
    .unwrap();
    cx.simulate_path_prompt_response(|_| Some(vec![source.clone()]));
    cx.run_until_parked();
    for language in ["ja", "en"] {
        cx.update_window(window, |_, w, cx| {
            w.render_frame(cx);
            w.click(language, cx);
        })
        .unwrap();
        cx.run_until_parked();
        let before = preferences();
        cx.update_window(window, |_, w, cx| {
            w.render_frame(cx);
            w.click(("theme", 1usize), cx);
            let progress = w.find("operation-progress");
            assert!(progress.visible());
            assert!(progress.label().unwrap().starts_with(if language == "en" {
                "Preparing and converting pack"
            } else {
                "パックを確認・変換中"
            }));
            assert!(w.find("cancel-download").visible());
            w.click("import", cx);
            w.click(if language == "en" { "ja" } else { "en" }, cx);
            assert_eq!(manager.read(cx).settings.language, language);
            if language == "en" {
                // 同じ取消操作をキーボードからも実行する。
                w.focus(&manager.read(cx).focus.clone(), cx);
                for _ in 0..40 {
                    if w.find("cancel-download").focused() == Some(true) {
                        break;
                    }
                    w.press("tab", cx);
                }
                assert_eq!(w.find("cancel-download").focused(), Some(true));
                w.press("enter", cx);
            } else {
                w.click("cancel-download", cx);
            }
            assert!(
                w.find("operation-progress")
                    .label()
                    .unwrap()
                    .contains(if language == "en" {
                        "Waiting for cancellation"
                    } else {
                        "キャンセルの完了"
                    })
            );
            // 採用版は無効状態をアクセシビリティ値に出さないため、再操作と結果で確かめる。
            w.click("cancel-download", cx);
            assert!(
                manager
                    .read(cx)
                    .operation
                    .progress()
                    .unwrap()
                    .is_cancelled()
            );
        })
        .unwrap();
        cx.run_until_parked();
        assert_eq!(preferences(), before);
        assert_eq!(source_bytes(&source), original);
        cx.update_window(window, |_, w, cx| {
            w.render_frame(cx);
            assert!(matches!(
                manager.read(cx).operation.status(),
                Status::Cancelled
            ));
            assert!(manager.read(cx).progress_refresh.is_none());
            assert!(w.find(("theme", 1usize)).visible());
            assert!(w.try_find("operation-progress").is_none());
        })
        .unwrap();
    }
    // 同じ候補からの再試行で初めて一覧を更新する。既存指定は保持する。
    cx.update_window(window, |_, w, cx| {
        w.click(("theme", 1usize), cx);
    })
    .unwrap();
    cx.run_until_parked();
    let settings = load().unwrap();
    assert_eq!(settings.packs.len(), 2);
    assert!(!settings.selections[&existing_id].enabled("one.json"));
    assert!(settings.active.is_none() && settings.journal.is_empty());
    assert_eq!(source_bytes(&source), original);
    // 保存確定の直後に中断要求が来ても、実際の成功結果をキャンセルへ変えない。
    let source_for_commit = source.clone();
    cx.update_window(window, |_, _, cx| {
        manager.update(cx, |view, cx| {
            let service = view._guard.as_ref().unwrap().clone();
            let progress = Arc::new(file_icons_catalog::Progress::default());
            let display = progress.clone();
            view.start(
                Kind::Import,
                move || {
                    service.import(&source_for_commit, "new", &progress)?;
                    progress.cancel();
                    Ok(())
                },
                false,
                Some(display),
                cx,
            );
        });
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(load().unwrap().packs.len(), 2);
    cx.update_window(window, |_, w, cx| {
        w.render_frame(cx);
        assert!(matches!(
            manager.read(cx).operation.status(),
            Status::Complete
        ));
        assert!(manager.read(cx).pending.is_none());
        w.click("import", cx);
    })
    .unwrap();
    cx.simulate_path_prompt_response(|_| Some(vec![source.clone()]));
    cx.run_until_parked();
    let before_close = preferences();
    let (progress, guard) = cx
        .update_window(window, |_, w, cx| {
            w.render_frame(cx);
            w.click(("theme", 1usize), cx);
            let view = manager.read(cx);
            let progress = view.operation.progress().unwrap().clone();
            let guard = Arc::downgrade(view._guard.as_ref().unwrap());
            w.remove_window();
            (progress, guard)
        })
        .unwrap();
    drop(manager);
    cx.update(|_| {});
    assert!(
        progress.is_cancelled(),
        "画面破棄から中断要求が届いていない"
    );
    assert!(
        guard.upgrade().is_some(),
        "残る背景作業の管理ロックを早期解放した"
    );
    assert!(ManagerService::acquire().is_err());
    cx.run_until_parked();
    assert_eq!(preferences(), before_close);
    assert_eq!(source_bytes(&source), original);
    assert!(guard.upgrade().is_none());
    assert!(ManagerService::acquire().is_ok());
}

#[test]
fn conversion_counts_distinct_images_and_preserves_revalidation() {
    let source = tempfile::tempdir().unwrap();
    fixture(source.path());
    let cache = tempfile::tempdir().unwrap();
    for _ in 0..2 {
        let mut observed = Vec::new();
        file_icons_core::import_observed(
            source.path(),
            "new",
            cache.path(),
            None,
            || Ok(()),
            |done, total| observed.push((done, total)),
        )
        .unwrap();
        assert_eq!(observed, [(0, 2), (1, 2), (2, 2)]);
    }
    // 通知が変換後に届いてから中断しても、出典を持つ完成パックは公開しない。
    let cache = tempfile::tempdir().unwrap();
    let progress = file_icons_catalog::Progress::default();
    let error = file_icons_core::import_observed(
        source.path(),
        "new",
        cache.path(),
        None,
        || progress.check(),
        |done, _| {
            if done == 1 {
                progress.cancel();
            }
        },
    )
    .unwrap_err();
    assert!(error.is::<file_icons_catalog::Cancelled>());
    assert!(
        std::fs::read_dir(cache.path()).unwrap().all(|entry| !entry
            .unwrap()
            .path()
            .join("provenance.json")
            .exists())
    );
}

#[test]
fn conversion_checkpoint_cancellation_does_not_publish_an_incomplete_pack() {
    let source = tempfile::tempdir().unwrap();
    fixture(source.path());
    let before = source_bytes(source.path());
    let cache = tempfile::tempdir().unwrap();
    let progress = file_icons_catalog::Progress::default();
    let mut converted = false;
    let result =
        file_icons_core::import_controlled(source.path(), "new", cache.path(), None, || {
            // 最初の実ICOを書き終えた次の確認箇所で中断する。時刻や待機に依存しない。
            converted = fs::read_dir(cache.path())?
                .filter_map(Result::ok)
                .any(|dir| {
                    fs::read_dir(dir.path())
                        .unwrap()
                        .filter_map(Result::ok)
                        .any(|entry| entry.path().extension().is_some_and(|e| e == "ico"))
                });
            if converted {
                progress.cancel();
            }
            progress.check()
        });
    assert!(converted);
    assert!(result.unwrap_err().is::<file_icons_catalog::Cancelled>());
    assert_eq!(source_bytes(source.path()), before);
    for bucket in fs::read_dir(cache.path()).unwrap() {
        assert!(!bucket.unwrap().path().join("provenance.json").exists());
    }
}
